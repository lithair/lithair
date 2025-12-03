# 🥒 Tests Cucumber pour Lithair

Suite de tests BDD (Behavior-Driven Development) complète pour le framework Lithair.

## 🎯 Objectif

**Utiliser Cucumber comme pilier central** pour :
- ✅ Tester toutes les fonctionnalités (features + bugs)
- ✅ Documenter le comportement attendu (Gherkin lisible)
- ✅ Valider l'intégration complète (vrais tests, pas des stubs)
- ✅ Tracer les bugs découverts avec contexte technique

## 📁 Structure

```
cucumber-tests/
├── features/                   # Spécifications Gherkin (.feature)
│   ├── basic.feature          # Tests de base
│   ├── core/                  # Fonctionnalités core
│   ├── persistence/           # Persistance & event sourcing
│   ├── integration/           # Intégrations (sessions, web, models)
│   └── observability/         # Monitoring, logs, métriques
│
├── src/features/
│   ├── world.rs              # LithairWorld (état partagé + moteur réel)
│   └── steps/                # Implémentations des steps
│       ├── basic_steps.rs
│       ├── advanced_persistence_steps.rs
│       ├── distribution_steps.rs
│       ├── security_steps.rs
│       └── ...
│
├── tests/
│   └── cucumber_tests.rs     # Runner principal
│
├── TESTING_STACK.md          # 📊 Documentation technique complète
├── BUG_REPORTS.md            # 🐛 Historique des bugs découverts
└── README.md                 # 📖 Ce fichier
```

## 🚀 Quick Start

### Lancer tous les tests

```bash
cd cucumber-tests
cargo test --test cucumber_tests
```

### Lancer un feature spécifique

```bash
# Uniquement la persistance avancée
cargo test --test cucumber_tests -- features/persistence/advanced_persistence.feature

# Uniquement le basic
cargo test --test cucumber_tests -- features/basic.feature
```

### Activer les logs détaillés

```bash
export RUST_LOG=debug
export RS_OPT_PERSIST=1
cargo test --test cucumber_tests
```

## 📝 Workflow : Ajouter un nouveau test

### 1. Créer la feature Gherkin

`features/mon_module/nouvelle_feature.feature` :

```gherkin
# language: fr
# Stack: Lithair Core + MonModule v1.0
# Bugs connus: Aucun

Fonctionnalité: Ma Nouvelle Feature
  En tant que développeur
  Je veux tester MonModule
  Afin de garantir son bon fonctionnement

  Contexte:
    Soit un serveur Lithair avec MonModule activé

  @critical @mon_module
  Scénario: Cas nominal
    Quand j'effectue l'action X
    Alors le résultat doit être Y
    Et l'état doit être cohérent
```

### 2. Créer les steps

`src/features/steps/mon_module_steps.rs` :

```rust
use cucumber::{given, when, then};
use crate::features::world::LithairWorld;

/// Initialise MonModule pour les tests
/// 
/// # Stack Technique
/// - Utilise MonModule::new() avec config test
/// - Crée répertoire temporaire pour données
/// 
/// # Performances
/// - Temps: ~100ms
#[given(expr = "un serveur Lithair avec MonModule activé")]
async fn given_mon_module_enabled(world: &mut LithairWorld) {
    // Vraie initialisation, pas un stub !
    let temp_path = world.init_temp_storage().await
        .expect("Init storage failed");
    
    // TODO: Initialiser MonModule ici
    
    println!("✅ MonModule activé: {:?}", temp_path);
}

#[when(expr = "j'effectue l'action X")]
async fn when_action_x(world: &mut LithairWorld) {
    // VRAI TEST: Appeler MonModule
    // let result = world.mon_module.do_action_x().await?;
    
    println!("🔧 Action X effectuée");
}

#[then(expr = "le résultat doit être Y")]
async fn then_result_is_y(world: &mut LithairWorld) {
    // VRAIE ASSERTION
    // let actual = world.mon_module.get_result();
    // assert_eq!(actual, "Y", "Résultat incorrect");
    
    println!("✅ Résultat validé: Y");
}

#[then(expr = "l'état doit être cohérent")]
async fn then_state_consistent(world: &mut LithairWorld) {
    // VRAIE VÉRIFICATION
    let checksum = world.compute_memory_checksum().await;
    println!("✅ État cohérent (checksum: 0x{:08x})", checksum);
}
```

### 3. Enregistrer le module

`src/features/steps/mod.rs` :

```rust
pub mod mon_module_steps;
```

### 4. Lancer les tests

```bash
cargo test --test cucumber_tests
```

## 🐛 Documenter un bug découvert

### Quand un test échoue

1. **Identifier** le scénario qui échoue
2. **Reproduire** manuellement
3. **Documenter** dans `BUG_REPORTS.md` :

```markdown
## 🐛 Bug #XXX : Titre descriptif

**Status:** 🔴 CRITIQUE  
**Découvert par:** `feature.feature:42` - Nom du scénario  
**Date:** 2024-11-11  
**Reproductible:** ✅ Oui

### Symptôme
...

### Stack Technique Impliquée
...

### Cause Racine
\`\`\`rust
// Code buggé
\`\`\`

### Fix Appliqué
\`\`\`rust
// Code corrigé
\`\`\`
```

4. **Ajouter un test de régression** dans les steps
5. **Référencer** le bug dans la feature Gherkin :

```gherkin
Scénario: Test de régression Bug #XXX
  # BUG #XXX: Description
  # FIX: Commit hash
  Quand ...
  Alors ...
```

## 📊 Consulter la stack technique

### Documentation complète

Voir [`TESTING_STACK.md`](./TESTING_STACK.md) pour :
- Architecture des tests
- Composants Lithair testés
- Dépendances et versions
- Métriques de couverture
- Guide de debugging

### Historique des bugs

Voir [`BUG_REPORTS.md`](./BUG_REPORTS.md) pour :
- Tous les bugs découverts
- Contexte technique complet
- Fixes appliqués
- Tests de régression

## 🔍 Debugging

### Test spécifique qui échoue

```bash
# Voir le détail complet
RUST_LOG=trace cargo test --test cucumber_tests -- features/mon_feature.feature

# Garder les fichiers temporaires
export LITHAIR_KEEP_TEMP=1
cargo test --test cucumber_tests

# Inspecter les fichiers après
ls -la /tmp/lithair-test-*/
cat /tmp/lithair-test-*/events.raftlog | jq .
```

### Ajouter un step de debug

```rust
#[then(expr = "je debug l'état complet")]
async fn debug_full_state(world: &mut LithairWorld) {
    let articles = world.get_articles().await;
    let checksum = world.compute_memory_checksum().await;
    
    eprintln!("🐛 DEBUG STATE:");
    eprintln!("  Articles count: {}", articles.len());
    eprintln!("  Articles: {:#?}", articles);
    eprintln!("  Checksum: 0x{:08x}", checksum);
    
    // Dump files
    if let Some(dir) = world.temp_dir.lock().await.as_ref() {
        eprintln!("  Temp dir: {:?}", dir.path());
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let entry = entry.unwrap();
            eprintln!("    - {:?} ({} bytes)", 
                entry.file_name(), 
                entry.metadata().unwrap().len());
        }
    }
}
```

## 📈 Métriques & Rapports

### Générer un rapport HTML

```bash
# TODO: À implémenter avec cucumber-html-formatter
cargo test --test cucumber_tests -- --format json > report.json
```

### Statistiques de couverture

Voir [`TESTING_STACK.md`](./TESTING_STACK.md#métriques-de-test) pour :
- Couverture par composant
- Temps d'exécution
- Taux de réussite

## 🎯 Bonnes Pratiques

### ✅ DO

- **Écrire des vrais tests** avec assertions réelles
- **Documenter la stack** technique dans les commentaires
- **Tracer les bugs** dans BUG_REPORTS.md
- **Ajouter tests de régression** pour chaque bug
- **Utiliser TempDir** pour isolation des tests
- **Calculer checksums** pour vérifier intégrité

### ❌ DON'T

- **Pas de `println!()` seuls** sans assertions
- **Pas de stubs vides** (toujours tester vraiment)
- **Pas de fichiers hardcodés** (utiliser TempDir)
- **Pas de tests dépendants** (isolation complète)
- **Pas de secrets** dans les tests

## 🤝 Contribuer

1. Créer une branche `feature/test-mon-module`
2. Ajouter les `.feature` + steps
3. Documenter dans TESTING_STACK.md si nouveau composant
4. Valider que tous les tests passent
5. Créer une PR avec description des tests ajoutés

## 📚 Ressources

- **Cucumber Book:** <https://cucumber.io/docs/guides/>
- **Lithair Docs:** `../docs/`
- **Rust async:** <https://tokio.rs/>
- **Event Sourcing:** Martin Fowler

---

**Mainteneur:** Lithair Team  
**Dernière mise à jour:** 2024-11-11  
**Questions ?** Ouvrir une issue GitHub
