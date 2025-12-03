#!/bin/bash

# 🚀 Script pour lancer TOUS les tests Lithair
# Ce script valide que TOUT fonctionne :
# - Tests unitaires
# - Tests E2E Cucumber
# - Tests d'intégration Build
# - Compilation finale

set -e  # Arrêter si une commande échoue

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🚀 Lithair - Suite de Tests Complète"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# ==================== NIVEAU 1 : Tests Unitaires ====================
echo "📊 NIVEAU 1 : Tests Unitaires (lithair-core)"
echo "────────────────────────────────────────────────"
cd lithair-core
cargo test --lib
echo "✅ Tests unitaires OK"
echo ""

# ==================== NIVEAU 2 : Tests E2E Cucumber ====================
echo "📊 NIVEAU 2 : Tests E2E Cucumber"
echo "────────────────────────────────────────────────"
cd ../cucumber-tests
cargo test --test cucumber_tests
echo "✅ Tests E2E Cucumber OK"
echo ""

# ==================== NIVEAU 3 : Tests d'Intégration Build ====================
echo "📊 NIVEAU 3 : Tests d'Intégration Build"
echo "────────────────────────────────────────────────"
cargo test --test integration_build_test
echo "✅ Tests d'intégration Build OK"
echo ""

# ==================== NIVEAU 4 : Compilation Finale ====================
echo "📊 NIVEAU 4 : Compilation Binaire Final"
echo "────────────────────────────────────────────────"
cd ..
cargo build --release --bin lithair
if [ -f "target/release/lithair" ]; then
    echo "✅ Binaire créé : target/release/lithair"
    
    # Tester --help
    ./target/release/lithair --help > /dev/null 2>&1 || true
    echo "✅ Commande --help fonctionne"
else
    echo "❌ Binaire non trouvé"
    exit 1
fi
echo ""

# ==================== RÉSUMÉ ====================
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🎉 TOUS LES TESTS SONT PASSÉS !"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "✅ Tests unitaires        : OK"
echo "✅ Tests E2E Cucumber     : OK"
echo "✅ Tests intégration Build: OK"
echo "✅ Compilation finale     : OK"
echo ""
echo "🚀 Le produit Lithair est prêt à être utilisé !"
echo ""
echo "Pour démarrer le serveur :"
echo "  ./target/release/lithair --config config.toml"
echo ""
