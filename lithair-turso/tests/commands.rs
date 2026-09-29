//! Application commands: a trusted sample app commits item, operation, event,
//! outbox and receipt together, and resolves retries through its own receipts.
use lithair_macros::DeclarativeModel;
use lithair_turso::{Decision, Error, Write};
use serde_json::{json, Value};
use std::{future::Future, time::Duration};
#[path = "support/command_app.rs"]
mod app;
use app::*;

#[tokio::test]
async fn commands_commit_every_collection_and_replay_durable_receipts() {
    durable_receipts().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_keys_and_revisions_resolve_to_one_committed_outcome() {
    let tmp = tempfile::tempdir().unwrap();
    let (_db, commands) = open(&tmp.path().join("commands.db"), "tenant-a").await;
    let mut same_key = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let commands = commands.clone();
        same_key.spawn(async move { submit(&commands, "alice", "item", "k", 0, "one").await });
    }
    let mut replies = Vec::new();
    while let Some(reply) = same_key.join_next().await {
        replies.push(reply.unwrap().unwrap());
    }
    assert!(replies.iter().all(|r| *r == replies[0]));
    assert_eq!(counts(&commands).await, uniform(1));

    // Different keys racing on the same revision: exactly one wins.
    let mut racing = tokio::task::JoinSet::new();
    for n in 0..8 {
        let commands = commands.clone();
        racing.spawn(async move {
            submit(&commands, "alice", "item", &format!("race-{n}"), 1, "two").await
        });
    }
    let mut won = 0;
    while let Some(reply) = racing.join_next().await {
        match reply.unwrap() {
            Ok(_) => won += 1,
            Err(Error::Command(e)) => assert_eq!(e.to_string(), "revision conflict"),
            Err(e) => panic!("{e}"),
        }
    }
    assert_eq!(won, 1);
    assert_eq!(counts(&commands).await["items"], 1);
    assert_eq!(counts(&commands).await["receipts"], 2);
}

#[tokio::test]
async fn current_policy_is_checked_before_replay_and_namespaces_are_isolated() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("commands.db");
    let (db, commands) = open(&path, "tenant-a").await;
    let first = submit(&commands, "alice", "item", "first", 0, "one").await.unwrap();
    permission(&commands, "alice", false).await;
    let revoked = submit(&commands, "alice", "item", "first", 0, "one").await;
    assert!(matches!(revoked, Err(Error::Command(e)) if e.to_string() == "forbidden"));
    permission(&commands, "alice", true).await;
    assert_eq!(submit(&commands, "alice", "item", "first", 0, "one").await.unwrap(), first);

    db.store::<Item>("tenant-b").unwrap().prepare().await.unwrap();
    let other = db.commands("tenant-b", COLLECTIONS).unwrap();
    assert_eq!(counts(&other).await, uniform(0));
    let theirs = submit(&other, "alice", "item", "first", 0, "one").await.unwrap();
    assert_eq!(theirs, first, "same scoped key, separate partition");
    assert_eq!(counts(&other).await, uniform(1));
    assert_eq!(counts(&commands).await, uniform(1));
    let undeclared = db.commands("tenant-a", &["items"]).unwrap();
    let read = undeclared
        .execute(|view| {
            Box::pin(async move { Ok(Decision::Read(json!(view.get("receipts", "x").await?))) })
        })
        .await;
    assert!(matches!(read, Err(Error::Command(_))));
    assert!(db.commands("tenant-a", &["items", "items"]).is_err());
    assert!(db.commands("", COLLECTIONS).is_err());
}

fn commit(
    writes: Vec<Write>,
) -> impl for<'a> FnOnce(&'a lithair_turso::View<'a>) -> lithair_turso::Decided<'a> + Send + 'static
{
    move |_| Box::pin(async move { Ok(Decision::Commit { writes, reply: Value::Null }) })
}

#[tokio::test]
async fn failed_decisions_and_invalid_batches_roll_back_everything() {
    let tmp = tempfile::tempdir().unwrap();
    let (db, commands) = open(&tmp.path().join("commands.db"), "tenant-a").await;
    submit(&commands, "alice", "item", "first", 0, "one").await.unwrap();
    let before = counts(&commands).await;
    let partial = || Write::put("outbox", "partial", json!({}));
    let item = |title: &str| Item { id: "item".into(), title: title.into(), revision: 9 };

    let rejected = [
        // Decision errors and panics, synchronous or after an await.
        commands
            .execute(|_| Box::pin(async move { anyhow::bail!("rejected by policy") }))
            .await,
        commands.execute(|_| panic!("synchronous decision panic")).await,
        commands
            .execute(|view| {
                Box::pin(async move {
                    view.get("items", "item").await?;
                    panic!("asynchronous decision panic")
                })
            })
            .await,
        // Invalid batches, refused before the first statement.
        commands.execute(commit(vec![])).await,
        commands
            .execute(commit(
                (0..101).map(|n| Write::put("events", n.to_string(), json!({}))).collect(),
            ))
            .await,
        commands
            .execute(commit(vec![partial(), Write::put("missing", "k", json!({}))]))
            .await,
        commands.execute(commit(vec![partial(), partial()])).await,
        commands
            .execute(commit(vec![partial(), Write::put("events", "", json!({}))]))
            .await,
        commands
            .execute(commit(vec![partial(), Write::put("events", "k", json!(true))]))
            .await,
        commands
            .execute(commit(vec![
                partial(),
                Write::put("events", "k", json!({"x": "y".repeat(1 << 20)})),
            ]))
            .await,
        // Typed partitions refuse raw and wrongly-versioned writes mid-batch.
        commands
            .execute(commit(vec![partial(), Write::put("items", "item", json!({}))]))
            .await,
        commands
            .execute(commit(vec![
                partial(),
                Write::model(&ItemV2 { id: "item".into(), title: "v2".into(), revision: 9 })
                    .unwrap(),
            ]))
            .await,
    ];
    for result in rejected {
        assert!(result.is_err());
    }
    assert!(Write::model(&item("")).is_err(), "model validation runs");
    let wrong_schema = commands
        .execute(|view| {
            Box::pin(async move {
                Ok(Decision::Read(json!(view.model::<ItemV2>("item").await?.is_some())))
            })
        })
        .await;
    assert!(matches!(wrong_schema, Err(Error::Command(_))));
    assert_eq!(counts(&commands).await, before);
    let store = db.store::<Item>("tenant-a").unwrap();
    assert_eq!(store.get("item", &[]).await.unwrap().unwrap().revision, 1);
    // The connection is still usable after every rollback.
    commands
        .execute(commit(vec![
            Write::model(&item("fine")).unwrap(),
            Write::delete("outbox", "absent"),
        ]))
        .await
        .unwrap();
    assert_eq!(store.get("item", &[]).await.unwrap().unwrap().revision, 9);
}

#[tokio::test]
async fn a_dropped_caller_does_not_cancel_an_admitted_command() {
    let tmp = tempfile::tempdir().unwrap();
    let (_db, commands) = open(&tmp.path().join("commands.db"), "tenant-a").await;
    // Hold the connection inside a decision so the next command queues behind it.
    let (entered, inside) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    let holder = commands.clone();
    let blocker = tokio::spawn(async move {
        holder
            .execute(move |_| {
                Box::pin(async move {
                    entered.send(()).unwrap();
                    released.await?;
                    Ok(Decision::Read(Value::Null))
                })
            })
            .await
    });
    inside.await.unwrap();
    // Admit a command, then drop its future as a lost response would.
    let mut lost = Box::pin(submit(&commands, "alice", "item", "lost", 0, "one"));
    std::future::poll_fn(|cx| {
        assert!(lost.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    drop(lost);
    release.send(()).unwrap();
    blocker.await.unwrap().unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while counts(&commands).await != uniform(1) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the admitted command committed without its caller");
    // The retry resolves through the receipt instead of committing twice.
    let retried = submit(&commands, "alice", "item", "lost", 0, "one").await.unwrap();
    assert_eq!(retried["revision"], 1);
    assert_eq!(counts(&commands).await, uniform(1));
}

#[tokio::test]
async fn crash_child() {
    let Ok(path) = std::env::var("LITHAIR_TURSO_COMMAND_CHILD") else {
        return;
    };
    let (_db, commands) = open(std::path::Path::new(&path), "tenant-a").await;
    let start = counts(&commands).await["items"].as_u64().unwrap();
    for n in start.. {
        submit(&commands, "alice", &format!("item-{n}"), &format!("key-{n}"), 0, "t")
            .await
            .unwrap();
        if n == start {
            println!("committed");
        }
    }
}

#[tokio::test]
async fn process_death_keeps_every_command_all_or_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("commands.db");
    let mut committed = 0;
    for delay in [0, 150, 400] {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "crash_child", "--nocapture"])
            .env("LITHAIR_TURSO_COMMAND_CHILD", &path)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        // Kill only once the child is committing, at a varying point after.
        let mut lines =
            std::io::BufRead::lines(std::io::BufReader::new(child.stdout.take().unwrap()));
        assert!(lines.any(|line| line.unwrap() == "committed"), "child exited early");
        tokio::time::sleep(Duration::from_millis(delay)).await;
        child.kill().unwrap();
        child.wait().unwrap();
        let (_db, commands) = open(&path, "tenant-a").await;
        let now = counts(&commands).await;
        let items = now["items"].as_u64().unwrap() as usize;
        assert_eq!(now, uniform(items), "a command committed partially");
        assert!(items > committed, "the confirmed commit survived");
        committed = items;
    }
}
