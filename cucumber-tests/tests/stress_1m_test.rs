use cucumber::World;
use cucumber_tests::features::world::LithairWorld;

#[tokio::main]
async fn main() {
    // Exécuter les STRESS TESTS 1M avec vérification complète
    LithairWorld::cucumber()
        .max_concurrent_scenarios(1) // Un seul scénario à la fois pour éviter conflits de ports
        .filter_run("features/performance/stress_1m_test.feature", |_, _, _| {
            // Lancer tous les scénarios de stress test
            true
        })
        .await;
}
