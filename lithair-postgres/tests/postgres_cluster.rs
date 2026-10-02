//! RFC 296 Q5: PostgreSQL models next to a three-node native cluster. Any node
//! serves them, independently of Raft leadership and of quorum, while native
//! models keep their consensus rules. Separate binary: it installs the
//! process-wide database.
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
};
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use support::*;
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

/// Replicated through Raft: writes need the leader and a quorum.
#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
struct Ledger {
    #[db(primary_key)]
    id: String,
    note: String,
}
/// Authority in PostgreSQL, shared by every node.
#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
#[storage(postgres, durable, collection = "cluster_invoices")]
struct Invoice {
    #[db(primary_key)]
    id: String,
    total: u64,
}
/// A local (native) model: never allowed next to a native cluster.
#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
struct Local {
    #[db(primary_key)]
    id: String,
}

struct Node {
    cluster: NativeCluster,
    url: String,
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<anyhow::Result<()>>,
}

async fn open(config: &std::path::Path, listener: TcpListener) -> NativeCluster {
    let models = vec![Model::of::<Ledger>("pg.ledger", "/api/ledger").unwrap()];
    NativeCluster::open_with_listener(config, "pg-cluster-v1", models, listener)
        .await
        .unwrap()
}

async fn serve(cluster: NativeCluster, data: &std::path::Path) -> Node {
    let server = LithairServer::new()
        .with_admin_panel(false)
        .with_native_cluster(cluster.clone())
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn postgres_models_are_served_by_every_node_even_without_quorum() {
    let Some(env) = env() else { return };
    connect(&env).await.install().unwrap();

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

    // A local model, or a shared model on a native route, is refused.
    let refused = |builder: lithair_core::app::LithairServerBuilder| match builder.build() {
        Ok(_) => panic!("must be refused next to a native cluster"),
        Err(error) => error.to_string(),
    };
    let probe = open(&files.configs[&1], listeners.remove(0)).await;
    let local = refused(
        LithairServer::new()
            .with_admin_panel(false)
            .with_native_cluster(probe.clone())
            .with_model::<Local>(files.root.path().join("local").to_string_lossy(), "/api/local"),
    );
    assert!(local.contains("native consensus cannot mix"), "{local}");
    let overlap = refused(
        LithairServer::new()
            .with_admin_panel(false)
            .with_native_cluster(probe.clone())
            .with_model::<Invoice>(files.root.path().join("x").to_string_lossy(), "/api/ledger"),
    );
    assert!(overlap.contains("overlaps"), "{overlap}");

    let mut nodes = vec![Some(serve(probe, &files.root.path().join("app-1")).await)];
    for (n, listener) in listeners.into_iter().enumerate() {
        let id = n as u64 + 2;
        let cluster = open(&files.configs[&id], listener).await;
        nodes.push(Some(serve(cluster, &files.root.path().join(format!("app-{id}"))).await));
    }
    nodes[0].as_ref().unwrap().cluster.bootstrap().await.unwrap();
    let lead = leader(&nodes).await;
    let follower = (lead + 1) % 3;
    let other = (lead + 2) % 3;
    let client = reqwest::Client::new();
    let urls: Vec<String> = nodes.iter().map(|n| n.as_ref().unwrap().url.clone()).collect();
    let url = |n: usize, path: &str| format!("{}{path}", urls[n]);

    // A follower serves PostgreSQL writes; every node reads them.
    let id = uuid::Uuid::new_v4().to_string();
    let created = client
        .post(url(follower, "/api/invoices"))
        .json(&json!({"id": id, "total": 42}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);
    for n in [lead, other] {
        let got: Value = client
            .get(url(n, &format!("/api/invoices/{id}")))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(got["total"], 42);
    }
    // Native writes still follow consensus: a follower refuses them.
    let native = client
        .post(url(follower, "/api/ledger"))
        .header("idempotency-key", "k1")
        .json(&json!({"id": "a", "note": "x"}))
        .send()
        .await
        .unwrap();
    assert_eq!(native.status(), 503);
    let info: Value = client.get(url(lead, "/info")).send().await.unwrap().json().await.unwrap();
    let listed = info.to_string();
    assert!(listed.contains("/api/ledger") && listed.contains("/api/invoices"), "{listed}");

    // Quorum lost: native models stop, PostgreSQL models keep working.
    stop(&mut nodes[lead]).await;
    stop(&mut nodes[other]).await;
    let alone = |path: &str| url(follower, path);
    let ledger = client.get(alone("/api/ledger")).send().await.unwrap();
    assert_eq!(ledger.status(), 503);
    let survivor = uuid::Uuid::new_v4().to_string();
    let created = client
        .post(alone("/api/invoices"))
        .json(&json!({"id": survivor, "total": 7}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);
    let got = client.get(alone(&format!("/api/invoices/{id}"))).send().await.unwrap();
    assert_eq!(got.status(), 200);
    stop(&mut nodes[follower]).await;
}
