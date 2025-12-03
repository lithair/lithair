//! Test runner pour les tests de durabilité multi-fichiers
//!
//! Ce test vérifie que chaque structure de données a son propre fichier
//! avec CRC32 validé pour l'intégrité des données.

use cucumber::World;
use cucumber_tests::features::world::LithairWorld;

#[tokio::main]
async fn main() {
    LithairWorld::cucumber()
        .max_concurrent_scenarios(1) // Séquentiel pour éviter conflits de fichiers
        .filter_run("features/performance/multi_file_durability.feature", |_, _, _| true)
        .await;
}
