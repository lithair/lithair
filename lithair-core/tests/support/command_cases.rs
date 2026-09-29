use lithair_core::cluster::{
    native::commands::{CommandStore, Decision, Write},
    operator::{run, OperatorCommand},
};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::net::TcpListener;
#[path = "command_example.rs"]
pub mod example;
use crate::cases::operator_files;

struct Group {
    files: operator_files::OperatorFiles,
    nodes: Vec<Option<CommandStore>>,
}
impl Group {
    async fn new() -> Self {
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
        let mut group = Self { files, nodes: vec![None, None, None] };
        for (n, listener) in listeners.into_iter().enumerate() {
            group.nodes[n] = Some(
                CommandStore::open_with_listener(
                    &group.files.configs[&(n as u64 + 1)],
                    example::CONTRACT,
                    example::COLLECTIONS,
                    listener,
                )
                .await
                .unwrap(),
            );
        }
        group.nodes[0].as_ref().unwrap().bootstrap().await.unwrap();
        group.leader().await;
        group
    }
    async fn leader(&self) -> usize {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                for (n, node) in self.nodes.iter().enumerate() {
                    if let Some(node) = node {
                        if node.ready().await {
                            return n;
                        }
                    }
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap()
    }
    async fn stop(&mut self, n: usize) {
        self.nodes[n].take().unwrap().shutdown().await.unwrap();
    }
    async fn start(&mut self, n: usize) {
        self.nodes[n] = Some(
            CommandStore::open(
                &self.files.configs[&(n as u64 + 1)],
                example::CONTRACT,
                example::COLLECTIONS,
            )
            .await
            .unwrap(),
        );
    }
}

pub async fn durable_commands() {
    let mut group = Group::new().await;
    let leader = group.leader().await;
    let store = group.nodes[leader].as_ref().unwrap().clone();
    let first = example::submit(&store, "tenant-a", "alice", "item", "first", 0).await.unwrap();
    let follower = (leader + 1) % 3;
    group.stop(follower).await;
    for n in 0..265 {
        example::submit(&store, "tenant-a", "alice", &format!("item-{n}"), &format!("key-{n}"), 0)
            .await
            .unwrap();
    }
    assert_eq!(
        example::submit(&store, "tenant-a", "alice", "item", "first", 0).await.unwrap(),
        first
    );
    assert!(example::submit(&store, "tenant-a", "alice", "other", "first", 0).await.is_err());
    // Another principal/tenant using the same key gets their own receipt.
    let other = example::submit(&store, "tenant-b", "alice", "item", "first", 0).await.unwrap();
    assert_ne!(first["id"], other["id"]);
    assert!(example::submit(&store, "tenant-a", "eve", "item", "first", 0).await.is_err());
    let (a, b) = tokio::join!(
        example::submit(&store, "tenant-a", "alice", "item", "update-a", 1),
        example::submit(&store, "tenant-a", "alice", "item", "update-b", 1)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert!(store
        .execute(|_| panic!("decision panic must not stop the runtime"))
        .await
        .is_err());
    let before = example::counts(&store).await.unwrap();
    assert_eq!(
        before,
        json!({"tasks":267,"operations":268,"events":268,"outbox":268,"receipts":268})
    );
    let too_large = store
        .execute(|_| {
            Ok(Decision::Commit {
                writes: vec![Write::put("tasks", "oversized", json!("x".repeat(256 * 1024)))],
                reply: Value::Null,
            })
        })
        .await;
    assert!(too_large.is_err());
    let invalid = store
        .execute(|_| {
            Ok(Decision::Commit {
                writes: vec![
                    Write::put("tasks", "partial", json!(true)),
                    Write::put("unknown", "key", json!(true)),
                ],
                reply: Value::Null,
            })
        })
        .await;
    assert!(invalid.is_err());
    assert_eq!(example::counts(&store).await.unwrap(), before);
    example::permission(&store, "tenant-a", "alice", false).await.unwrap();
    assert!(example::submit(&store, "tenant-a", "alice", "item", "first", 0).await.is_err());
    example::permission(&store, "tenant-a", "alice", true).await.unwrap();
    // Contract mismatch must refuse an existing store without damaging it.
    assert!(CommandStore::open(
        &group.files.configs[&(follower as u64 + 1)],
        "changed-v2",
        example::COLLECTIONS
    )
    .await
    .is_err());
    group.start(follower).await;
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if group.nodes[follower].as_ref().unwrap().inspect().await["snapshot_installs"]
                .as_u64()
                .unwrap()
                > 0
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(store);
    group.stop(leader).await;
    let elected = group.leader().await;
    let store = group.nodes[elected].as_ref().unwrap();
    assert_eq!(
        example::submit(store, "tenant-a", "alice", "item", "first", 0).await.unwrap(),
        first
    );
    assert_eq!(example::counts(store).await.unwrap(), before);
    let remaining = (0..3).find(|n| *n != leader && *n != elected).unwrap();
    group.stop(remaining).await;
    let isolated = group.nodes[elected].as_ref().unwrap();
    assert!(!isolated.ready().await);
    assert!(example::submit(isolated, "tenant-a", "alice", "unsafe", "no-quorum", 0)
        .await
        .is_err());
    assert!(example::counts(isolated).await.is_err());
    group.stop(elected).await;
    for n in 0..3 {
        group.start(n).await;
    }
    let elected = group.leader().await;
    let store = group.nodes[elected].as_ref().unwrap();
    assert_eq!(
        example::submit(store, "tenant-a", "alice", "item", "first", 0).await.unwrap(),
        first
    );
    assert_eq!(example::counts(store).await.unwrap(), before);
    for n in 0..3 {
        group.stop(n).await;
    }
}
