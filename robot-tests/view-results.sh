#!/bin/bash
# Script pour voir les résultats des tests Robot

echo "📊 Résultats des tests Robot Framework"
echo ""
echo "Fichiers générés:"
echo "  - log.html     : Logs détaillés (RECOMMANDÉ)"
echo "  - report.html  : Rapport de synthèse"
echo "  - output.xml   : Format XML"
echo ""

# Vérifier si les fichiers existent
if [ ! -f "robot-tests/results/log.html" ]; then
    echo "❌ Aucun résultat trouvé. Lance d'abord les tests:"
    echo "   robot robot-tests/test_simple_demo.robot"
    exit 1
fi

# Afficher un résumé dans le terminal
echo "📈 Résumé rapide:"
echo ""
tail -15 robot-tests/results/log.html | grep -o "test[s]*,[^<]*" | head -1
echo ""

# Proposer d'ouvrir le log
echo "Veux-tu ouvrir le log détaillé ? (y/n)"
read -r response

if [[ "$response" == "y" ]]; then
    if command -v xdg-open &> /dev/null; then
        xdg-open robot-tests/results/log.html
    elif command -v firefox &> /dev/null; then
        firefox robot-tests/results/log.html
    else
        echo "Ouvre manuellement : robot-tests/results/log.html"
    fi
fi
