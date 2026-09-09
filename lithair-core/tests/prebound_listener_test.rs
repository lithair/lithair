//! Issue #207: the socket must stay reserved between choosing a port and serving.
use lithair_core::app::LithairServer;
use std::time::Duration;
use tokio::net::TcpListener;

#[tokio::test]
async fn serves_reserved_socket_and_releases_it_after_shutdown() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("reserve listener");
    let addr = listener.local_addr().expect("actual address");
    assert!(TcpListener::bind(addr).await.is_err(), "socket stays reserved");
    let server = LithairServer::new()
        .with_data_dir(tmp.path().to_string_lossy().to_string())
        .build()
        .expect("build server");
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(server.serve_with_listener(listener, async move {
        let _ = stopped.await;
    }));
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("client")
        .get(format!("http://{addr}/health"))
        .send()
        .await
        .expect("request on reserved socket");
    assert!(response.status().is_success());
    stop.send(()).expect("signal shutdown");
    tokio::time::timeout(Duration::from_secs(15), task)
        .await
        .expect("shutdown deadline")
        .expect("server task")
        .expect("serve result");
    let _rebound = TcpListener::bind(addr).await.expect("listener released after shutdown");
}
