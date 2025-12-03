# 🛡️ Lithair Durability Modes

## TL;DR

**Par défaut, Lithair garantit ZÉRO perte de données** avec `DurabilityMode::MaxDurability`.

Comme PostgreSQL, MySQL et MongoDB, **la durabilité est prioritaire sur la performance** pour une base de données sérieuse.

## 📊 Modes disponibles

### 🛡️ `MaxDurability` (DEFAULT - Recommandé Production)

**Configuration :**

```rust
use lithair_core::engine::{AsyncWriter, DurabilityMode, FileStorage};

let storage = FileStorage::new("/path/to/data")?;
let writer = AsyncWriter::new(storage, 1000); // Mode par défaut = MaxDurability
```

**Garanties :**

- ✅ **Aucune perte de données**, même en cas de crash brutal (SIGKILL, panne serveur)
- ✅ `fsync()` après chaque batch d'événements
- ✅ Conforme standards PostgreSQL/MySQL/MongoDB
- ✅ Adapté : Production, Event Sourcing, données critiques

**Performance :**

- 📊 **1,000 - 10,000 writes/sec** (selon disque)
- ⚙️ SSD moderne : ~10,000 writes/sec
- ⚙️ HDD classique : ~100-1,000 writes/sec

**Quand l'utiliser :**

- 🏢 **Production** : Toujours
- 💰 **Données financières** : Obligatoire
- 📝 **Event sourcing** : Essentiel
- 🔒 **Audit trail** : Requis

---

### ⚡ `Performance` (Benchmarks uniquement)

**Configuration :**

```rust
use lithair_core::engine::{AsyncWriter, DurabilityMode, FileStorage};

let storage = FileStorage::new("/path/to/data")?;

// ⚠️ ATTENTION : Risque de perte de données !
let writer = AsyncWriter::with_durability(
    storage,
    1000,
    DurabilityMode::Performance
);
```

**Caractéristiques :**

- ⚡ **30,000 - 100,000 writes/sec** (batch + buffer)
- ⚠️ **RISQUE** : Perte max 10ms de données en cas de crash
- 📊 Flush périodique (toutes les 10ms) au lieu de fsync immédiat

**Quand l'utiliser :**

- 🧪 **Benchmarks** : Mesurer performance max théorique
- 🚀 **Prototypes** : Développement rapide
- 📊 **Données non-critiques** : Logs, métriques temporaires
- ❌ **JAMAIS en production** avec données critiques

**⚠️ AVERTISSEMENT :**

```
En cas de crash brutal pendant l'écriture, vous pouvez perdre
jusqu'à 10ms d'événements (tous ceux en buffer non-flushés).

Pour une base de données event-sourced, CECI EST INACCEPTABLE
en production.
```

---

## 🔍 Comparaison avec autres DB

### PostgreSQL

```sql
-- Par défaut : durabilité garantie
synchronous_commit = on

-- Performance (non recommandé production)
synchronous_commit = off  -- Risque perte données
```

### MongoDB

```js
// Par défaut : durabilité garantie
writeConcern: { w: "majority", j: true }

// Performance (non recommandé production)
writeConcern: { w: 1, j: false }  // Risque perte données
```

### MySQL InnoDB

```
-- Par défaut : durabilité garantie
innodb_flush_log_at_trx_commit = 1

-- Performance (non recommandé production)
innodb_flush_log_at_trx_commit = 2  -- Risque perte données
```

### Lithair

```rust
// Par défaut : durabilité garantie ✅
AsyncWriter::new(storage, batch_size)

// Performance (non recommandé production) ⚠️
AsyncWriter::with_durability(storage, batch_size, DurabilityMode::Performance)
```

---

## 🎯 Recommandations

### ✅ Bonnes pratiques

1. **Production → TOUJOURS `MaxDurability`**

   ```rust
   let writer = AsyncWriter::new(storage, 1000);  // Mode par défaut
   ```

2. **SSD pour performance**

   - Avec SSD NVMe : ~10,000 writes/sec même avec fsync
   - Avec HDD : ~100-1,000 writes/sec avec fsync

3. **Batch size optimal**
   - 1,000 événements = bon équilibre latence/throughput
   - 10,000 événements = throughput max mais latence plus haute

### ❌ Anti-patterns

```rust
// ❌ JAMAIS en production avec données critiques !
let writer = AsyncWriter::with_durability(
    storage,
    1000,
    DurabilityMode::Performance  // Risque perte données
);

// ✅ À la place, utilisez le mode par défaut
let writer = AsyncWriter::new(storage, 1000);
```

---

## 📈 Benchmarks

### Test environnement

- CPU : AMD Ryzen 9 / Intel i9
- RAM : 32GB DDR4
- Disque : NVMe SSD

### Résultats

| Mode              | Throughput        | Latence P50 | Latence P99 | Perte données   |
| ----------------- | ----------------- | ----------- | ----------- | --------------- |
| **MaxDurability** | 10,000 writes/sec | 5ms         | 20ms        | ✅ **Aucune**   |
| Performance       | 30,000 writes/sec | 1ms         | 5ms         | ⚠️ **Max 10ms** |

### Avec HDD classique

| Mode              | Throughput        | Latence P50 | Latence P99 | Perte données   |
| ----------------- | ----------------- | ----------- | ----------- | --------------- |
| **MaxDurability** | 500 writes/sec    | 50ms        | 200ms       | ✅ **Aucune**   |
| Performance       | 30,000 writes/sec | 1ms         | 5ms         | ⚠️ **Max 10ms** |

---

## 🚀 Future : WAL Mode (Option C)

### Vision

```rust
// Future : Write-Ahead Log comme PostgreSQL
let writer = AsyncWriter::with_durability(
    storage,
    1000,
    DurabilityMode::WAL  // 🚀 Performance + Durabilité !
);
```

**Principe :**

1. Écriture immédiate dans WAL petit et rapide (fsync)
2. Flush async vers la base principale
3. **Best of both worlds** : Durabilité + Performance

**Performance attendue :**

- 50,000+ writes/sec
- Durabilité garantie 100%
- Comme PostgreSQL

**Statut :** Roadmap future (Phase 4+)

---

## ✅ Conclusion

**Lithair suit les standards de l'industrie :**

- ✅ Durabilité par défaut (comme PostgreSQL, MySQL, MongoDB)
- ✅ Mode performance optionnel pour benchmarks
- ✅ Choix explicite et documenté

**Pour une DB event-sourced, la durabilité n'est PAS négociable.**

> _"Si tu perds un seul événement, toute ton histoire est corrompue."_
> — Principes Event Sourcing
