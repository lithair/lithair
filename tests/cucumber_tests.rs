use cucumber::cli;

#[tokio::main]
async fn main() {
    // Exécuter les tests Cucumber
    cli::Main::run()
        .await
        .expect("Erreur lors de l'exécution des tests Cucumber");
}
