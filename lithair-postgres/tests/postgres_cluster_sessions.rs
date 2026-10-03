//! RFC 308 with store B: three native nodes and one PostgreSQL. Users log in
//! through RBAC on any node, every node authorizes with the shared sessions,
//! a logout holds everywhere, and authentication survives the loss of the
//! native quorum. Separate binary: it installs the process-wide database.
#[path = "../../lithair-core/tests/support/operator_files.rs"]
#[allow(dead_code)]
mod operator_files;
#[path = "support/env.rs"]
mod support;
use lithair_core::{
    app::LithairServer,
    cluster::{
        native::{Model, NativeCluster},
        operator::{run, OperatorCommand},
    },
    rbac::{RbacUser, ServerRbacConfig},
    session::SessionManager,
};
use lithair_macros::DeclarativeModel;
use lithair_postgres::PostgresSessionStore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use support::*;
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
struct Ledger {
    #[db(primary_key)]
    id: String,
}
#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
#[storage(postgres, durable, collection = "session_invoices")]
struct Invoice {
    #[db(primary_key)]
    id: String,
    total: u64,
}

struct Node {
    cluster: NativeCluster,
    url: String,
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<anyhow::Result<()>>,
}

async fn serve(
    cluster: NativeCluster,
    data: &std::path::Path,
    sessions: Arc<PostgresSessionStore>,
) -> Node {
    let rbac = ServerRbacConfig::new()
        .with_roles(vec![("Admin".into(), vec!["*".into()])])
        .with_user(RbacUser::new("alice", "secret-pass", "Admin"));
    let server = LithairServer::new()
        .with_admin_panel(false)
        .with_native_cluster(cluster.clone())
        .with_sessions(SessionManager::from_arc(sessions))
        .with_rbac_config(rbac)
        .with_models_require_session(true)
        .with_model::<Invoice>(data.join("invoices").to_string_lossy(), "/api/invoices")
        .build()
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (stop, done) = oneshot::channel();
    let task = tokio::spawn(server.serve_with_listener(listener, async {
        let _ = done.await;
    }));
    let client = reqwest::Client::new();
    tokio::time::timeout(Duration::from_secs(10), async {
        while client.get(format!("{url}/health")).send().await.is_err() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    Node { cluster, url, stop: Some(stop), task }
}

async fn leader(nodes: &[Option<Node>]) -> usize {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            for (n, node) in nodes.iter().enumerate() {
                if let Some(node) = node {
                    if node.cluster.ready().await {
                        return n;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("a ready leader")
}

async fn stop(node: &mut Option<Node>) {
    let mut node = node.take().unwrap();
    let _ = node.stop.take().unwrap().send(());
    (&mut node.task).await.unwrap().unwrap();
    node.cluster.shutdown().await.unwrap();
}

async fn login(client: &reqwest::Client, url: &str) -> String {
    let resp = client
        .post(format!("{url}/auth/login"))
        .json(&json!({"username": "alice", "password": "secret-pass"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    resp.json::<Value>().await.unwrap()["session_token"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_node_authenticates_with_shared_sessions_even_without_quorum() {
    let Some(env) = env() else { return };
    let db = connect(&env).await;
    let sessions = Arc::new(PostgresSessionStore::new(&db).await.unwrap());
    db.install().unwrap();

    let files = operator_files::OperatorFiles::new();
    let mut listeners = Vec::new();
    for _ in 0..3 {
        listeners.push(TcpListener::bind("127.0.0.1:0").await.unwrap());
    }
    for path in files.configs.values() {
        let mut config = std::fs::read_to_string(path).unwrap();
        for (n, listener) in listeners.iter().enumerate() {
            config = config.replace(
                &format!("127.0.0.1:{}", 20001 + n),
                &listener.local_addr().unwrap().to_string(),
            );
        }
        std::fs::write(path, config).unwrap();
        run(OperatorCommand::Provision, path).await.unwrap();
    }
    let mut nodes = Vec::new();
    for (n, listener) in listeners.into_iter().enumerate() {
        let id = n as u64 + 1;
        let models = vec![Model::of::<Ledger>("s.ledger", "/api/ledger").unwrap()];
        let cluster =
            NativeCluster::open_with_listener(&files.configs[&id], "sessions-v1", models, listener)
                .await
                .unwrap();
        nodes.push(Some(
            serve(cluster, &files.root.path().join(format!("app-{id}")), Arc::clone(&sessions))
                .await,
        ));
    }
    nodes[0].as_ref().unwrap().cluster.bootstrap().await.unwrap();
    let lead = leader(&nodes).await;
    let (follower, other) = ((lead + 1) % 3, (lead + 2) % 3);
    let urls: Vec<String> = nodes.iter().map(|n| n.as_ref().unwrap().url.clone()).collect();
    let client = reqwest::Client::new();
    let get = |n: usize, path: &str, token: Option<&str>| {
        let mut req = client.get(format!("{}{path}", urls[n]));
        if let Some(token) = token {
            req = req.bearer_auth(token);
        }
        req.send()
    };

    // Log in on a follower; every node authorizes the session.
    let token = login(&client, &urls[follower]).await;
    for n in 0..3 {
        assert_eq!(get(n, "/api/invoices", None).await.unwrap().status(), 401, "node {n}");
        assert_eq!(get(n, "/api/invoices", Some(&token)).await.unwrap().status(), 200, "node {n}");
        let valid: Value =
            get(n, "/auth/validate", Some(&token)).await.unwrap().json().await.unwrap();
        assert_eq!(valid["valid"], true, "node {n}");
    }
    // Native models: the gate first, then consensus (the leader serves reads).
    assert_eq!(get(lead, "/api/ledger", None).await.unwrap().status(), 401);
    assert_eq!(get(lead, "/api/ledger", Some(&token)).await.unwrap().status(), 200);
    assert_eq!(get(follower, "/api/ledger", Some(&token)).await.unwrap().status(), 503);

    // Logout on another node holds everywhere, immediately.
    let logout = client
        .post(format!("{}/auth/logout", urls[other]))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(logout.status(), 200);
    for n in 0..3 {
        assert_eq!(get(n, "/api/invoices", Some(&token)).await.unwrap().status(), 401, "node {n}");
    }

    // Quorum lost: native models stop, but login and PostgreSQL models still
    // work on the surviving node, since sessions live in PostgreSQL.
    stop(&mut nodes[lead]).await;
    stop(&mut nodes[other]).await;
    let token = login(&client, &urls[follower]).await;
    assert_eq!(get(follower, "/api/invoices", Some(&token)).await.unwrap().status(), 200);
    assert_eq!(get(follower, "/api/ledger", Some(&token)).await.unwrap().status(), 503);
    stop(&mut nodes[follower]).await;
}
