# Schema Migration Demo

**Test principal pour valider le système de migration de schéma de Lithair.**

Ce test valide le cycle complet de migration de schéma :
- **Automatic schema change detection** - Détecte AddField, RemoveField, ModifyField
- **Migration classification** - Additive vs Breaking vs Versioned
- **Pending changes en mode Manual** - Workflow d'approbation humaine
- **Persistance sur disque** - Sauvegarde après approbation
- **Lock/Unlock mechanism** - Maintenance window pattern for deployments
- **History tracking** - Persistent audit trail of all schema changes
- **Multiple modes** - warn, strict, auto, manual migration strategies

## Scripts Helper

```bash
# Lancer tous les tests (mode warn par défaut)
./examples/schema_migration_demo/run-tests.sh

# Lancer tests en mode Manual (test 16 inclus)
./examples/schema_migration_demo/run-tests.sh manual

# Tester le workflow approve + persistence
./examples/schema_migration_demo/test-approve.sh

# Voir le status actuel
./examples/schema_migration_demo/show-status.sh
```

## Quick Start

```bash
# 1. Start server (creates initial schema)
cargo run -p schema_migration_demo

# 2. In another terminal, test the API
curl http://localhost:8090/api/products

# 3. Test lock/unlock
curl -X POST http://localhost:8090/_admin/schema/lock
curl http://localhost:8090/_admin/schema/lock/status
curl -X POST http://localhost:8090/_admin/schema/unlock -d '{"duration_seconds": 60}'

# 4. View history
curl http://localhost:8090/_admin/schema/history
```

## CLI Commands

```bash
# Run server
cargo run -p schema_migration_demo -- -p 8090

# Show stored schema
cargo run -p schema_migration_demo -- --show-schema

# Show change history
cargo run -p schema_migration_demo -- --show-history

# Show lock status
cargo run -p schema_migration_demo -- --show-lock

# Run automated tests (server must be running)
cargo run -p schema_migration_demo -- --test

# Reset all data
cargo run -p schema_migration_demo -- --reset-schema
```

## CLI Options

```
Options:
  -p, --port <PORT>              Port to listen on [default: 8090]
  -d, --data-dir <DATA_DIR>      Data directory [default: ./data/schema_demo]
  -m, --migration-mode <MODE>    Migration mode: warn, strict, auto, manual [default: warn]
      --no-validation            Disable schema validation
      --show-schema              Show stored schema and exit
      --show-history             Show schema change history and exit
      --show-lock                Show lock status and exit
      --reset-schema             Delete stored schema and exit
      --test                     Run automated tests against a running server
      --test-url <URL>           Server URL for tests [default: http://localhost:8090]
  -h, --help                     Print help
```

### Modes de migration

| Mode | Flag | Description |
|------|------|-------------|
| Warn | `-m warn` | Log les changements, auto-accepte (défaut) |
| Strict | `-m strict` | Refuse les breaking changes au démarrage |
| Auto | `-m auto` | Sauvegarde auto tous les changements |
| Manual | `-m manual` | Crée des pending, requiert approbation |

## How It Works

1. **First Run**: Schema is extracted from the `Product` struct and saved to `.schema/Product.json`
2. **Subsequent Runs**: Current schema is compared with stored version
3. **Changes Detected**: Logged with migration strategy (Additive/Breaking/Safe)
4. **History Recorded**: All applied changes are persisted to `schema_history.json`

## Testing Schema Changes

### Step 1: Establish Baseline

```bash
cargo run -p schema_migration_demo
# Output: "Product - first run, saving schema v1"
```

### Step 2: Modify the Model

Edit `src/main.rs` and uncomment one of the test fields:

```rust
// Additive change (safe - nullable field)
pub discount: Option<f64>,

// Breaking change (needs default value)
pub sku: String,

// Safe migration (has default value!)
#[db(default = 0)]
#[serde(default)]
pub rating: i32,
```

### Step 3: Run Again

```bash
cargo run -p schema_migration_demo
```

Output:
```
Validating model schemas...
   Product - 1 schema change(s) detected:
      - AddField on 'discount' (Additive)
Schema validation complete
```

## Migration Modes

| Mode | Behavior | Use Case |
|------|----------|----------|
| `warn` | Log changes, continue | Development (default) |
| `strict` | Fail on breaking changes | Production |
| `auto` | Save new schema automatically | CI/CD |
| `manual` | Create pending, require approval | Production avec approbation humaine |

### Set via CLI

```bash
cargo run -p schema_migration_demo -- -m strict
cargo run -p schema_migration_demo -- -m manual  # Mode avec approbation
```

## Mode Manual (Workflow d'approbation)

Le mode `manual` est le mode recommandé pour la production. Il crée des "pending changes" qui doivent être approuvés avant d'être appliqués.

### Démarrage en mode Manual

```bash
# Préparer avec baseline v1 (7 champs)
rm -rf ./data/schema_demo
mkdir -p ./data/schema_demo/.schema
cp examples/schema_migration_demo/baseline/Product_v1.json ./data/schema_demo/.schema/Product.json

# Lancer en mode manual
cargo run -p schema_migration_demo -- -p 8090 -m manual
```

### Output au démarrage

```
🔍 Validating model schemas...
   ⚠️  Product - 3 schema change(s) detected:
      - AddField on 'priority' (Additive)
      - AddField on 'category' (Additive)
      - AddField on 'featured' (Additive)
      🔒 Manual mode: change pending approval (id: a5fc2044-27c0-4b43-85ca-965468116f0c)
      ⏳ Approve via: POST /_admin/schema/approve/a5fc2044-27c0-4b43-85ca-965468116f0c
✅ Schema validation complete
```

### Workflow d'approbation

```bash
# 1. Voir les pending changes
curl http://localhost:8090/_admin/schema/pending | jq .

# 2. Approuver le changement
curl -X POST http://localhost:8090/_admin/schema/approve/{pending_id}

# Response:
# {
#   "status": "applied",
#   "message": "Schema change approved, applied, and persisted",
#   "model": "Product"
# }

# 3. Vérifier que le schéma est persisté sur disque
cat ./data/schema_demo/.schema/Product.json | jq '.fields | keys | length'
# Output: 10 (avant: 7)
```

### Flow de migration Manual

```
┌─────────────────────────────────────────────────────────────┐
│ 1. Démarrage serveur avec -m manual                         │
│    - Charge schéma depuis .schema/Product.json (7 champs)   │
│    - Compare avec schéma du code Rust (10 champs)           │
│    - Détecte 3 changements (priority, category, featured)   │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ 2. Création du pending change                               │
│    - ID unique généré (UUID)                                │
│    - Stocké en mémoire dans schema_sync_state               │
│    - Visible via GET /_admin/schema/pending                 │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ 3. Attente d'approbation                                    │
│    - Serveur tourne normalement                             │
│    - Schéma sur disque inchangé (7 champs)                  │
│    - Admin peut approuver ou rejeter                        │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ 4. POST /_admin/schema/approve/{id}                         │
│    - Applique le changement en mémoire                      │
│    - Persiste sur disque (.schema/Product.json)             │
│    - Log: "💾 Schema for 'Product' persisted to disk"       │
│    - Schéma sur disque: 10 champs                           │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ 5. Prochain redémarrage                                     │
│    - Charge le nouveau schéma (10 champs)                   │
│    - Compare avec code (10 champs)                          │
│    - Aucun changement détecté ✅                            │
└─────────────────────────────────────────────────────────────┘
```

## Lock/Unlock Mechanism

The lock/unlock feature implements a "maintenance window" pattern for schema migrations.

### Lock Schema Changes

```bash
# Lock all migrations
curl -X POST http://localhost:8090/_admin/schema/lock \
  -H "Content-Type: application/json" \
  -d '{"reason": "Production freeze for holiday"}'
```

Response:
```json
{
  "status": "locked",
  "reason": "Production freeze for holiday",
  "message": "Schema migrations are now locked. All changes will be rejected."
}
```

### Unlock with Timeout

```bash
# Unlock for 30 minutes (auto-relock after)
curl -X POST http://localhost:8090/_admin/schema/unlock \
  -H "Content-Type: application/json" \
  -d '{
    "reason": "v2.5 deployment",
    "duration_seconds": 1800,
    "unlocked_by": "admin@example.com"
  }'
```

Response:
```json
{
  "status": "unlocked",
  "reason": "v2.5 deployment",
  "unlocked_by": "admin@example.com",
  "duration_seconds": 1800,
  "auto_relock_at": 1704806400,
  "message": "Schema migrations are now unlocked. (auto-relock in 1800s)"
}
```

### Check Lock Status

```bash
curl http://localhost:8090/_admin/schema/lock/status
```

Response:
```json
{
  "locked": false,
  "reason": "v2.5 deployment",
  "unlocked_by": "admin@example.com",
  "remaining_seconds": 1750
}
```

## Admin API Endpoints

### Lock/Unlock

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/_admin/schema/lock/status` | Get current lock status |
| `POST` | `/_admin/schema/lock` | Lock schema migrations |
| `POST` | `/_admin/schema/unlock` | Unlock schema migrations |

### History & Diff

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/_admin/schema/history` | Get schema change history |
| `GET` | `/_admin/schema/diff` | Get current schema differences |

### Products API

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/products` | List all products |
| `POST` | `/api/products` | Create a product |
| `GET` | `/api/products/{id}` | Get a product |
| `PUT` | `/api/products/{id}` | Update a product |
| `DELETE` | `/api/products/{id}` | Delete a product |

## Automated Tests (16 tests)

Run the built-in test suite against a running server:

```bash
# Terminal 1: Start server
cargo run -p schema_migration_demo -- -p 8090

# Terminal 2: Run tests
cargo run -p schema_migration_demo -- --test
```

### Liste complète des tests

| # | Test | Description | Critique |
|---|------|-------------|----------|
| 1 | API Health Check | Vérifie `/api/products` répond | |
| 2 | Lock Status Endpoint | `GET /_admin/schema/lock` retourne le statut | |
| 3 | Lock Endpoint | `POST /_admin/schema/lock` verrouille | |
| 4 | Verify Lock Active | Confirme le verrouillage actif | |
| 5 | Unlock with Timeout | `POST /_admin/schema/unlock` avec timeout | |
| 6 | History Endpoint | `GET /_admin/schema/history` retourne l'historique | |
| 7 | Schema Diff Endpoint | `GET /_admin/schema/diff` compare code vs disque | |
| 8 | Create Product | `POST /api/products` crée un produit | |
| 9 | Migration Test (AddField) | Détecte 3 AddField (priority, category, featured) | ⭐ |
| 10 | List Schemas Endpoint | `GET /_admin/schema` liste les schémas | |
| 11 | Pending Changes Endpoint | `GET /_admin/schema/pending` liste les pending | |
| 12 | Breaking Change (RemoveField) | Détecte RemoveField comme Breaking | ⭐ |
| 13 | Lock Blocks Revalidate | Revalidate bloqué (HTTP 423) si locked | |
| 14 | History After Changes | Historique contient les changements | |
| 15 | Schema Sync Endpoint | `POST /_admin/schema/sync` (400 standalone) | |
| 16 | Approve + Disk Persistence | Approve persiste sur disque (7→10 champs) | ⭐ Manual |

### Tests critiques (⭐)

#### Test 9: Migration Test (AddField)
Simule une migration réelle :
1. Sauvegarde le schéma actuel
2. Remplace par baseline v1 (7 champs)
3. Appelle `POST /_admin/schema/revalidate`
4. Vérifie détection de 3 AddField
5. Restaure le schéma original

#### Test 12: Breaking Change (RemoveField)
Détecte les breaking changes :
1. Utilise baseline v2 avec `legacy_sku` (11 champs)
2. Le code n'a pas `legacy_sku` → RemoveField détecté
3. Vérifie que RemoveField est classé comme "Breaking"

#### Test 16: Approve + Disk Persistence (Mode Manual requis)
Vérifie l'approbation et persistance :
1. Remplace schéma par baseline v1 (7 champs)
2. Appelle revalidate → crée pending en mode Manual
3. Récupère l'ID du pending via `GET /_admin/schema/pending`
4. Appelle `POST /_admin/schema/approve/{id}`
5. Vérifie schéma persisté sur disque (10 champs)

### Lancer les tests en mode Manual

```bash
# Terminal 1: Start server with Manual mode
rm -rf ./data/schema_demo && mkdir -p ./data/schema_demo/.schema
cp examples/schema_migration_demo/baseline/Product_v1.json ./data/schema_demo/.schema/Product.json
cargo run -p schema_migration_demo -- -p 8090 -m manual

# Terminal 2: Run tests
cargo run -p schema_migration_demo -- --test
```

### Output attendu

```
🧪 Running Schema Migration Tests
   Target: http://localhost:8090

  1. API Health Check... ✅ OK
  2. Lock Status Endpoint... ✅ OK (locked: false)
  3. Lock Endpoint... ✅ OK
  4. Verify Lock Active... ✅ OK (confirmed locked)
  5. Unlock with Timeout... ✅ OK (expires in 300s)
  6. History Endpoint... ✅ OK (0 change(s))
  7. Schema Diff Endpoint... ✅ OK
  8. Create Product... ✅ OK
  9. Migration Test (AddField)... ✅ OK (3 changes detected, history updated)
 10. List Schemas Endpoint... ✅ OK (1 schema(s))
 11. Pending Changes Endpoint... ✅ OK
 12. Breaking Change (RemoveField)... ✅ OK (RemoveField detected as Breaking)
 13. Lock Blocks Revalidate... ✅ OK (revalidate correctly blocked)
 14. History After Changes... ✅ OK (2 change(s) recorded)
 15. Schema Sync Endpoint... ✅ OK (standalone mode)
 16. Approve + Disk Persistence... ✅ OK (approved & persisted 10 fields)

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Results: 16 passed, 0 failed
  🎉 All tests passed!
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

> **Note**: Si le serveur n'est pas en mode Manual, le test 16 est skippé avec `⏭️  SKIPPED (requires -m manual)`

## Change Types

| Type | Example | Strategy | Consensus |
|------|---------|----------|-----------|
| AddField (nullable) | `pub foo: Option<T>` | Additive | No |
| AddField (with default) | `#[db(default = 0)]` | Safe | No |
| AddField (required) | `pub foo: T` | Breaking | Yes |
| RemoveField | Delete field | Breaking | Yes |
| AddIndex | `#[db(indexed)]` | Additive | No |
| RemoveIndex | Remove `#[db(indexed)]` | Breaking | Yes |

## Data Storage

```
data/schema_demo/
├── .schema/
│   └── Product.json        # Stored schema specification
├── schema_history.json     # Change history (persistent)
├── schema_lock.json        # Lock status (persistent)
└── products/
    └── ...                 # Product data (event log)
```

### Example History Entry

```json
{
  "changes": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "model_name": "Product",
      "changes": [
        {
          "model": "Product",
          "change_type": "AddField",
          "field_name": "priority",
          "migration_strategy": "Additive"
        }
      ],
      "applied_at": 1704806400,
      "applied_by_node": 0
    }
  ]
}
```

## Safe Migration Pattern

Use `#[db(default = X)]` with `#[serde(default)]` for safe migrations:

```rust
/// New field with default - SAFE MIGRATION
#[db(default = 0)]
#[serde(default)]
pub priority: i32,

/// New field with custom default - SAFE MIGRATION
#[db(default = "uncategorized")]
#[serde(default = "__default_category")]
pub category: String,
```

Old events will automatically get the default value during deserialization.

## Fichiers Baseline

Les fichiers baseline sont dans `examples/schema_migration_demo/baseline/`.

### Product_v1.json (7 champs)

Version minimale du schéma, sans les nouveaux champs.

```json
{
  "model_name": "Product",
  "version": 1,
  "fields": {
    "id": { "primary_key": true },
    "name": { "indexed": true },
    "description": {},
    "price_cents": {},
    "stock": {},
    "active": {},
    "created_at": {}
  }
}
```

### Product_v2_with_legacy.json (11 champs)

Version avec un champ supplémentaire `legacy_sku` qui n'existe pas dans le code actuel.
Utilisé pour tester la détection de RemoveField (breaking change).

```json
{
  "model_name": "Product",
  "version": 2,
  "fields": {
    // ... 10 champs du modèle actuel ...
    "legacy_sku": { "unique": true, "indexed": true }  // Champ qui sera "supprimé"
  }
}
```

## Troubleshooting

### "Address already in use"

```bash
lsof -ti:8090 | xargs kill -9
```

### Test 16 skipped

Le test 16 nécessite le mode Manual. Lancez le serveur avec `-m manual`.

### Schéma pas trouvé

```bash
mkdir -p ./data/schema_demo/.schema
cp examples/schema_migration_demo/baseline/Product_v1.json ./data/schema_demo/.schema/Product.json
```

### Vérifier le contenu du schéma

```bash
cat ./data/schema_demo/.schema/Product.json | jq '.fields | keys'
cat ./data/schema_demo/.schema/Product.json | jq '.fields | keys | length'
```

### Reset complet

```bash
rm -rf ./data/schema_demo
```

## Évolutions futures

- [ ] Test de rollback après échec
- [ ] Test de migration multi-modèles
- [ ] Test de consensus en mode cluster
- [ ] Test de timeout sur pending changes
- [ ] Métriques de performance des migrations
- [ ] Test de reject (POST /_admin/schema/reject/{id})

## See Also

- [Migration System Overview](../../docs/features/migration/overview.md)
- [Schema Detection](../../docs/features/migration/schema-detection.md)
