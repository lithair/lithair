# 📚 Lithair Declarative Attributes Reference Guide

## 🎯 **Overview**

Lithair uses **declarative attributes** to define data behavior directly in their structure. Each attribute encapsulates a dimension of behavior (database, lifecycle, HTTP, permissions, persistence).

```rust
#[derive(DeclarativeModel)]
pub struct Product {
    #[db(primary_key)]                    // 🗄️ DB constraints
    #[lifecycle(immutable)]               // 🔄 Lifecycle
    #[http(expose)]                       // 🌐 HTTP API
    #[persistence(replicate)]             // 💾 Distribution
    #[permission(read = "ProductRead")]   // 🔒 Security
    pub id: Uuid,
}
```

---

## 🗄️ **Database Attributes (`#[db(...)]`)**

Define constraints and properties at the database level.

### 📋 **Syntax**
```rust
#[db(constraint1, constraint2, constraint3 = "value")]
```

### 🔑 **Available Constraints**

| Attribute | Description | Example | Impact |
|----------|-------------|---------|--------|
| `primary_key` | Primary key | `#[db(primary_key)]` | ✅ Unique index, immutable by default |
| `unique` | Uniqueness constraint | `#[db(unique)]` | ✅ Unique index, automatic validation |
| `indexed` | Index for performance | `#[db(indexed)]` | ⚡ Fast search |
| `nullable` | Allow null values | `#[db(nullable)]` | 🔄 Requires `Option<T>` type |
| `fk = "Model"` | Foreign key | `#[db(fk = "User")]` | 🔗 Reference to other model |

### 📝 **Detailed Examples**

```rust
#[derive(DeclarativeModel)]
pub struct User {
    #[db(primary_key)]
    pub id: Uuid,                          // ✅ Auto-generated PK
    
    #[db(unique, indexed)]  
    pub email: String,                     // ✅ Unique email + index
    
    #[db(indexed)]
    pub username: String,                  // ⚡ Fast search
    
    #[db(nullable)]
    pub phone: Option<String>,             // 🔄 Optional
}

#[derive(DeclarativeModel)]
pub struct Order {
    #[db(primary_key)]
    pub id: Uuid,
    
    #[db(fk = "User", indexed)]           // 🔗 FK + index
    pub customer_id: Uuid,
    
    #[db(indexed)]                        // ⚡ Search by status
    pub status: OrderStatus,
}
```

### ⚙️ **Automatic Behaviors**

```rust
// Lithair automatically generates:

// 1. DB constraints
CREATE TABLE users (
    id UUID PRIMARY KEY,
    email VARCHAR UNIQUE,
    username VARCHAR,
    phone VARCHAR NULL
);
CREATE INDEX idx_users_email ON users(email);
CREATE INDEX idx_users_username ON users(username);

// 2. Validation at insertion
if existing_user_with_email.is_some() {
    return Err("Email already exists");
}

// 3. Optimized queries  
fn find_by_email(email: &str) -> Option<User> {
    // Automatically uses the index
}
```

---

## 🔄 **Lifecycle Attributes (`#[lifecycle(...)]`)**

Define lifecycle management and temporal data evolution.

### 📋 **Syntax**
```rust
#[lifecycle(policy1, policy2, retention = 365)]
```

### 🕰️ **Available Policies**

| Attribute | Description | Example | Impact |
|----------|-------------|---------|--------|
| `immutable` | Never changes | `#[lifecycle(immutable)]` | 🔒 Error if modification attempted |
| `audited` | Complete history | `#[lifecycle(audited)]` | 📝 All modifications tracked |
| `versioned = N` | Max N versions | `#[lifecycle(versioned = 5)]` | 🔄 Keep last 5 versions |
| `snapshot_only` | No intermediate events | `#[lifecycle(snapshot_only)]` | 📸 Only final state matters |
| `retention = N` | N days retention | `#[lifecycle(retention = 365)]` | 🗑️ Auto-delete after 1 year |

### 📝 **Detailed Examples**

```rust
#[derive(DeclarativeModel)]
pub struct Article {
    #[lifecycle(immutable)]
    pub id: Uuid,                          // 🔒 Never changes
    
    #[lifecycle(audited)]
    pub title: String,                     // 📝 Complete history
    
    #[lifecycle(audited, retention = 90)]
    pub content: String,                   // 📝 + 🗑️ Deleted after 90d
    
    #[lifecycle(versioned = 3)]
    pub metadata: serde_json::Value,       // 🔄 Last 3 versions
    
    #[lifecycle(snapshot_only)]
    pub view_count: u32,                   // 📸 Current value only
}

#[derive(DeclarativeModel)]  
pub struct UserProfile {
    #[lifecycle(audited, versioned = 10, retention = 1095)]
    pub sensitive_data: String,            // 📝 + 🔄 + 🗑️ Combined
}
```

### ⚙️ **Automatic Behaviors**

```rust
// For #[lifecycle(audited)]:
GET /articles/{id}/history
// Automatically returns:
[
    {
        "field": "title",
        "old_value": "Old Title", 
        "new_value": "New Title",
        "changed_at": "2024-01-15T10:30:00Z",
        "changed_by": "user-uuid"
    }
]

// For #[lifecycle(versioned = 3)]:
let versions = article.get_field_versions("metadata"); 
// Returns last 3 versions automatically

// For #[lifecycle(retention = 90)]:
// Auto-generated background task:
DELETE FROM article_history 
WHERE field = 'content' 
AND changed_at < NOW() - INTERVAL '90 days';
```

---

## 🌐 **HTTP Attributes (`#[http(...)]`)**

Control field exposure and validation in REST API.

### 📋 **Syntax**
```rust
#[http(expose, validate = "rule", serialize = "format")]
```

### 🌍 **Available Options**

| Attribute | Description | Example | Impact |
|----------|-------------|---------|--------|
| `expose` | Exposed in API | `#[http(expose)]` | 🌐 Included in JSON responses |
| `expose = false` | Hidden from API | `#[http(expose = false)]` | 🚫 Never in responses |
| `validate = "rule"` | Validation rule | `#[http(validate = "email")]` | ✅ Validation before persistence |
| `serialize = "format"` | Serialization format | `#[http(serialize = "base64")]` | 🔄 Transformation before JSON |

### ✅ **Validation Rules**

| Rule | Description | Example | Validation |
|-------|-------------|---------|------------|
| `email` | Email format | `validate = "email"` | `user@domain.com` |
| `length(min, max)` | Length | `validate = "length(5, 50)"` | Between 5 and 50 chars |
| `min_length(n)` | Minimum length | `validate = "min_length(8)"` | At least 8 chars |
| `range(min, max)` | Numeric value | `validate = "range(1, 100)"` | Between 1 and 100 |
| `regex("pattern")` | Regular expression | `validate = "regex(\"^[A-Z]+$\")"` | Uppercase letters |
| `non_empty` | Non-empty | `validate = "non_empty"` | Non-empty string |
| `url` | Valid URL | `validate = "url"` | `https://example.com` |
| `uuid` | Valid UUID | `validate = "uuid"` | Correct UUID format |

### 📝 **Detailed Examples**

```rust
#[derive(DeclarativeModel)]
pub struct User {
    #[http(expose)]
    pub id: Uuid,                          // 🌐 Always visible
    
    #[http(expose, validate = "email")]
    pub email: String,                     // 🌐 + ✅ Email validation
    
    #[http(expose, validate = "length(3, 50)")]  
    pub username: String,                  // 🌐 + ✅ 3-50 characters
    
    #[http(expose = false)]
    pub password_hash: String,             // 🚫 Never exposed
    
    #[http(expose, validate = "url")]
    pub website: Option<String>,           // 🌐 + ✅ Valid URL if present
}

#[derive(DeclarativeModel)]
pub struct Product {
    #[http(expose)]
    pub id: Uuid,
    
    #[http(expose, validate = "length(1, 200)")]
    pub name: String,                      // 🌐 + ✅ Required name
    
    #[http(expose, validate = "range(0.01, 999999.99)")]
    pub price: f64,                        // 🌐 + ✅ Positive price
    
    #[http(expose, serialize = "base64")]
    pub image_data: Vec<u8>,               // 🌐 + 🔄 Base64 in JSON
    
    #[http(expose, validate = "regex(\"^[A-Z]{3}-[0-9]{4}$\")")]
    pub sku: String,                       // 🌐 + ✅ Format ABC-1234
}
```

### ⚙️ **Automatically Generated API**

```rust
// Lithair automatically generates:

// 1. CRUD routes with validation
POST   /users              // With email, username validation
PUT    /users/{id}          // With validation of modified fields
GET    /users/{id}          // password_hash never included

// 2. Consistent JSON responses
{
    "id": "uuid-here",
    "email": "user@example.com", 
    "username": "john_doe",
    "website": "https://john.dev"
    // password_hash automatically omitted
}

// 3. Structured validation errors  
{
    "error": "validation_failed",
    "details": {
        "email": "Invalid email format",
        "username": "Must be between 3 and 50 characters"
    }
}
```

---

## 💾 **Persistence Attributes (`#[persistence(...)]`)**

**NEW**: Fine-grained control of data persistence and distribution.

### 📋 **Syntax**
```rust
#[persistence(strategy1, strategy2)]
```

### 🌐 **Available Strategies**

| Attribute | Description | Example | Impact |
|----------|-------------|---------|--------|
| `memory_only` | Memory only | `#[persistence(memory_only)]` | ⚡ Fast, lost on reboot |
| `persist` | Disk persistence | `#[persistence(persist)]` | 💾 Saved to disk |
| `auto_persist` | Automatic persistence | `#[persistence(auto_persist)]` | 💾 Save on every write |
| `replicate` | Distributed replication | `#[persistence(replicate)]` | 🌐 Replicated across all nodes |
| `track_history` | Event history | `#[persistence(track_history)]` | 📝 Modification journal |
| `no_replication` | Exclude from replication | `#[persistence(no_replication)]` | 🏠 Local to node only |

### 📝 **Use Case Examples**

```rust
#[derive(DeclarativeModel)]
pub struct UserSession {
    #[persistence(memory_only)]           // ⚡ Fast cache
    pub session_token: String,
    
    #[persistence(persist)]               // 💾 Survives restarts
    pub user_id: Uuid,
    
    #[persistence(replicate, track_history)] // 🌐 + 📝 Critical
    pub login_time: DateTime<Utc>,
}

#[derive(DeclarativeModel)]
pub struct Order {
    #[persistence(replicate, track_history)] // 🌐 + 📝 Critical data
    pub total_amount: f64,
    
    #[persistence(replicate, track_history)]
    pub status: OrderStatus,
    
    #[persistence(auto_persist)]          // 💾 Auto-save 
    pub customer_notes: String,
    
    #[persistence(memory_only)]           // ⚡ Temporary calculation
    pub processing_metadata: serde_json::Value,
}

#[derive(DeclarativeModel)]
pub struct AnalyticsEvent {
    #[persistence(persist, no_replication)] // 💾 Local, not replicated
    pub user_agent: String,
    
    #[persistence(memory_only)]           // ⚡ Real-time aggregation
    pub temp_counters: HashMap<String, u64>,
    
    #[persistence(replicate)]             // 🌐 Shared metrics
    pub event_type: String,
}
```

### ⚙️ **Automatic Behaviors**

```rust
// Engine configuration based on attributes:

// memory_only -> Fast L1 cache
let cache_engine = MemoryEngine::new();

// persist -> SCC2 with FileStorage
let persistent_engine = Scc2Engine::new(event_store, config);

// replicate -> Automatic Raft distribution
let distributed_engine = RaftEngine::new(cluster_config);

// track_history -> Complete event sourcing
let events = get_field_history("total_amount");
// [
//     {"old": 100.0, "new": 120.0, "at": "2024-01-15T10:00:00Z"},
//     {"old": 120.0, "new": 99.99, "at": "2024-01-15T11:00:00Z"}
// ]
```

---

## 🔒 **Permission Attributes (`#[permission(...)]`)**

Define security policies and access control at the field level.

### 📋 **Syntax**
```rust
#[permission(read = "Permission", write = "Permission")]
#[rbac(owner_field, role_based)]
```

### 🛡️ **Available Permissions**

| Attribute | Description | Example | Impact |
|----------|-------------|---------|--------|
| `read = "Perm"` | Read permission | `#[permission(read = "UserRead")]` | 🔍 Check before reading |
| `write = "Perm"` | Write permission | `#[permission(write = "UserWrite")]` | ✏️ Check before writing |
| `owner_field` | Ownership-based | `#[rbac(owner_field)]` | 👤 Only owner accesses |
| `role_based` | Role-based | `#[rbac(role_based)]` | 🎭 According to user role |

### 📝 **Detailed Examples**

```rust
#[derive(DeclarativeModel)]
pub struct Article {
    #[permission(read = "ArticleReadAny")]
    pub id: Uuid,                          // 🔍 All with permission
    
    #[permission(read = "ArticleReadAny", write = "ArticleWriteAny")]
    pub title: String,                     // 🔍 + ✏️ Different permissions
    
    #[rbac(owner_field)]                  // 👤 Only the author
    #[permission(write = "ArticleEditOwn")]
    pub content: String,
    
    #[permission(write = "AdminOnly")]    // ✏️ Admins only
    pub featured: bool,
    
    // Fields without attributes = free access according to parent model
    pub created_at: DateTime<Utc>,
}

#[derive(DeclarativeModel)]
pub struct User {
    #[permission(read = "UserReadAny")]
    pub username: String,                  // 🔍 Free reading
    
    #[rbac(owner_field)]                  // 👤 User or admin
    #[permission(read = "UserReadOwn", write = "UserWriteOwn")]
    pub email: String,
    
    #[rbac(owner_field)]
    #[permission(read = "UserReadOwn", write = "UserWriteOwn")]
    pub private_notes: String,            // 👤 Strictly personal
    
    #[permission(read = "AdminOnly", write = "AdminOnly")]
    pub admin_flags: AdminFlags,          // 🔐 Admin only
}
```

### ⚙️ **Automatic Checks**

```rust
// Lithair automatically generates:

// 1. Permission middleware on each route
GET /articles/{id}
// → Checks "ArticleReadAny" for title
// → Checks ownership for content if owner_field

// 2. Field filtering according to permissions
{
    "id": "uuid-here",
    "title": "Visible Title",
    // content omitted because user is not owner
    "created_at": "2024-01-15T10:00:00Z"
}

// 3. Explicit permission errors
PUT /articles/{id}
{
    "admin_flags": {"featured": true}  // ← Normal user
}
// Returns: 403 Forbidden - Insufficient permissions for field 'admin_flags'

// 4. Optimized queries with automatic filters
SELECT * FROM articles 
WHERE id = $1 
AND (author_id = $2 OR $3 IN (SELECT permission FROM user_permissions WHERE user_id = $2))
```

---

## 🎭 **RBAC Attributes (`#[rbac(...)]`)**

Role-based and ownership-based access control.

### 📋 **RBAC Options**

| Attribute | Description | Example | Behavior |
|----------|-------------|---------|-------------|
| `owner_field` | Data ownership | `#[rbac(owner_field)]` | Only owner + admins |
| `role_based` | Role-based | `#[rbac(role_based)]` | According to role hierarchy |

### 📝 **Complex Examples**

```rust
#[derive(DeclarativeModel)]
pub struct Document {
    #[rbac(owner_field)]
    pub author_id: Uuid,                  // 👤 Owner field
    
    #[rbac(owner_field)]                  // 👤 Only author can modify
    #[permission(write = "DocumentEdit")]
    pub content: String,
    
    #[rbac(role_based)]                   // 🎭 According to hierarchical role  
    #[permission(read = "DocumentModerate")]
    pub moderation_notes: String,
    
    // Public access (no RBAC attribute)
    pub title: String,
}

// Role configuration (in application)
impl RbacConfig for MyApp {
    fn role_hierarchy() -> Vec<(Role, Vec<Role>)> {
        vec![
            (Role::Admin, vec![Role::Moderator, Role::User]),
            (Role::Moderator, vec![Role::User]),
            (Role::User, vec![]),
        ]
    }
    
    fn owner_field_mapping() -> HashMap<&'static str, &'static str> {
        hashmap! {
            "Document" => "author_id",
            "Comment" => "user_id", 
            "Order" => "customer_id",
        }
    }
}
```

---

## 🎨 **Advanced Combinations**

### 🏢 **Enterprise Model**
```rust
#[derive(DeclarativeModel)]
pub struct FinancialRecord {
    #[db(primary_key)]
    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(replicate, track_history)]
    #[permission(read = "FinanceRead")]
    pub id: Uuid,
    
    #[db(indexed)]
    #[lifecycle(audited, retention = 2555)]  // 7 years legal
    #[http(expose, validate = "range(0.01, 999999999.99)")]
    #[persistence(replicate, track_history)]
    #[permission(read = "FinanceRead", write = "FinanceWrite")]
    pub amount: f64,
    
    #[db(fk = "User")]
    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(replicate)]
    #[rbac(owner_field)]
    pub created_by: Uuid,
    
    #[lifecycle(audited, versioned = 10)]
    #[http(expose = false)]               // Internal only
    #[persistence(memory_only)]           // Calculation cache
    #[permission(read = "InternalOnly")]
    pub risk_metadata: serde_json::Value,
}
```

### 🚀 **Performance Model**
```rust
#[derive(DeclarativeModel)]  
pub struct HighFrequencyEvent {
    #[db(primary_key)]
    #[persistence(memory_only)]           // ⚡ Ultra fast
    pub id: Uuid,
    
    #[db(indexed)]
    #[lifecycle(snapshot_only)]           // 📸 No history
    #[persistence(memory_only)]
    pub event_type: String,
    
    #[lifecycle(retention = 1)]           // 🗑️ 1 day only
    #[persistence(auto_persist)]          // 💾 Batch writes
    pub metrics: HashMap<String, f64>,
    
    // Deferred replication for performance
    #[persistence(replicate)]             // 🌐 Async replication
    pub summary_data: EventSummary,
}
```

### 🔐 **Secure Model**
```rust
#[derive(DeclarativeModel)]
pub struct SecureUserData {
    #[db(primary_key)]
    #[lifecycle(immutable)]
    #[permission(read = "UserReadOwn")]
    #[rbac(owner_field)]
    pub user_id: Uuid,
    
    #[lifecycle(audited, retention = 365)]
    #[http(expose, validate = "email")]
    #[persistence(replicate, track_history)]
    #[permission(read = "UserReadOwn", write = "UserWriteOwn")]
    #[rbac(owner_field)]
    pub email: String,
    
    #[http(expose = false)]               // 🚫 Never exposed
    #[lifecycle(audited, retention = 90)] 
    #[persistence(replicate, track_history)]
    #[permission(read = "SecurityAudit", write = "SecurityAdmin")]
    pub encrypted_personal_data: Vec<u8>,
    
    #[persistence(memory_only)]           // ⚡ Session only
    #[permission(read = "UserReadOwn")]
    #[rbac(owner_field)]
    pub temp_preferences: serde_json::Value,
}
```

---

## 🔧 **Automatic Generation**

### 📊 **What Lithair generates for you:**

```rust
// From your declarative attributes, Lithair generates:

// 1. 🗄️ Optimized database schema
CREATE TABLE products (
    id UUID PRIMARY KEY,
    name VARCHAR(200) NOT NULL,
    price DECIMAL(10,2) CHECK (price >= 0.01 AND price <= 999999.99),
    sku VARCHAR(10) UNIQUE CHECK (sku ~ '^[A-Z]{3}-[0-9]{4}$')
);
CREATE INDEX idx_products_sku ON products(sku);

// 2. 🌐 Complete REST API with validation
POST   /products          // Automatic validation
GET    /products          // Pagination, filters
GET    /products/{id}     // Permissions verified 
PUT    /products/{id}     // Validation + permissions
DELETE /products/{id}     // Soft delete if audited
GET    /products/{id}/history  // History if lifecycle(audited)

// 3. 🔒 Security middleware
fn check_permissions(user: &User, operation: Operation, resource: &Resource) {
    // Automatically checks all #[permission] and #[rbac] attributes
}

// 4. 💾 Optimized persistence engines  
let engines = create_engines_from_attributes(&model_spec);
// Automatically chooses Memory/SCC2/Raft according to #[persistence] attributes

// 5. 📝 Automatic audit trail
fn track_changes<T: DeclarativeModel>(old: &T, new: &T, user: UserId) {
    // Automatically compares #[lifecycle(audited)] fields
    // Generates history events
}

// 6. ✅ Integrated validation
fn validate_product(product: &Product) -> Result<(), ValidationErrors> {
    // Automatically applies all #[http(validate)] rules
}
```

## 🎯 **Summary: The Power of Declarative**

### **One attribute line = Hundreds of generated lines**

```rust
#[lifecycle(audited, retention = 365)]
pub title: String,
```

**Automatically generates:**
- 📄 Audit table with appropriate columns
- 🔧 Triggers to capture changes  
- 🌐 API `/resource/{id}/history` for consultation
- 🗑️ Cleanup task after 365 days
- ✅ Retention rule validation
- 🔒 Permission checks for history access
- ⚡ Query optimizations with appropriate indexes

**Impact:** **1 attribute** → **~200 lines of equivalent code** in traditional approach

### **Complete Mental Shift**

❌ **Before:** "How to implement history?"
✅ **Now:** "Does this data need history?"

❌ **Before:** "What permissions for this API?"  
✅ **Now:** "Who can read/modify this data?"

❌ **Before:** "How to optimize this query?"
✅ **Now:** "Is this data often searched?"

**Lithair transforms implementation questions into business modeling questions.** 🚀