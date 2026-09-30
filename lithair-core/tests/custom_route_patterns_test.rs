//! Issue #293: the documented `with_route_async` example, executed over HTTP.
use lithair_core::app::{response, LithairServer, Method, RouteRequest, StatusCode};
use tokio::{net::TcpListener, sync::oneshot};

#[tokio::test]
async fn documented_wildcard_route_extracts_and_validates_the_segment() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    // Same handler as the `with_route_async` doc example.
    let server = LithairServer::new()
        .with_admin_panel(false)
        .with_route_async(Method::POST, "/api/jobs/*/run", |req: RouteRequest| async move {
            let name = req.uri().path().split('/').nth(3).unwrap_or_default();
            if name.is_empty()
                || name.len() > 64
                || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                return Ok(response::json(
                    StatusCode::BAD_REQUEST,
                    r#"{"error":"invalid job name"}"#,
                ));
            }
            Ok(response::json_value(
                StatusCode::ACCEPTED,
                &serde_json::json!({ "job": name, "status": "queued" }),
            ))
        })
        .with_route_async(Method::GET, "/files/*", |_| async {
            Ok(response::json(StatusCode::OK, "{}"))
        })
        .build()
        .unwrap();
    let (stop, done) = oneshot::channel::<()>();
    let task = tokio::spawn(server.serve_with_listener(listener, async {
        let _ = done.await;
    }));
    let client = reqwest::Client::new();
    let post = |path: &str| client.post(format!("{url}{path}")).send();

    let queued = post("/api/jobs/nightly/run").await.unwrap();
    assert_eq!(queued.status(), 202);
    let body: serde_json::Value = queued.json().await.unwrap();
    assert_eq!(body, serde_json::json!({"job": "nightly", "status": "queued"}));

    // An encoded separator stays in the segment and fails validation.
    assert_eq!(post("/api/jobs/a%2Fb/run").await.unwrap().status(), 400);
    assert_eq!(post("/api/jobs//run").await.unwrap().status(), 400);
    // Wrong segment count or boundary never reaches the handler.
    for path in ["/api/jobs/a/b/run", "/api/jobs/nightly/runx", "/api/jobsx/nightly/run"] {
        assert_eq!(post(path).await.unwrap().status(), 404, "{path}");
    }
    // Suffix wildcards stop at the segment boundary.
    let get = |path: &str| client.get(format!("{url}{path}")).send();
    assert_eq!(get("/files/a/b").await.unwrap().status(), 200);
    for path in ["/files", "/filesx", "/filesx/a"] {
        assert_eq!(get(path).await.unwrap().status(), 404, "{path}");
    }
    let _ = stop.send(());
    task.await.unwrap().unwrap();
}
