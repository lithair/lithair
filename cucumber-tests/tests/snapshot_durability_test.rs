//! Test runner pour les tests de durabilité des snapshots
//!
//! Ce test vérifie que les snapshots fonctionnent correctement
//! pour accélérer la récupération après redémarrage.

use cucumber::World;
use cucumber_tests::features::world::LithairWorld;

#[tokio::main]
async fn main() {
    LithairWorld::cucumber()
        .max_concurrent_scenarios(1) // Séquentiel pour éviter conflits de fichiers
        .filter_run("features/performance/snapshot_durability.feature", |_, _, _| true)
        .await;
}
