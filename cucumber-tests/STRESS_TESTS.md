# 🚀 Tests de Stress Lithair - 1 Million d'Articles

## 📊 Vue d'ensemble

Suite de tests Cucumber pour valider la **performance**, la **durabilité** et la **cohérence** de Lithair à grande échelle.

## 🎯 Scénarios de test

### 1️⃣ **STRESS TEST ULTIME - 1 MILLION d'articles**

**Fichier** : `features/performance/stress_1m_test.feature`

**Opérations :**

- ✅ **1,000,000** créations (CREATE)
- ✅ **200,000** modifications (UPDATE) - 20%
- ✅ **100,000** suppressions (DELETE) - 10%
- ✅ **État final** : 900,000 articles actifs

**Vérifications :**

- 📝 1,300,000 événements persistés (1M + 200K + 100K)
- 🔍 Ordre chronologique des événements
- 🛡️ Cohérence mémoire/disque (SCC2 vs FileStorage)
- ✅ Checksums validés
- 📊 Métriques de performance

**Lancement :**

```bash
cd cucumber-tests
cargo test --test database_perf_test --release
```

---

### 2️⃣ **Performance Maximale - 500K articles**

**Mode** : `DurabilityMode::Performance`

**Objectif :**

- 🎯 Throughput > 20,000 articles/sec
- ⏱️ Temps total < 30 secondes
- 🚀 Throughput suppression > 15,000 articles/sec

**Caractéristiques :**

- ⚡ Performance maximale
- ⚠️ Risque perte max 10ms
- 📊 Mesure des limites théoriques

---

### 3️⃣ **Cohérence Garantie - 100K articles**

**Mode** : `DurabilityMode::MaxDurability` (DEFAULT)

**Opérations :**

- 100,000 CREATE
- 50,000 UPDATE
- 25,000 DELETE
- État final : 75,000 articles

**Garanties :**

- 🛡️ **ZÉRO perte de données**
- ✅ fsync après chaque batch
- 🔍 Cohérence mémoire/disque validée
- 📝 Tous événements persistés

---

### 4️⃣ **Résilience - 10K opérations aléatoires**

**Distribution** :

- 50% CREATE
- 30% UPDATE (si articles existants)
- 20% DELETE (si articles existants)

**Validation :**

- ✅ Tous événements persistés
- ✅ Cohérence mémoire/disque
- ✅ Pas d'erreurs de concurrence

---

## 📈 Performance attendue

### Architecture Full Async + SCC2 + MaxDurability

| Opération       | Throughput   | Latence P50 | Latence P99 |
| --------------- | ------------ | ----------- | ----------- |
| **CREATE**      | 10-30K/sec   | 5-10ms      | 20-50ms     |
| **READ** (SCC2) | 40M+ ops/sec | < 1µs       | < 10µs      |
| **UPDATE**      | 5-15K/sec    | 10-20ms     | 50-100ms    |
| **DELETE**      | 5-15K/sec    | 10-20ms     | 50-100ms    |

**Note** : Avec `DurabilityMode::Performance`, throughput 3-5x plus élevé mais risque perte données.

---

## 🛡️ Modes de Durabilité

### MaxDurability (DEFAULT - Production)

```rust
// Par défaut dans les tests
let writer = AsyncWriter::new(storage, 1000);
```

**Garanties :**

- ✅ ZÉRO perte de données
- ✅ fsync après chaque batch
- ✅ Conforme PostgreSQL/MySQL

**Performance :**

- 10,000-30,000 writes/sec (selon disque)

### Performance (Benchmarks uniquement)

```gherkin
Given le mode de durabilité est "Performance"
```

**Caractéristiques :**

- ⚡ 30,000-100,000 writes/sec
- ⚠️ Perte max 10ms si crash

**⚠️ JAMAIS en production !**

---

## 🧪 Vérifications d'intégrité

### 1. **Persistence complète**

```gherkin
Then le fichier events.raftlog doit exister
And le fichier events.raftlog doit contenir exactement 1000000 événements "ArticleCreated"
```

### 2. **Cohérence mémoire/disque**

```gherkin
Then le nombre d'articles en mémoire doit égaler le nombre sur disque
```

Vérifie que **SCC2 (RAM)** et **FileStorage (disque)** sont synchronisés.

### 3. **Ordre chronologique**

```gherkin
And tous les événements doivent être dans l'ordre chronologique
```

Garantit l'intégrité de l'event sourcing.

### 4. **Checksums**

```gherkin
And tous les checksums doivent correspondre
```

Détection de corruption de données.

---

## 📊 Métriques collectées

### Statistiques finales

```
╔══════════════════════════════════════╗
║   📊 STATISTIQUES FINALES           ║
╠══════════════════════════════════════╣
║ Total requêtes:          1,300,000   ║
║ Durée totale:                 65.32s ║
║ Throughput:              19,902/sec  ║
║ Erreurs:                         0   ║
╚══════════════════════════════════════╝
```

### Par opération

- **Throughput création** : ops/sec
- **Throughput modification** : ops/sec
- **Throughput suppression** : ops/sec

---

## 🚀 Lancer les tests

### Test complet 1M

```bash
cd cucumber-tests
cargo test --test database_perf_test --release
```

### Test spécifique

```bash
# Uniquement test durabilité
cargo test --release -- "Mode MaxDurability"

# Uniquement test performance
cargo test --release -- "Performance maximale"
```

### Avec logs détaillés

```bash
RUST_LOG=debug cargo test --test database_perf_test --release
```

---

## 🎯 Résultats attendus

### ✅ Succès

- Tous les événements persistés (100%)
- Cohérence mémoire/disque validée
- Checksums corrects
- Throughput conforme aux attentes

### ⚠️ Avertissements possibles

- Timeouts réseau sous forte charge
- Latence accrue avec MaxDurability (normal)
- Ralentissements avec HDD classique

### ❌ Échecs

- Perte d'événements → BUG CRITIQUE
- Incohérence mémoire/disque → BUG CRITIQUE
- Checksum invalide → CORRUPTION DONNÉES

---

## 🔧 Configuration

### Batch size AsyncWriter

```rust
const BATCH_SIZE: usize = 1000;
```

- Plus petit → Latence réduite, moins de throughput
- Plus grand → Throughput élevé, latence accrue

### Flush interval (mode Performance)

```rust
const FLUSH_INTERVAL_MS: u64 = 10;
```

- Plus court → Moins de perte potentielle
- Plus long → Meilleur throughput

---

## 📝 Notes

### SSD vs HDD

- **SSD NVMe** : ~10,000 fsync/sec → Excellent avec MaxDurability
- **SSD SATA** : ~5,000 fsync/sec → Bon avec MaxDurability
- **HDD 7200rpm** : ~100-500 fsync/sec → Lent avec MaxDurability

### Recommandations Production

1. ✅ **Toujours** `DurabilityMode::MaxDurability`
2. ✅ Utiliser un **SSD** pour les événements
3. ✅ Batch size **1000** (équilibre optimal)
4. ✅ Monitoring des **métriques de persistence**

---

## 🎯 Prochaines étapes

- [ ] WAL Mode (Write-Ahead Log)
- [ ] Compression des événements
- [ ] Tests cluster distribué (multi-nodes)
- [ ] Benchmarks vs PostgreSQL/MongoDB
- [ ] Tests de récupération après crash

---

**Lithair - Event-Sourced Database with Guaranteed Durability** 🛡️🚀
