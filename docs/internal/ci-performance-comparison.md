# CI Performance Comparison

## Approches testées

| Approche | Configuration | Temps estimé | Avantages | Inconvénients |
|----------|---------------|--------------|-----------|---------------|
| **Original** | `ubuntu-latest` + setup Rust | ~6-8min | Flexible, standard | Lent (install Rust) |
| **Docker Standard** | `rust:1-bookworm` (Debian stable) | ~4-5min | Rust pré-installé, toolchain complète | Image plus lourde |
| **Docker Alpine** | `rust:1-alpine` + musl-dev + gcc | ~3-4min | Image légère, Rust pré-installé | Libs C manuelles |

## Gains attendus avec Docker

### ✅ **Temps économisé :**
- **Rust Setup** : ~90-120s → 0s (pré-installé)
- **Clippy install** : ~30s → 0s (inclus)
- **Cache miss impact** : Réduit (Rust tools déjà là)

### ✅ **Optimisations supplémentaires :**
- **Timeout réduit** : 40min → 30min (standard) / 20min (alpine)
- **APT quiet mode** : `-qq` pour logs propres
- **Cache key optimisé** : Inclut version Rust

### 📊 **Estimation des gains :**
```
Avant (ubuntu + setup):     6-8 minutes
Après (rust:1-bookworm):   4-5 minutes (-25-35%)
Après (rust:1-alpine):     3-4 minutes (-40-50%)
```

## Recommandation

**Utiliser `rust:1-alpine`** pour :
- CI rapide quotidienne (PR checks)
- Développement itératif
- Tests fréquents

**Garder `rust:1` standard** pour :
- Release builds
- Tests complets avec smoke tests
- Compatibility checks

## Configuration choisie

- **ci.yml** : `rust:1-bookworm` (complet, stable, Debian)
- **ci-fast.yml** : `rust:1-alpine` + musl-dev + gcc (rapide, PR)

### Pourquoi `rust:1` plutôt qu'une version fixe ?

✅ **Avantages :**
- **Sécurité automatique** : Récupère les patches de sécurité Rust
- **Compatibilité future** : Code testé avec les dernières versions
- **Simplicité** : Pas besoin de maintenir les versions manuellement
- **Performance** : Optimisations Rust les plus récentes

⚠️ **Compromis :**
- **Stabilité** : Risque de breaking changes (rare en stable)
- **Reproductibilité** : Builds différents dans le temps

💡 **Bonne pratique :** Utiliser `rust:1-bookworm` (stable) et `rust:1-alpine` (rapide).

## ⚠️ Problèmes Alpine et Solutions

### **Erreur commune Alpine :**
```
cannot find crti.o: No such file or directory
error: linking with `cc` failed
```

### **Cause :**
- Alpine utilise **musl** (C library minimaliste)
- Manque les **C development tools** par défaut
- Rust a besoin du **C linker** pour les proc macros et shared libraries

### **Solution :**
```dockerfile
# Dans ci-fast.yml
apk add --no-cache musl-dev gcc
#                  ^^^^^^^^ ^^^
#                  C headers  C toolchain
```

### **Pourquoi bookworm est plus simple :**
- **Debian** = `glibc` + **build-essential** complet
- **Alpine** = `musl` + outils manuels
- Bookworm = "ça marche" / Alpine = "plus rapide mais configuration"
