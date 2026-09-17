use cucumber::{given, then, when, World};
use hybrid_storage::Archive;
use lithair_turso::{Database, Error, Mutation, Store};
use std::time::Duration;

#[derive(Default, World)]
struct TursoWorld {
    tmp: Option<tempfile::TempDir>,
    database: Option<Database>,
    store: Option<Store<Archive>>,
    rejected: bool,
}
impl Drop for TursoWorld {
    fn drop(&mut self) {
        self.store = None;
        self.database = None;
    }
}
impl std::fmt::Debug for TursoWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TursoWorld").finish_non_exhaustive()
    }
}
fn permissions() -> Vec<String> {
    vec!["ArchiveRead".into(), "ArchiveWrite".into()]
}
fn archive() -> Archive {
    Archive { id: "one".into(), title: "kept".into(), category: "work".into() }
}

#[given("an isolated Turso model store")]
async fn isolated(world: &mut TursoWorld) {
    world.tmp = Some(tempfile::tempdir().expect("tempdir"));
    reopen(world).await;
}
#[when("I reopen the Turso database")]
async fn reopen(world: &mut TursoWorld) {
    world.store = None;
    world.database = None;
    let path = world.tmp.as_ref().expect("tempdir").path().join("sql.db");
    let database = Database::open(path.to_str().expect("path")).await.expect("open");
    world.store = Some(database.store("tenant-a").expect("store"));
    world.database = Some(database);
}
#[when("I create a valid SQL document")]
async fn create(world: &mut TursoWorld) {
    world
        .store
        .as_ref()
        .expect("store")
        .create(archive(), &permissions())
        .await
        .expect("create");
}
#[then("the SQL document is readable")]
async fn readable(world: &mut TursoWorld) {
    let item = world
        .store
        .as_ref()
        .expect("store")
        .get("one", &permissions())
        .await
        .expect("get")
        .expect("document");
    assert_eq!(item.title, "kept");
}
#[when("I submit a SQL batch with a duplicate primary key")]
async fn duplicate(world: &mut TursoWorld) {
    let result = world
        .store
        .as_ref()
        .expect("store")
        .batch(vec![Mutation::Create(archive()), Mutation::Create(archive())], &permissions())
        .await;
    world.rejected = matches!(result, Err(Error::Conflict));
}
#[then("the SQL batch is rejected without partial data")]
async fn rollback(world: &mut TursoWorld) {
    assert!(world.rejected);
    assert!(world
        .store
        .as_ref()
        .expect("store")
        .get("one", &permissions())
        .await
        .expect("get")
        .is_none());
    reopen(world).await;
    assert!(world
        .store
        .as_ref()
        .expect("store")
        .get("one", &permissions())
        .await
        .expect("get")
        .is_none());
}
#[then("invalid SQL documents are rejected")]
async fn invalid(world: &mut TursoWorld) {
    let mut item = archive();
    item.title.clear();
    assert!(matches!(
        world.store.as_ref().expect("store").create(item, &permissions()).await,
        Err(Error::Validation(_))
    ));
}
#[then("unauthorized SQL reads and writes are denied")]
async fn unauthorized(world: &mut TursoWorld) {
    let store = world.store.as_ref().expect("store");
    assert!(matches!(store.create(archive(), &[]).await, Err(Error::Forbidden)));
    store.create(archive(), &permissions()).await.expect("authorized create");
    assert!(matches!(store.get("one", &[]).await, Err(Error::Forbidden)));
    assert!(matches!(store.delete("one", &[]).await, Err(Error::Forbidden)));
}

// Exercise the actual example over HTTP, including a full server restart.
#[then("native and SQL HTTP models coexist, survive restart and delete with 204 and no body")]
async fn mixed_http(world: &mut TursoWorld) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("client");
    for first_boot in [true, false] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let base = format!("http://{}", listener.local_addr().expect("address"));
        let builder = hybrid_storage::application(tmp.path(), "test-archive-token".into())
            .await
            .expect("application");
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let task =
            tokio::spawn(builder.build().expect("server").serve_with_listener(listener, async {
                let _ = stopped.await;
            }));
        let live = format!("{base}/api/tasks");
        let sql = format!("{base}/api/archives");
        assert_eq!(client.get(&sql).send().await.expect("unauthorized").status(), 401);
        if first_boot {
            assert_eq!(
                client
                    .post(&live)
                    .json(&serde_json::json!({"id":"one","title":"live"}))
                    .send()
                    .await
                    .expect("native create")
                    .status(),
                201
            );
            assert_eq!(
                client
                    .post(&sql)
                    .bearer_auth("test-archive-token")
                    .json(&archive())
                    .send()
                    .await
                    .expect("sql create")
                    .status(),
                201
            );
            let mut bad = archive();
            bad.id = "bad".into();
            bad.title.clear();
            assert_eq!(
                client
                    .post(&sql)
                    .bearer_auth("test-archive-token")
                    .json(&bad)
                    .send()
                    .await
                    .expect("validation")
                    .status(),
                400
            );
            let mut updated = archive();
            updated.title = "archived".into();
            assert_eq!(
                client
                    .put(format!("{sql}/one"))
                    .bearer_auth("test-archive-token")
                    .json(&updated)
                    .send()
                    .await
                    .expect("update")
                    .status(),
                200
            );
        }
        let native: serde_json::Value = client
            .get(format!("{live}/one"))
            .send()
            .await
            .expect("native read")
            .json()
            .await
            .expect("native JSON");
        // Native get-by-id returns the item itself.
        assert_eq!(native["title"], "live");
        let page: serde_json::Value = client
            .get(format!("{sql}?category=work&limit=1&offset=0"))
            .bearer_auth("test-archive-token")
            .send()
            .await
            .expect("sql list")
            .json()
            .await
            .expect("SQL JSON");
        assert_eq!(page["data"].as_array().expect("page").len(), 1);
        assert_eq!(page["data"][0]["title"], "archived");
        assert!(tmp.path().join("archives/model.db").exists());
        assert!(!tmp.path().join("archives/events.raftlog").exists());
        assert_eq!(
            client
                .patch(format!("{sql}/one"))
                .bearer_auth("test-archive-token")
                .json(&serde_json::json!({"id": "changed"}))
                .send()
                .await
                .expect("immutable key")
                .status(),
            400
        );
        assert_eq!(
            client
                .patch(format!("{sql}/one"))
                .bearer_auth("test-archive-token")
                .json(&serde_json::json!({"category": "work"}))
                .send()
                .await
                .expect("generated patch")
                .status(),
            200
        );
        if !first_boot {
            let deleted = client
                .delete(format!("{sql}/one"))
                .bearer_auth("test-archive-token")
                .send()
                .await
                .expect("SQL delete");
            assert_eq!(deleted.status(), 204);
            assert!(deleted.bytes().await.expect("SQL delete body").is_empty());
            assert_eq!(
                client
                    .get(format!("{sql}/one"))
                    .bearer_auth("test-archive-token")
                    .send()
                    .await
                    .expect("missing")
                    .status(),
                404
            );
            assert_eq!(
                client
                    .get(format!("{live}/one"))
                    .send()
                    .await
                    .expect("native still present")
                    .status(),
                200
            );
            let deleted = client.delete(format!("{live}/one")).send().await.expect("native delete");
            assert_eq!(deleted.status(), 204);
            assert!(deleted.bytes().await.expect("native delete body").is_empty());
        }
        stop.send(()).expect("shutdown");
        tokio::time::timeout(Duration::from_secs(15), task)
            .await
            .expect("shutdown deadline")
            .expect("task")
            .expect("server result");
    }
    world.rejected = false;
}

#[then("the SQL HTTP list provides a continuation until all 55 documents are read")]
async fn paginated_http(_world: &mut TursoWorld) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/api/archives", listener.local_addr().unwrap());
    let builder = hybrid_storage::application(tmp.path(), "pagination-token".into())
        .await
        .unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(builder.build().unwrap().serve_with_listener(listener, async {
        let _ = stopped.await;
    }));
    for id in 0..55 {
        let item = Archive { id: format!("{id:03}"), ..archive() };
        assert_eq!(
            client
                .post(&url)
                .bearer_auth("pagination-token")
                .json(&item)
                .send()
                .await
                .unwrap()
                .status(),
            201
        );
    }
    let mut ids = Vec::new();
    let mut offset = 0;
    for (count, next) in [(50, Some(50)), (5, None)] {
        let response = client
            .get(format!("{url}?category=work&offset={offset}"))
            .bearer_auth("pagination-token")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let page: serde_json::Value = response.json().await.unwrap();
        let items = page["data"].as_array().unwrap();
        assert_eq!(items.len(), count);
        ids.extend(items.iter().map(|item| item["id"].as_str().unwrap().to_owned()));
        assert_eq!(page["has_more"], serde_json::json!(next.is_some()));
        assert_eq!(page.get("next_offset"), Some(&serde_json::json!(next)));
        if let Some(next) = page["next_offset"].as_u64() {
            offset = next;
        }
    }
    assert_eq!(ids, (0..55).map(|id| format!("{id:03}")).collect::<Vec<_>>());
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(15), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[derive(Clone, serde::Serialize, serde::Deserialize, lithair_core::DeclarativeModel)]
#[storage(turso, collection = "evolving")]
struct OriginalDocument {
    id: String,
    title: String,
}

fn add_category(document: &mut serde_json::Value) -> lithair_turso::Result<()> {
    document["category"] = serde_json::json!("work");
    Ok(())
}

fn reject_migration(document: &mut serde_json::Value) -> lithair_turso::Result<()> {
    add_category(document)?;
    if document["id"] == "two" {
        return Err(Error::Validation("rejected migration".into()));
    }
    Ok(())
}

#[derive(Clone, serde::Serialize, serde::Deserialize, lithair_core::DeclarativeModel)]
#[storage(turso, collection = "evolving", version = 2, migrations(add_category))]
struct EvolvedDocument {
    id: String,
    title: String,
    category: String,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, lithair_core::DeclarativeModel)]
#[storage(turso, collection = "evolving", version = 2, migrations(reject_migration))]
struct RejectedDocument {
    id: String,
    title: String,
    category: String,
}

#[given("SQL documents stored by the first application version")]
async fn original_schema(world: &mut TursoWorld) {
    isolated(world).await;
    let store = world.database.as_ref().unwrap().store::<OriginalDocument>("tenant-a").unwrap();
    for id in ["one", "two"] {
        store
            .create(OriginalDocument { id: id.into(), title: "kept".into() }, &[])
            .await
            .unwrap();
    }
}

#[when("the application upgrades its declared SQL model")]
async fn upgrade_schema(world: &mut TursoWorld) {
    world
        .database
        .as_ref()
        .unwrap()
        .store::<EvolvedDocument>("tenant-a")
        .unwrap()
        .prepare()
        .await
        .expect("upgrade");
}

#[then("the migrated documents and schema survive another restart")]
async fn migrated_schema(world: &mut TursoWorld) {
    reopen(world).await;
    let store = world.database.as_ref().unwrap().store::<EvolvedDocument>("tenant-a").unwrap();
    store.prepare().await.expect("already migrated");
    for id in ["one", "two"] {
        let value = store.get(id, &[]).await.unwrap().unwrap();
        assert_eq!(value.title, "kept");
        assert_eq!(value.category, "work");
    }
    assert!(matches!(
        world
            .database
            .as_ref()
            .unwrap()
            .store::<OriginalDocument>("tenant-a")
            .unwrap()
            .prepare()
            .await,
        Err(Error::Schema(_))
    ));
}

#[when("a declared SQL migration fails")]
async fn rejected_schema(world: &mut TursoWorld) {
    world.rejected = world
        .database
        .as_ref()
        .unwrap()
        .store::<RejectedDocument>("tenant-a")
        .unwrap()
        .prepare()
        .await
        .is_err();
}

#[then("the previous SQL model can still read every original document")]
async fn original_schema_preserved(world: &mut TursoWorld) {
    assert!(world.rejected);
    reopen(world).await;
    let store = world.database.as_ref().unwrap().store::<OriginalDocument>("tenant-a").unwrap();
    for id in ["one", "two"] {
        assert_eq!(store.get(id, &[]).await.unwrap().unwrap().title, "kept");
    }
}

#[tokio::main]
async fn main() {
    TursoWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit("features/persistence/turso.feature")
        .await;
}
