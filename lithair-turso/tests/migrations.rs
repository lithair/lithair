use lithair_macros::DeclarativeModel;
use lithair_turso::{Database, Error};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
#[storage(turso, collection = "notes")]
struct Legacy {
    id: String,
    title: String,
}

fn v2(document: &mut Value) -> lithair_turso::Result<()> {
    document["category"] = json!("work");
    Ok(())
}

#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
#[storage(turso, collection = "notes", version = 2, migrations(crate::v2))]
struct Current {
    id: String,
    title: String,
    category: String,
}

#[tokio::test]
async fn upgrade_survives_restart_and_rejects_old_handles() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("model.db");
    let db = Database::open(path.to_str().unwrap()).await.unwrap();
    let old = db.store::<Legacy>("tenant-a").unwrap();
    old.create(Legacy { id: "one".into(), title: "kept".into() }, &[])
        .await
        .unwrap();
    let current = db.store::<Current>("tenant-a").unwrap();
    current.prepare().await.unwrap();
    assert_eq!(current.get("one", &[]).await.unwrap().unwrap().category, "work");
    assert!(matches!(old.get("one", &[]).await, Err(Error::Schema(_))));
    assert!(matches!(old.delete("one", &[]).await, Err(Error::Schema(_))));
    drop((old, current, db));
    let db = Database::open(path.to_str().unwrap()).await.unwrap();
    let current = db.store::<Current>("tenant-a").unwrap();
    current.prepare().await.unwrap();
    assert_eq!(current.get("one", &[]).await.unwrap().unwrap().title, "kept");
    // Same declaration under another Rust name keeps the partition and does not
    // replay callbacks just because their function name changed.
    let renamed = db.store::<Once>("tenant-a").unwrap();
    renamed.prepare().await.unwrap();
    assert_eq!(renamed.get("one", &[]).await.unwrap().unwrap().title, "kept");
}

fn append_title(document: &mut Value) -> lithair_turso::Result<()> {
    v2(document)?;
    document["title"] = json!(format!("{} migrated", document["title"].as_str().unwrap()));
    Ok(())
}

#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
#[storage(turso, collection = "notes", version = 2, migrations(append_title))]
struct Once {
    id: String,
    title: String,
    category: String,
}

// Seed the exact 0.1 layout, without using the new adapter's schema machinery.
async fn legacy_file(path: &std::path::Path, count: usize) {
    let db = turso::Builder::new_local(path.to_str().unwrap()).build().await.unwrap();
    let conn = db.connect().unwrap();
    conn.execute("CREATE TABLE lithair_documents_v1 (namespace TEXT NOT NULL, model TEXT NOT NULL, id TEXT NOT NULL, body TEXT NOT NULL, PRIMARY KEY (namespace, model, id))", ()).await.unwrap();
    for (namespace, collection) in
        [("tenant-a", "notes"), ("tenant-b", "notes"), ("tenant-a", "other")]
    {
        for n in 0..count {
            let id = format!("{n:04}");
            let body =
                json!({"id":id,"title":"kept","rank":"7","extra":{"preserved":true}}).to_string();
            conn.execute(
                "INSERT INTO lithair_documents_v1 VALUES (?1, ?2, ?3, ?4)",
                turso::params![namespace, collection, id, body],
            )
            .await
            .unwrap();
        }
    }
}

#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
#[storage(turso, collection = "other")]
struct Other {
    id: String,
    title: String,
}

#[tokio::test]
async fn legacy_adoption_pages_are_isolated_and_completed_steps_do_not_repeat() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("model.db");
    legacy_file(&path, 205).await;
    for _ in 0..2 {
        let db = Database::open(path.to_str().unwrap()).await.unwrap();
        let store = db.store::<Once>("tenant-a").unwrap();
        store.prepare().await.unwrap();
        for n in 0..205 {
            let item = store.get(&format!("{n:04}"), &[]).await.unwrap().unwrap();
            assert_eq!(item.title, "kept migrated");
        }
        assert_eq!(
            db.store::<Legacy>("tenant-b")
                .unwrap()
                .get("0204", &[])
                .await
                .unwrap()
                .unwrap()
                .title,
            "kept"
        );
        assert_eq!(
            db.store::<Other>("tenant-a")
                .unwrap()
                .get("0204", &[])
                .await
                .unwrap()
                .unwrap()
                .title,
            "kept"
        );
    }
}

fn rename_and_convert(document: &mut Value) -> lithair_turso::Result<()> {
    let fields = document.as_object_mut().unwrap();
    let title = fields.remove("title").unwrap();
    fields.insert("label".into(), title);
    let rank = fields["rank"]
        .as_str()
        .ok_or_else(|| Error::Validation("rank must be text".into()))?
        .parse::<u32>()
        .map_err(|_| Error::Validation("rank must be numeric".into()))?;
    fields.insert("rank".into(), json!(rank));
    Ok(())
}

#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
#[storage(turso, collection = "notes", version = 3, migrations(v2, rename_and_convert))]
struct Third {
    id: String,
    label: String,
    category: String,
    rank: u32,
    extra: Value,
}

#[tokio::test]
async fn ordered_steps_support_direct_or_incremental_upgrades_without_losing_unknown_fields() {
    for intermediate in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("model.db");
        legacy_file(&path, 1).await;
        if intermediate {
            let db = Database::open(path.to_str().unwrap()).await.unwrap();
            db.store::<Current>("tenant-a").unwrap().prepare().await.unwrap();
        }
        let db = Database::open(path.to_str().unwrap()).await.unwrap();
        let store = db.store::<Third>("tenant-a").unwrap();
        store.prepare().await.unwrap();
        let item = store.get("0000", &[]).await.unwrap().unwrap();
        assert_eq!(item.label, "kept");
        assert_eq!(item.category, "work");
        assert_eq!(item.rank, 7);
        assert_eq!(item.extra, json!({"preserved":true}));
    }
}

fn fail_second_page(document: &mut Value) -> lithair_turso::Result<()> {
    if document["id"] == "0101" {
        return Err(Error::Validation("deliberate failure on second page".into()));
    }
    rename_and_convert(document)
}
#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
#[storage(turso, collection = "notes", version = 3, migrations(v2, fail_second_page))]
struct FailingThird {
    id: String,
    label: String,
    category: String,
    rank: u32,
}

#[tokio::test]
async fn failure_rolls_back_all_pages_and_steps_then_allows_retry_after_reopen() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("model.db");
    legacy_file(&path, 105).await;
    {
        let db = Database::open(path.to_str().unwrap()).await.unwrap();
        db.store::<First>("tenant-a").unwrap().prepare().await.unwrap();
        let err = db.store::<FailingThird>("tenant-a").unwrap().prepare().await.unwrap_err();
        assert!(err.to_string().contains("0101"));
        assert!(err.to_string().contains("deliberate failure"));
    }
    let db = Database::open(path.to_str().unwrap()).await.unwrap();
    let old = db.store::<First>("tenant-a").unwrap();
    for n in 0..105 {
        assert_eq!(old.get(&format!("{n:04}"), &[]).await.unwrap().unwrap().title, "kept");
    }
    let current = db.store::<Third>("tenant-a").unwrap();
    current.prepare().await.unwrap();
    assert_eq!(current.get("0104", &[]).await.unwrap().unwrap().rank, 7);
}

#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
#[storage(turso, collection = "notes", version = 1)]
struct First {
    id: String,
    title: String,
}
#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
#[storage(turso, collection = "notes", version = 1)]
struct Drift {
    id: String,
    title: String,
    #[serde(default)]
    unnoticed: bool,
}

#[tokio::test]
async fn tracked_schema_rejects_drift_and_downgrade_even_when_deserialization_would_succeed() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("model.db");
    legacy_file(&path, 1).await;
    let db = Database::open(path.to_str().unwrap()).await.unwrap();
    let first = db.store::<First>("tenant-a").unwrap();
    first.prepare().await.unwrap();
    let error = db.store::<Drift>("tenant-a").unwrap().prepare().await.unwrap_err();
    assert!(error.to_string().contains("without increasing version"));
    assert!(db.store::<Legacy>("tenant-a").unwrap().prepare().await.is_err());
    db.store::<Current>("tenant-a").unwrap().prepare().await.unwrap();
    assert!(first.get("0000", &[]).await.is_err());
    assert!(first.list(Default::default(), None, &[]).await.is_err());
    assert!(first
        .create(First { id: "new".into(), title: "stale".into() }, &[])
        .await
        .is_err());
    let error = db.store::<First>("tenant-a").unwrap().prepare().await.unwrap_err();
    assert!(error.to_string().contains("downgrade"));
}

fn panic_migration(_: &mut Value) -> lithair_turso::Result<()> {
    panic!("user callback panic")
}
fn change_id(doc: &mut Value) -> lithair_turso::Result<()> {
    v2(doc)?;
    doc["id"] = json!("different");
    Ok(())
}
fn invalid_model(doc: &mut Value) -> lithair_turso::Result<()> {
    v2(doc)?;
    doc["title"] = json!(42);
    Ok(())
}
fn invalid_validation(doc: &mut Value) -> lithair_turso::Result<()> {
    v2(doc)?;
    doc["title"] = json!("");
    Ok(())
}
fn oversized(doc: &mut Value) -> lithair_turso::Result<()> {
    v2(doc)?;
    doc["title"] = json!("x".repeat(lithair_turso::MAX_DOCUMENT_BYTES));
    Ok(())
}
fn non_object(doc: &mut Value) -> lithair_turso::Result<()> {
    *doc = json!([]);
    Ok(())
}

macro_rules! rejected_model {
    ($name:ident, $callback:path) => {
        #[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
        #[storage(
            turso,
            collection = "notes",
            namespace = "tenant-a",
            version = 2,
            migrations($callback)
        )]
        struct $name {
            id: String,
            #[http(validate = "non_empty")]
            title: String,
            category: String,
        }
    };
}
rejected_model!(Panicking, panic_migration);
rejected_model!(ChangedId, change_id);
rejected_model!(Invalid, invalid_model);
rejected_model!(InvalidValidation, invalid_validation);
rejected_model!(Oversized, oversized);
rejected_model!(NonObject, non_object);

#[tokio::test]
async fn invalid_results_and_panics_roll_back_and_leave_connection_usable() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("model.db");
    legacy_file(&path, 1).await;
    let db = Database::open(path.to_str().unwrap()).await.unwrap();
    let failures = [
        db.store::<Panicking>("tenant-a").unwrap().prepare().await,
        db.store::<ChangedId>("tenant-a").unwrap().prepare().await,
        db.store::<Invalid>("tenant-a").unwrap().prepare().await,
        db.store::<InvalidValidation>("tenant-a").unwrap().prepare().await,
        db.store::<Oversized>("tenant-a").unwrap().prepare().await,
        db.store::<NonObject>("tenant-a").unwrap().prepare().await,
    ];
    for failure in failures {
        assert!(matches!(failure, Err(Error::Schema(_))));
    }
    assert_eq!(
        db.store::<Legacy>("tenant-a")
            .unwrap()
            .get("0000", &[])
            .await
            .unwrap()
            .unwrap()
            .title,
        "kept"
    );
    db.store::<Current>("tenant-a").unwrap().prepare().await.unwrap();
}

#[tokio::test]
async fn empty_partitions_start_at_target_version_without_running_old_callbacks() {
    let tmp = tempfile::tempdir().unwrap();
    let db = Database::open(tmp.path().join("model.db").to_str().unwrap()).await.unwrap();
    let store = db.store::<Panicking>("tenant-a").unwrap();
    store.prepare().await.unwrap();
    store
        .create(
            Panicking { id: "new".into(), title: "kept".into(), category: "work".into() },
            &[],
        )
        .await
        .unwrap();
    assert_eq!(store.get("new", &[]).await.unwrap().unwrap().title, "kept");
}

async fn serve(
    builder: lithair_core::app::LithairServerBuilder,
) -> (
    String,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<anyhow::Result<()>>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(builder.build().unwrap().serve_with_listener(listener, async {
        let _ = stopped.await;
    }));
    (base, stop, task)
}

#[tokio::test]
async fn both_builders_migrate_before_http_and_preserve_updates_across_restarts() {
    use lithair_core::app::LithairServer;
    for declarative in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap();
        for boot in 0..3 {
            let path = tmp.path().join("notes").to_string_lossy().to_string();
            let builder = LithairServer::new().with_data_dir(tmp.path().to_string_lossy());
            let builder = match (boot, declarative) {
                (0, false) => builder.with_model::<Legacy>(path, "/notes"),
                (0, true) => builder.with_declarative_model::<Legacy>(path, "/notes"),
                (_, false) => builder.with_model::<Current>(path, "/notes"),
                (_, true) => builder.with_declarative_model::<Current>(path, "/notes"),
            };
            let (base, stop, task) = serve(builder).await;
            if boot == 0 {
                assert_eq!(
                    client
                        .post(format!("{base}/notes"))
                        .json(&json!({"id":"one","title":"kept"}))
                        .send()
                        .await
                        .unwrap()
                        .status(),
                    201
                );
            } else {
                let response = client.get(format!("{base}/notes/one")).send().await.unwrap();
                assert_eq!(response.status(), 200);
                let item: Value = response.json().await.unwrap();
                assert_eq!(item["category"], "work");
                assert_eq!(item["title"], if boot == 1 { "kept" } else { "updated" });
                assert_eq!(
                    client
                        .patch(format!("{base}/notes/one"))
                        .json(&json!({"title":"updated"}))
                        .send()
                        .await
                        .unwrap()
                        .status(),
                    200
                );
            }
            stop.send(()).unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(10), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
        }
    }
}

#[tokio::test]
async fn failed_migration_stops_server_startup_and_preserves_old_database() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("model.db");
    legacy_file(&path, 1).await;
    let builder = lithair_core::app::LithairServer::new()
        .with_data_dir(tmp.path().to_string_lossy())
        .with_model::<ChangedId>(tmp.path().to_string_lossy(), "/notes");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        builder.build().unwrap().serve_with_listener(listener, std::future::pending()),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert!(format!("{error:#}").contains("primary key"));
    let db = Database::open(path.to_str().unwrap()).await.unwrap();
    assert_eq!(
        db.store::<Legacy>("tenant-a")
            .unwrap()
            .get("0000", &[])
            .await
            .unwrap()
            .unwrap()
            .title,
        "kept"
    );
}

#[tokio::test]
async fn adopting_version_one_validates_legacy_data_without_advancing_on_failure() {
    // Drift requires a defaultable field, so it is a valid first adoption. Use
    // an incompatible title type to prove deserialization is checked first.
    #[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
    #[storage(turso, collection = "notes", version = 1)]
    struct Incompatible {
        id: String,
        title: u64,
    }
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("model.db");
    legacy_file(&path, 1).await;
    let db = Database::open(path.to_str().unwrap()).await.unwrap();
    assert!(matches!(
        db.store::<Incompatible>("tenant-a").unwrap().prepare().await,
        Err(Error::Schema(_))
    ));
    assert_eq!(
        db.store::<Legacy>("tenant-a")
            .unwrap()
            .get("0000", &[])
            .await
            .unwrap()
            .unwrap()
            .title,
        "kept"
    );
    db.store::<First>("tenant-a").unwrap().prepare().await.unwrap();
}
