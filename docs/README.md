# Documentation Lithair

Bienvenue dans la documentation complète de Lithair, le framework Rust disruptif qui unifie le développement backend par la pensée Data-First.

## 📚 Table des Matières

### 🏗️ Architecture

- **[Vue d'Ensemble](architecture/overview.md)** - Architecture générale de Lithair avec diagrammes
- **[Flux de Données](architecture/data-flow.md)** - Comment les données circulent dans le système
- **[Conception Système](architecture/system-design.md)** - Principes de conception et patterns architecturaux

### 🔧 Modules

- **[HTTP Firewall](modules/firewall/README.md)** - Système de sécurité HTTP avec filtrage IP et rate limiting
- **[Stockage](modules/storage/README.md)** - Système de persistance et event sourcing
- **[Consensus Raft](modules/consensus/README.md)** - Réplication distribuée et consensus
- **[Modèles Déclaratifs](modules/declarative-models/README.md)** - Système de déclaration de modèles avec attributs
- **[Serveur HTTP](modules/http-server/README.md)** - Serveur HTTP Hyper avec génération automatique d'API

### ✨ Fonctionnalités

- **[Aperçu des Fonctionnalités](features/README.md)**
- **Frontend — Vue d'ensemble**: [features/frontend/overview.md](features/frontend/overview.md)
- **Frontend — Modes de service**: [features/frontend/modes.md](features/frontend/modes.md)
- **Backend — Vue d'ensemble**: [features/backend/overview.md](features/backend/overview.md)
- **Security — Vue d'ensemble**: [features/security/overview.md](features/security/overview.md)
- **Persistence — Vue d'ensemble**: [features/persistence/overview.md](features/persistence/overview.md)
- **State Engine — Vue d'ensemble**: [features/state-engine/overview.md](features/state-engine/overview.md)
- **Declarative — Vue d'ensemble**: [features/declarative/overview.md](features/declarative/overview.md)
- **Clustering — Vue d'ensemble**: [features/clustering/overview.md](features/clustering/overview.md)
- **Event Sourcing — Implémentation & Tests**: [event-sourcing/README.md](event-sourcing/README.md) · [Tests](event-sourcing/testing.md)

### 📖 Guides

- **[Démarrage Rapide](guides/getting-started.md)** - Premier pas avec Lithair
- **[Guide Développeur](guides/developer-guide.md)** - Guide complet pour les développeurs
- **[Philosophie Data-First](guides/data-first-philosophy.md)** - Comprendre l'approche Data-First
- **[Tutoriel E-commerce](guides/ecommerce-tutorial.md)** - Créer une application e-commerce complète
- **[Intégration CRUD](guides/crud-integration.md)** - Intégrer les opérations CRUD
- **[Performance](guides/performance.md)** - Optimisation et benchmarks
- **[HTTP Stateless Performance Endpoints](guides/http_performance_endpoints.md)** - Points de terminaison de benchmarking et génération de charge
- **[HTTP Hardening, Gzip & Firewall](guides/http_hardening_gzip_firewall.md)** - Contrôles de production et protections

### 📋 Référence

- **[Attributs Déclaratifs](reference/declarative-attributes.md)** - Référence complète des attributs
- **[API Reference](reference/api-reference.md)** - Documentation de l'API
- **[Comparaison SQL vs Lithair](reference/sql-vs-lithair.md)** - Comparaison détaillée
- **[Configuration Reference](configuration-reference.md)** - Complete configuration variables reference
- **[Configuration Matrix](configuration-matrix.md)** - Quick reference matrix for all config options
- **[Variables d'Environnement](reference/env-vars.md)** - RUST_LOG, RS_ADMIN_PATH, RS_DEV_RELOAD_TOKEN

### 🎯 Exemples

- **[Aperçu des Exemples](examples/overview.md)** - Vue d'ensemble de tous les exemples
- **[Comparaison Data-First](examples/data-first-comparison.md)** - Comparaison avec l'approche traditionnelle
- **[Rapport d'Audit](examples/audit-report.md)** - Audit des exemples et bonnes pratiques

### 📊 Diagrammes

- **[Diagrammes Mermaid](diagrams/README.md)** - Collection de tous les diagrammes du système

## 🚀 Démarrage Rapide

```bash
# Cloner le projet
git clone https://github.com/your-org/lithair
cd lithair

# Lancer l'exemple de référence (benchmark distribué)
cd examples/raft_replication_demo
cargo run --bin simplified_consensus_demo
```

## 🎯 Concepts Clés

### Philosophie Data-First

Au lieu de séparer les couches business logic, base de données et API, Lithair vous permet de **modéliser vos données une seule fois** et génère tout le reste automatiquement.

```rust
#[derive(DeclarativeModel)]
pub struct User {
    #[db(primary_key)]                    // Contraintes base de données
    #[lifecycle(immutable)]               // Règles métier
    #[http(expose)]                       // Génération API
    #[persistence(replicate)]             // Distribution
    #[permission(read = "UserRead")]      // Sécurité
    pub id: Uuid,
}
```

**Résultat :** 1 définition de struct → Backend complet avec API, base de données, sécurité, audit, réplication !

### Révolution vs Traditionnel

| Tâche                           | Approche Traditionnelle                     | Lithair Data-First                              |
| ------------------------------- | ------------------------------------------- | ------------------------------------------------- |
| **Ajouter un champ avec audit** | 50+ lignes (migration, service, controller) | **1 ligne :** `#[lifecycle(audited)]`             |
| **Ajouter validation API**      | DTO + service + tests                       | **1 attribut :** `#[http(validate = "email")]`    |
| **Ajouter permissions**         | Middleware + logique service                | **1 attribut :** `#[permission(write = "Admin")]` |
| **Ajouter réplication**         | Setup distribué complexe                    | **1 attribut :** `#[persistence(replicate)]`      |

## 🏆 Résultats Prouvés

Notre exemple de référence `simplified_consensus_demo.rs` démontre la puissance complète de Lithair :

- **2 000 opérations CRUD aléatoires** sur un cluster distribué de 3 nœuds
- **250,91 ops/sec de débit HTTP** via des endpoints REST auto-générés
- **Consistance parfaite des données** : 1 270 produits identiques sur tous les nœuds
- **Zéro traitement manuel** : Tout auto-généré à partir des attributs DeclarativeModel

## 🔗 Liens Utiles

- [Philosophie du Projet](guides/data-first-philosophy.md)
- [Guide de Performance](guides/performance.md)
- [Roadmap](reference/roadmap.md)
- [Exemples Complets](examples/overview.md)

## 👥 Contribution

Consultez le [Guide du Développeur](guides/developer-guide.md) pour contribuer au projet.

**Auteur :** Yoan Roblet (Arcker)
**Version :** 2024.3
**Licence :** MIT
