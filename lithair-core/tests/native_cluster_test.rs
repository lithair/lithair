#![cfg(all(feature = "cluster", feature = "tls"))]
#[path = "support/native_cluster_cases.rs"]
mod cases;
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn generated_crud_is_ordered_durable_and_idempotent() {
    cases::crud_and_retry().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn acknowledged_models_and_results_survive_failover_and_cold_restart() {
    cases::failover_and_cold_recovery().await;
}

#[derive(Clone, serde::Serialize, serde::Deserialize, lithair_core::DeclarativeModel)]
struct Renamed {
    #[db(primary_key)]
    #[serde(rename = "key")]
    id: String,
}
#[derive(Clone, serde::Serialize, serde::Deserialize, lithair_core::DeclarativeModel)]
#[retention(memory = 10)]
struct Evicting {
    #[db(primary_key)]
    id: String,
}
#[derive(Clone, serde::Serialize, serde::Deserialize, lithair_core::DeclarativeModel)]
struct Audited {
    #[db(primary_key)]
    id: String,
    #[lifecycle(audited)]
    name: String,
}
#[test]
fn unsupported_model_policies_fail_before_opening_storage() {
    use lithair_core::cluster::native::Model;
    assert!(Model::of::<Renamed>("renamed", "/api/renamed").is_err());
    assert!(Model::of::<Evicting>("evicting", "/api/evicting").is_err());
    assert!(Model::of::<Audited>("audited", "/api/audited").is_err());
    assert!(Model::of::<cases::Record>("", "/api/records").is_err());
    assert!(Model::of::<cases::Record>("records", "/api/../records").is_err());
}
