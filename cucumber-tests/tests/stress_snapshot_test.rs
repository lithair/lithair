//! Test runner pour les stress tests de snapshots
//!
//! Ce test vérifie les performances des snapshots à grande échelle
//! avec 10K, 100K, 500K et 1M événements.

use cucumber::World;
use cucumber_tests::features::world::LithairWorld;

#[tokio::main]
async fn main() {
    // Par défaut, on lance uniquement les tests rapides (@quick)
    // Pour lancer les tests complets, utilisez: cargo test --test stress_snapshot_test -- --tags @1m
    LithairWorld::cucumber()
        .max_concurrent_scenarios(1) // Séquentiel pour éviter conflits de fichiers
        .filter_run("features/performance/stress_snapshot_1m.feature", |_, _, sc| {
            // Par défaut, lancer uniquement les tests @quick
            // Les autres tests peuvent être lancés manuellement
            sc.tags.iter().any(|t| t == "quick")
        })
        .await;
}
