//! Settings exported by scripts/postgres-tests.sh, shared by the test binaries.
#![allow(dead_code)]
use lithair_postgres::{Database, PostgresConfig};

pub struct Env {
    pub url: String,
    pub isolated: String,
    pub ca: String,
    pub other_ca: String,
    pub restart: Option<String>,
}
pub fn env() -> Option<Env> {
    let var = |name: &str| std::env::var(name).ok();
    match (
        var("LITHAIR_TEST_POSTGRES_URL"),
        var("LITHAIR_TEST_POSTGRES_ISOLATED_URL"),
        var("LITHAIR_TEST_POSTGRES_CA"),
        var("LITHAIR_TEST_POSTGRES_OTHER_CA"),
    ) {
        (Some(url), Some(isolated), Some(ca), Some(other_ca)) => {
            Some(Env { url, isolated, ca, other_ca, restart: var("LITHAIR_TEST_POSTGRES_RESTART") })
        }
        _ if var("LITHAIR_REQUIRE_POSTGRES").is_some() => {
            panic!("LITHAIR_TEST_POSTGRES_* settings are missing")
        }
        _ => {
            eprintln!("skipped: run scripts/postgres-tests.sh (cidx run postgres-test)");
            None
        }
    }
}
pub async fn connect(env: &Env) -> Database {
    Database::connect(PostgresConfig::new(&env.url).with_ca_file(&env.ca))
        .await
        .unwrap()
}

/// A plain (non-pooled) admin connection over loopback, for fault injection.
pub async fn tokio_postgres_connect(
    url: &str,
) -> (
    tokio_postgres::Client,
    impl std::future::Future<Output = Result<(), tokio_postgres::Error>>,
) {
    // Plain loopback is accepted by the test server (see the script's pg_hba).
    tokio_postgres::connect(&format!("{} sslmode=disable", url_to_kv(url)), tokio_postgres::NoTls)
        .await
        .unwrap()
}
pub fn url_to_kv(url: &str) -> String {
    let parsed = url::Url::parse(url).unwrap();
    format!(
        "host={} port={} user={} password={} dbname={}",
        parsed.host_str().unwrap(),
        parsed.port().unwrap_or(5432),
        parsed.username(),
        parsed.password().unwrap_or_default(),
        parsed.path().trim_start_matches('/'),
    )
}
