# Schema Migration Demo

**Main test to validate Lithair's schema migration system.**

This test validates the full schema migration lifecycle:
- **Automatic schema change detection** - Detects AddField, RemoveField, ModifyField
- **Migration classification** - Additive vs Breaking vs Versioned
- **Pending changes in Manual mode** - Human approval workflow
- **Disk persistence** - Saved after approval
- **Lock/Unlock mechanism** - Maintenance window pattern for deployments
- **History tracking** - Persistent audit trail of all schema changes
- **Multiple modes** - warn, strict, auto, manual migration strategies

## Helper Scripts

```bash
# Run all tests (warn mode by default)
./examples/08-schema-migration/run-tests.sh

# Run tests in Manual mode (includes test 16)
./examples/08-schema-migration/run-tests.sh manual

# Test the approve + persistence workflow
./examples/08-schema-migration/test-approve.sh

# View current status
./examples/08-schema-migration/show-status.sh
```

## Quick Start

```bash
# 1. Start server (creates initial schema)
cargo run -p schema-migration

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
cargo run -p schema-migration -- -p 8090

# Show stored schema
cargo run -p schema-migration -- --show-schema

# Show change history
cargo run -p schema-migration -- --show-history

# Show lock status
cargo run -p schema-migration -- --show-lock

# Run automated tests (server must be running)
cargo run -p schema-migration -- --test

# Reset all data
cargo run -p schema-migration -- --reset-schema
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

### Migration Modes

| Mode | Flag | Description |
|------|------|-------------|
| Warn | `-m warn` | Logs changes, auto-accepts (default) |
| Strict | `-m strict` | Rejects breaking changes at startup |
| Auto | `-m auto` | Automatically saves all changes |
| Manual | `-m manual` | Creates pending changes, requires approval |

## How It Works

1. **First Run**: Schema is extracted from the `Product` struct and saved to `.schema/Product.json`
2. **Subsequent Runs**: Current schema is compared with stored version
3. **Changes Detected**: Logged with migration strategy (Additive/Breaking/Safe)
4. **History Recorded**: All applied changes are persisted to `schema_history.json`

## Testing Schema Changes

### Step 1: Establish Baseline

```bash
cargo run -p schema-migration
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
cargo run -p schema-migration
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
| `manual` | Create pending, require approval | Production with human approval |

### Set via CLI

```bash
cargo run -p schema-migration -- -m strict
cargo run -p schema-migration -- -m manual  # Mode with approval
```

## Manual Mode (Approval Workflow)

The `manual` mode is the recommended mode for production. It creates "pending changes" that must be approved before being applied.

### Starting in Manual Mode

```bash
# Set up with baseline v1 (7 fields)
rm -rf ./data/schema_demo
mkdir -p ./data/schema_demo/.schema
cp examples/08-schema-migration/baseline/Product_v1.json ./data/schema_demo/.schema/Product.json

# Start in manual mode
cargo run -p schema-migration -- -p 8090 -m manual
```

### Startup Output

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

### Approval Workflow

```bash
# 1. View pending changes
curl http://localhost:8090/_admin/schema/pending | jq .

# 2. Approve the change
curl -X POST http://localhost:8090/_admin/schema/approve/{pending_id}

# Response:
# {
#   "status": "applied",
#   "message": "Schema change approved, applied, and persisted",
#   "model": "Product"
# }

# 3. Verify the schema has been persisted to disk
cat ./data/schema_demo/.schema/Product.json | jq '.fields | keys | length'
# Output: 10 (before: 7)
```

### Manual Migration Flow

```
┌─────────────────────────────────────────────────────────────┐
│ 1. Server starts with -m manual                              │
│    - Loads schema from .schema/Product.json (7 fields)       │
│    - Compares with Rust code schema (10 fields)              │
│    - Detects 3 changes (priority, category, featured)        │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ 2. Pending change created                                    │
│    - Unique ID generated (UUID)                              │
│    - Stored in memory in schema_sync_state                   │
│    - Visible via GET /_admin/schema/pending                  │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ 3. Awaiting approval                                         │
│    - Server runs normally                                    │
│    - On-disk schema unchanged (7 fields)                     │
│    - Admin can approve or reject                             │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ 4. POST /_admin/schema/approve/{id}                          │
│    - Applies the change in memory                            │
│    - Persists to disk (.schema/Product.json)                 │
│    - Log: "💾 Schema for 'Product' persisted to disk"        │
│    - On-disk schema: 10 fields                               │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ 5. Next restart                                              │
│    - Loads the new schema (10 fields)                        │
│    - Compares with code (10 fields)                          │
│    - No changes detected ✅                                  │
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
cargo run -p schema-migration -- -p 8090

# Terminal 2: Run tests
cargo run -p schema-migration -- --test
```

### Full Test List

| # | Test | Description | Critical |
|---|------|-------------|----------|
| 1 | API Health Check | Verifies `/api/products` responds | |
| 2 | Lock Status Endpoint | `GET /_admin/schema/lock` returns status | |
| 3 | Lock Endpoint | `POST /_admin/schema/lock` locks migrations | |
| 4 | Verify Lock Active | Confirms lock is active | |
| 5 | Unlock with Timeout | `POST /_admin/schema/unlock` with timeout | |
| 6 | History Endpoint | `GET /_admin/schema/history` returns history | |
| 7 | Schema Diff Endpoint | `GET /_admin/schema/diff` compares code vs disk | |
| 8 | Create Product | `POST /api/products` creates a product | |
| 9 | Migration Test (AddField) | Detects 3 AddField (priority, category, featured) | ⭐ |
| 10 | List Schemas Endpoint | `GET /_admin/schema` lists schemas | |
| 11 | Pending Changes Endpoint | `GET /_admin/schema/pending` lists pending changes | |
| 12 | Breaking Change (RemoveField) | Detects RemoveField as Breaking | ⭐ |
| 13 | Lock Blocks Revalidate | Revalidate blocked (HTTP 423) when locked | |
| 14 | History After Changes | History contains the changes | |
| 15 | Schema Sync Endpoint | `POST /_admin/schema/sync` (400 standalone) | |
| 16 | Approve + Disk Persistence | Approve persists to disk (7 to 10 fields) | ⭐ Manual |

### Critical Tests (⭐)

#### Test 9: Migration Test (AddField)
Simulates a real migration:
1. Saves the current schema
2. Replaces it with baseline v1 (7 fields)
3. Calls `POST /_admin/schema/revalidate`
4. Verifies detection of 3 AddField changes
5. Restores the original schema

#### Test 12: Breaking Change (RemoveField)
Detects breaking changes:
1. Uses baseline v2 with `legacy_sku` (11 fields)
2. The code does not have `legacy_sku` -- RemoveField detected
3. Verifies that RemoveField is classified as "Breaking"

#### Test 16: Approve + Disk Persistence (Manual mode required)
Verifies approval and persistence:
1. Replaces schema with baseline v1 (7 fields)
2. Calls revalidate -- creates pending change in Manual mode
3. Retrieves the pending ID via `GET /_admin/schema/pending`
4. Calls `POST /_admin/schema/approve/{id}`
5. Verifies schema is persisted to disk (10 fields)

### Running Tests in Manual Mode

```bash
# Terminal 1: Start server with Manual mode
rm -rf ./data/schema_demo && mkdir -p ./data/schema_demo/.schema
cp examples/08-schema-migration/baseline/Product_v1.json ./data/schema_demo/.schema/Product.json
cargo run -p schema-migration -- -p 8090 -m manual

# Terminal 2: Run tests
cargo run -p schema-migration -- --test
```

### Expected Output

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

> **Note**: If the server is not running in Manual mode, test 16 is skipped with `⏭️  SKIPPED (requires -m manual)`

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

## Baseline Files

The baseline files are in `examples/08-schema-migration/baseline/`.

### Product_v1.json (7 fields)

Minimal schema version, without the new fields.

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

### Product_v2_with_legacy.json (11 fields)

Version with an additional `legacy_sku` field that does not exist in the current code.
Used to test RemoveField detection (breaking change).

```json
{
  "model_name": "Product",
  "version": 2,
  "fields": {
    // ... 10 fields from the current model ...
    "legacy_sku": { "unique": true, "indexed": true }  // Field that will be "removed"
  }
}
```

## Troubleshooting

### "Address already in use"

```bash
lsof -ti:8090 | xargs kill -9
```

### Test 16 skipped

Test 16 requires Manual mode. Start the server with `-m manual`.

### Schema not found

```bash
mkdir -p ./data/schema_demo/.schema
cp examples/08-schema-migration/baseline/Product_v1.json ./data/schema_demo/.schema/Product.json
```

### Verify schema contents

```bash
cat ./data/schema_demo/.schema/Product.json | jq '.fields | keys'
cat ./data/schema_demo/.schema/Product.json | jq '.fields | keys | length'
```

### Full reset

```bash
rm -rf ./data/schema_demo
```

## Planned Improvements

- [ ] Rollback test after failure
- [ ] Multi-model migration test
- [ ] Consensus test in cluster mode
- [ ] Timeout test on pending changes
- [ ] Migration performance metrics
- [ ] Reject test (POST /_admin/schema/reject/{id})

## See Also

- [Migration System Overview](../../docs/features/migration/overview.md)
- [Schema Detection](../../docs/features/migration/schema-detection.md)
