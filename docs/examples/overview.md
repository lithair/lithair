# Vue d'Ensemble des Exemples Lithair

Cette section présente tous les exemples disponibles dans Lithair, organisés par complexité et cas d'usage.

## 🎯 Exemple de Référence

### [Simplified Consensus Demo](../../examples/raft_replication_demo/)
**Le benchmark de référence qui prouve que Lithair fonctionne !**

```bash
cd examples/raft_replication_demo
cargo run --bin simplified_consensus_demo
```

**Résultats prouvés :**
- ✅ 2 000 opérations CRUD aléatoires distribuées
- ✅ 250,91 ops/sec de débit HTTP
- ✅ Consistance parfaite : 1 270 produits identiques sur 3 nœuds
- ✅ Zéro traitement manuel : tout auto-généré

## 🚀 Exemples par Complexité

### Niveau Débutant
- **[HTTP Firewall Demo](../../examples/http_firewall_demo/)** - Démonstration du système de sécurité
- **[HTTP Hardening (stateless perf)](../../examples/raft_replication_demo/bench_http_server_stateless.sh)** - Endpoints `/perf/*`

### Niveau Intermédiaire
- **[Consensus Demo](../../examples/raft_replication_demo/)** - Cluster 3 nœuds + réplication

### Niveau Avancé
- **[Schema Evolution](../../examples/schema_evolution/)** - Migration de schémas avancées

## 🏗️ Exemples par Cas d'Usage

### 🛍️ E-commerce
- **SCC2 Ecommerce Demo** - Architecture complète avec gestion des stocks
- **Relations Database** - Modélisation produits/commandes/clients

### 📝 Content Management
- **Schema Evolution** - Évolution de contenu dans le temps

### 📊 Monitoring & Analytics
- **Dashboard Performance** - Visualisation de métriques temps réel
- **Consensus Demo** - Métriques distribuées et réplication

### 🔒 Sécurité
- **HTTP Firewall Demo** - Protection contre les attaques
- **Tous les exemples** - RBAC intégré par défaut

## 🔧 Structure Type d'un Exemple

Chaque exemple Lithair suit cette structure :

```
exemple_name/
├── src/
│   ├── main.rs              # Point d'entrée
│   ├── models.rs            # Modèles déclaratifs
│   └── config.rs            # Configuration
├── Cargo.toml              # Dépendances
├── README.md               # Documentation spécifique
├── run_demo.sh            # Script de démonstration
└── data/                  # Données persistées
```

## 🎨 Modèles Déclaratifs par Exemple

> Les anciens exemples full‑stack (Blog NextJS) ont été retirés. Les démos maintenues se concentrent sur le serveur HTTP, la réplication et le hardening.

### Modèle exemple (simplifié)
```rust
#[derive(DeclarativeModel)]
pub struct Product {
    #[db(primary_key, indexed)]
    #[http(expose)]
    pub id: Uuid,

    #[http(expose, validate = "non_empty")]
    pub name: String,

    #[http(expose, validate = "min_value(0.01)")]
    #[lifecycle(audited)]
    pub price: f64,
}
```

## 📈 Métriques de Performance

| Exemple | Débit (ops/sec) | Latence (ms) | Nœuds | Données |
|---------|----------------|--------------|-------|---------|
| **Consensus Demo** | **250.91** | **~4ms** | **3** | **1,270 produits** |
| Blog NextJS | 180.5 | ~5ms | 1 | 500 articles |
| E-commerce SCC2 | 320.2 | ~3ms | 1 | 1,000 produits |
| Dashboard Performance | 450.8 | ~2ms | 1 | 10,000 métriques |

## 🚀 Lancer Tous les Exemples

```bash
# Script pour valider les démos principales
bash examples/raft_replication_demo/bench_http_server_stateless.sh
./examples/raft_replication_demo/bench_1000_crud_parallel.sh 10000
bash examples/http_firewall_demo/run_declarative_demo.sh
```

## 📚 Ressources Complémentaires

- **[Comparaison Data-First](data-first-comparison.md)** - Lithair vs approche traditionnelle
- **[Rapport d'Audit](audit-report.md)** - Analyse de qualité des exemples
- **[Guide de Performance](../guides/performance.md)** - Optimisations et benchmarks

## 🎯 Prochains Exemples

- **GraphQL API** - API GraphQL auto-générée
- **Microservices** - Architecture distribuée complète
- **IoT Integration** - Ingestion de données IoT haute fréquence
- **ML Pipeline** - Pipeline de machine learning intégré

---

**💡 Conseil :** Commencez par l'exemple **Simplified Consensus Demo** pour voir la puissance complète de Lithair, puis explorez les autres selon votre cas d'usage.