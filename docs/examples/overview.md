# Vue d'Ensemble des Exemples Lithair

Cette section présente les exemples réellement maintenus dans le workspace
`examples/`.

## 🎯 Catalogue canonique

Le point d'entrée principal est le catalogue racine :

- [examples/README.md](../../examples/README.md)

Ce fichier décrit les packages de démonstration qui font partie du workspace.

## 🚀 Parcours recommandé

### Démarrage

- **[01-hello-world](../../examples/01-hello-world/)** - builder
  `LithairServer`, configuration minimale, démarrage rapide
- **[03-rest-api](../../examples/03-rest-api/)** - CRUD déclaratif le plus simple
- **[04-blog](../../examples/04-blog/)** - frontend, sessions, RBAC

### Authentification et sécurité

- **[06-auth-sessions](../../examples/06-auth-sessions/)** - sessions, login,
  tokens et permissions
- **[07-auth-rbac-mfa](../../examples/07-auth-rbac-mfa/)** - RBAC avancé, MFA,
  patterns SSO
- **[advanced/http-firewall](../../examples/advanced/http-firewall/)** - scripts
  de validation firewall
- **[advanced/http-hardening](../../examples/advanced/http-hardening/)** -
  scripts de validation hardening HTTP

### Données et évolution de schéma

- **[05-ecommerce](../../examples/05-ecommerce/)** - relations, modèles
  multiples, workflow métier
- **[08-schema-migration](../../examples/08-schema-migration/)** - évolution de
  schéma et workflows de migration

### Distribué et validation

- **[09-replication](../../examples/09-replication/)** - cluster multi-nœuds,
  scripts de bench et loadgen
- **[10-blog-distributed](../../examples/10-blog-distributed/)** - blog
  distribué
- **[advanced/consistency-test](../../examples/advanced/consistency-test/)** -
  outil de validation de cohérence
- **[advanced/stress-test](../../examples/advanced/stress-test/)** - outil de
  stress et diagnostic

## 🏗️ Deux familles à distinguer

### Exemples d'apprentissage

Ce sont les packages numérotés `examples/01-*` à `examples/15-*`.

- Ils servent de **références produit**.
- Ils doivent rester **lisibles** et **documentés**.
- Ils sont faits pour être cités dans la doc publique et dans la CI.

### Outils de validation avancés

Les dossiers `examples/advanced/*` servent surtout à valider un comportement ou
à reproduire des scénarios de charge.

- Ils restent utiles dans le repo.
- Ils ne doivent pas être confondus avec les exemples d'introduction.
- Certains sont plus proches d'outils de test que de tutoriels.

## 🔧 Structure attendue

Dans l'organisation actuelle, un exemple racine est en général un package cargo
du workspace :

```text
examples/04-blog/
├── Cargo.toml
├── README.md
├── src/
├── frontend/
└── run_blog_tests.sh
```

Les outils avancés peuvent avoir une structure un peu différente, mais ils
restent rangés sous `examples/advanced/` pour éviter de les mélanger avec les
exemples d'apprentissage.

## ▶️ Commandes utiles

```bash
task examples:list
task examples:test
task examples:hello-world
task examples:rbac-session
task examples:blog:test
task examples:replication:firewall
task examples:replication:hardening
```

## 📚 Ressources complémentaires

- **[Comparaison Data-First](data-first-comparison.md)** - Lithair vs approche
  traditionnelle
- **[Rapport d'Audit](audit-report.md)** - analyse historique du dossier
  examples
- **[Guide de Performance](../guides/performance.md)** - optimisations et
  benchmarks

---

**Conseil :** commencez par `01-hello-world`, puis `03-rest-api` ou
`06-auth-sessions`, avant d'aller vers `09-replication` et les dossiers
`advanced/`.
