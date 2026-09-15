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
#[then("native and SQL HTTP models coexist and survive server restart")]
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
                    .put(format!("{sql}?id=one"))
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
        if !first_boot {
            assert_eq!(
                client
                    .delete(format!("{sql}?id=one"))
                    .bearer_auth("test-archive-token")
                    .send()
                    .await
                    .expect("delete")
                    .status(),
                200
            );
            assert_eq!(
                client
                    .get(format!("{sql}?id=one"))
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

#[tokio::main]
async fn main() {
    TursoWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit("features/persistence/turso.feature")
        .await;
}
