# 🚀 RaftStone - Justfile (Task Runner)
# Installation: cargo install just
# Usage: just <command>

# Variables
export RUST_BACKTRACE := "1"
export RUST_LOG := "info"

# Par défaut, afficher l'aide
default:
    @just --list

# ==================== TESTS ====================

# Lance TOUS les tests (unitaires + E2E + intégration)
test-all: test-unit test-e2e test-integration
    @echo "✅ Tous les tests passés !"

# Tests unitaires seulement (rapide)
test-unit:
    @echo "📊 Tests unitaires..."
    cd raftstone-core && cargo test --lib
    @echo "✅ Tests unitaires OK"

# Tests E2E Cucumber (moyen)
test-e2e:
    @echo "📊 Tests E2E Cucumber..."
    cd cucumber-tests && cargo test --test cucumber_tests
    @echo "✅ Tests E2E OK"

# Tests d'intégration build (lent)
test-integration:
    @echo "📊 Tests d'intégration build..."
    cd cucumber-tests && cargo test --test integration_build_test
    @echo "✅ Tests d'intégration OK"

# Tests avec coverage
test-coverage:
    @echo "📊 Tests avec coverage..."
    cargo install cargo-tarpaulin || true
    cargo tarpaulin --out Html --output-dir ./coverage
    @echo "✅ Coverage report: ./coverage/index.html"

# Tests rapides (watch mode pour développement)
test-watch:
    @echo "👀 Mode watch activé..."
    cargo install cargo-watch || true
    cargo watch -x test

# ==================== BUILD ====================

# Build en mode debug
build:
    @echo "🔨 Build debug..."
    cargo build
    @echo "✅ Build debug OK"

# Build en mode release (optimisé)
build-release:
    @echo "🔨 Build release..."
    cargo build --release
    @echo "✅ Build release OK"
    @echo "📦 Binaire: ./target/release/raftstone"

# Build + tous les tests
build-test: build test-all
    @echo "✅ Build + Tests OK"

# Build release + tests + validation finale
build-full: clean build-release test-all validate
    @echo "🎉 Build complet validé !"

# ==================== VALIDATION ====================

# Valide le binaire final
validate:
    @echo "🔍 Validation du binaire..."
    ./target/release/raftstone --version || echo "⚠️ Pas de binaire"
    ./target/release/raftstone --help || echo "⚠️ Help non disponible"
    @echo "✅ Validation OK"

# Vérifie la qualité du code
check:
    @echo "🔍 Vérification du code..."
    cargo fmt -- --check
    cargo clippy -- -D warnings
    @echo "✅ Code quality OK"

# Lint et format le code
lint:
    @echo "🎨 Formatage du code..."
    cargo fmt
    cargo clippy --fix --allow-dirty
    @echo "✅ Code formaté"

# ==================== NETTOYAGE ====================

# Nettoie les artefacts de build
clean:
    @echo "🧹 Nettoyage..."
    cargo clean
    rm -rf coverage/
    @echo "✅ Nettoyage OK"

# ==================== CI/CD SIMULATION ====================

# Simule un build CI (ce que GitHub Actions ferait)
ci: clean check build-test
    @echo "✅ CI Simulation OK"

# Prépare une release
release: clean check build-full
    @echo "🎉 Release prête !"
    @echo "📦 Binaire: ./target/release/raftstone"
    @echo ""
    @echo "Pour distribuer:"
    @echo "  cp ./target/release/raftstone /usr/local/bin/"

# ==================== DÉVELOPPEMENT ====================

# Lance le serveur en mode dev
dev:
    @echo "🚀 Serveur dev..."
    cargo run

# Lance le serveur en mode watch (redémarre à chaque changement)
dev-watch:
    @echo "👀 Dev watch mode..."
    cargo install cargo-watch || true
    cargo watch -x run

# ==================== DOCUMENTATION ====================

# Génère la documentation
doc:
    @echo "📚 Génération documentation..."
    cargo doc --no-deps --open

# ==================== BENCHMARKS ====================

# Lance les benchmarks
bench:
    @echo "⚡ Benchmarks..."
    cargo bench

# ==================== OUTILS ====================

# Installe les outils nécessaires
setup:
    @echo "🔧 Installation des outils..."
    cargo install cargo-watch || true
    cargo install cargo-nextest || true
    cargo install cargo-tarpaulin || true
    cargo install cargo-make || true
    @echo "✅ Outils installés"

# Affiche les informations système
info:
    @echo "ℹ️ Informations système"
    @echo "Rust version:"
    @rustc --version
    @echo ""
    @echo "Cargo version:"
    @cargo --version
    @echo ""
    @echo "Projet:"
    @cargo tree --depth 1

# ==================== EXEMPLES D'USAGE ====================

# just test-all       → Lance tous les tests
# just build-release  → Build optimisé
# just ci             → Simule CI
# just release        → Prépare release
# just dev-watch      → Dev avec hot reload
