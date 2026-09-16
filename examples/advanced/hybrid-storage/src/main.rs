#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let token = std::env::var("ARCHIVE_TOKEN")?;
    let data = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data/hybrid".into());
    let port: u16 = std::env::var("PORT").unwrap_or_else(|_| "8080".into()).parse()?;
    hybrid_storage::application(std::path::Path::new(&data), token)
        .await?
        .with_host("127.0.0.1")
        .with_port(port)
        .serve_with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
}
