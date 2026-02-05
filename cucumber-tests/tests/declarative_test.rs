use cucumber::World;

mod steps {
    pub mod declarative_steps;
}

#[tokio::main]
async fn main() {
    // Exécuter SEULEMENT le nouveau test pour validation rapide
    steps::declarative_steps::DeclarativeWorld::cucumber()
        .run_and_exit("features/declarative_engine.feature")
        .await;
}
