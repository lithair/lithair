# 🥒 Lithair BDD Testing with Cucumber + Gherkin

Ce dossier contient la suite complète de tests **Behavior-Driven Development (BDD)** pour Lithair, utilisant Cucumber et le langage Gherkin.

## 📁 Structure des Features

```
features/
├── core/                    # Fonctionnalités principales du framework
│   ├── performance.feature # Tests de performance ultra-haute
│   ├── security.feature    # Tests de sécurité enterprise
│   └── distribution.feature# Tests de distribution et consensus
├── integration/             # Tests d'intégration complets
│   └── web_server.feature  # Serveur web complet avec frontend
├── persistence/             # Persistance et event sourcing
│   └── event_sourcing.feature# Tests de persistance des événements
├── observability/           # Monitoring et métriques
│   └── monitoring.feature  # Tests d'observabilité
├── steps/                   # Implémentation des steps Gherkin
│   ├── performance_steps.rs
│   ├── security_steps.rs
│   └── mod.rs
├── world.rs                 # État partagé des tests
└── lib.rs                   # Module public des features
```

## 🚀 Comment utiliser

### Installation
```bash
task bdd:setup
```

### Exécuter tous les tests
```bash
task bdd:run
```

### Tests par catégorie
```bash
task bdd:performance    # Tests de performance
task bdd:security       # Tests de sécurité
task bdd:distribution   # Tests de distribution
task bdd:integration    # Tests d'intégration
task bdd:persistence    # Tests de persistance
task bdd:observability  # Tests d'observabilité
```

### CI/CD avec BDD
```bash
task ci:bdd    # CI complète avec tests BDD
task bdd:ci    # Mode CI (sortie JSON)
```

## 📋 Scénarios couverts

### 🚀 Performance Ultra-Haute
- Serveur HTTP avec performances maximales
- Benchmark JSON throughput
- Concurrence massive
- Évolution des performances sous charge

### 🛡️ Sécurité Enterprise
- Protection contre les attaques DDoS
- Contrôle d'accès par rôles (RBAC)
- Validation des tokens JWT
- Filtrage IP géographique
- Rate limiting par endpoint

### 🔄 Distribution et Consensus
- Élection du leader
- Réplication des données
- Partition réseau et split-brain
- Rejoindre un cluster existant
- Scalabilité horizontale

### 🌐 Serveur Web Complet
- Service des pages HTML
- API CRUD complète
- CORS pour frontend externe
- WebSockets temps réel
- Cache intelligent des assets

### 💾 Event Sourcing et Persistance
- Persistance des événements
- Reconstruction de l'état
- Snapshots optimisés
- Déduplication des événements
- Récupération après corruption

### 📊 Observabilité et Monitoring
- Health checks complets
- Métriques Prometheus
- Performance profiling
- Logging structuré
- Alertes automatiques

## 🔧 Architecture Technique

### World partagé
Les tests utilisent une structure `LithairWorld` qui maintient :
- L'état des serveurs (port, PID, running status)
- Les métriques de performance
- Les données de test (articles, utilisateurs, tokens)
- La dernière réponse HTTP
- Les erreurs rencontrées

### Steps réutilisables
Chaque catégorie de tests a ses steps :
- **Performance** : démarrage serveur, envoi requêtes, mesures
- **Sécurité** : authentification, autorisation, rate limiting
- **Distribution** : clustering, replication, consensus
- **Integration** : APIs CRUD, CORS, WebSockets

### Configuration dynamique
Les tests peuvent être configurés avec :
- Variables d'environnement (RUST_LOG, PORT, etc.)
- Fichiers de configuration externes
- Paramètres de ligne de commande

## 📈 Rapports et Résultats

### Sortie standard
```
🥒 Cucumber Results:
✅ 45 scenarios passed
❌ 2 scenarios failed
📊 95.7% success rate
⏱️  Total time: 3m 24s
```

### Rapport JSON (CI)
```bash
task bdd:ci
# Génère test-results/cucumber-results.json
```

### Intégration avec GitHub Actions
Les tests BDD s'intègrent parfaitement dans le pipeline CI :
```yaml
- name: Run BDD Tests
  run: task ci:bdd
```

## 🎯 Avantages du BDD pour Lithair

1. **Documentation vivante** : Les features servent de documentation technique
2. **Collaboration** : Langage commun entre développeurs, QA et product owners
3. **Traçabilité** : Chaque bug peut être lié à un scénario spécifique
4. **Régression** : Tests automatiques complets après chaque changement
5. **Vision client** : Focus sur le comportement utilisateur plutôt que l'implémentation

## 🔄 Migration depuis les Examples

Les examples traditionnels sont progressivement migrés :
- `scc2_server_demo/` → `performance.feature`
- `http_firewall_demo/` → `security.feature`
- `raft_replication_demo/` → `distribution.feature`
- `blog_server/` → `web_server.feature`

Cette approche permet de :
- Conserver la fonctionnalité existante
- Ajouter une couche de validation BDD
- Améliorer la couverture de tests
- Faciliter la maintenance

## 🚀 Prochaines étapes

1. **Compléter** les step definitions manquantes
2. **Ajouter** des scénarios de charge extrême
3. **Intégrer** avec les benchmarks existants
4. **Automatiser** la génération de rapports
5. **Étendre** aux tests de negative testing

---

**Lithair BDD** - Transformant la façon dont nous testons les systèmes distribués ultra-performants ! 🚀
