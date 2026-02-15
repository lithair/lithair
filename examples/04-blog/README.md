# Lithair Blog - Fully Declarative Example

This example demonstrates Lithair's **fully declarative approach** for building a complete blog platform with enterprise-grade features.

## 🎯 Key Features

### ✨ Declarative Architecture
- **Single `LithairServer`** - Unified API instead of manual routing
- **`DeclarativeModel` macro** - Automatic CRUD generation with `#[http]` and `#[permission]` attributes
- **Event-sourced sessions** - `PersistentSessionStore` with automatic persistence
- **SCC2 Frontend Engine** - Static assets served from memory at 40M+ ops/sec
- **Built-in admin panel** - Zero configuration required

### 🔐 RBAC System
- **4 roles**: Anonymous, Contributor, Reporter, Admin
- **Granular permissions**: ArticleRead, ArticleWrite, ArticlePublish, ArticleDelete
- **Automatic enforcement** on all CRUD operations
- **Session-based authentication** with JWT-like tokens

### 📊 What's Declarative vs Manual

| Feature | Before (Manual) | After (Declarative) |
|---------|----------------|---------------------|
| **Servers** | 2 separate (3001 + 3002) | 1 unified (3000) |
| **CRUD endpoints** | Manual handlers | `.with_model_full::<Article>()` |
| **RBAC setup** | 132 lines of seeding | 96 lines `BlogPermissionChecker` |
| **Sessions** | Not implemented | `PersistentSessionStore` built-in |
| **Frontend** | Not implemented | `FrontendEngine` (SCC2) |
| **Routes** | 15+ manual routes | 2 custom (login/logout) |
| **Code lines** | 305 imperative | 320 declarative |

## 🚀 Running the Example

```bash
# From the Lithair root directory
cargo run -p blog

# Or with custom port
cargo run -p blog -- --port 3001
```

The server will start on `http://localhost:3000` by default.

## 📚 API Endpoints

### Authentication
- `POST /auth/login` - Login with username/password
- `POST /auth/logout` - Logout and destroy session

### Articles (Auto-generated CRUD)
- `GET /api/articles` - List all articles
- `POST /api/articles` - Create article
- `GET /api/articles/:id` - Get article by ID
- `PUT /api/articles/:id` - Update article
- `DELETE /api/articles/:id` - Delete article

### Admin
- `GET /admin/*` - Admin panel (built-in)

## 👥 Demo Users

| Username | Password | Role | Permissions |
|----------|----------|------|-------------|
| `admin` | `password123` | Admin | All operations including delete |
| `reporter` | `password123` | Reporter | Read + Write + Publish |
| `contributor` | `password123` | Contributor | Read + Write (own articles) |

## 🧪 Testing the API

### 1. Login
```bash
curl -X POST http://localhost:3000/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username": "admin", "password": "password123"}'

# Response:
# {"session_token": "uuid...", "role": "Admin", "expires_in": 3600}
```

### 2. Create Article
```bash
curl -X POST http://localhost:3000/api/articles \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <session_token>" \
  -d '{
    "id": "article-1",
    "title": "My First Article",
    "content": "This is declarative!",
    "author_id": "admin",
    "status": "Draft"
  }'
```

### 3. List Articles
```bash
curl http://localhost:3000/api/articles \
  -H "Authorization: Bearer <session_token>"
```

### 4. Logout
```bash
curl -X POST http://localhost:3000/auth/logout \
  -H "Authorization: Bearer <session_token>"
```

## 🏗️ Code Structure

```rust
// 1. Define your model with DeclarativeModel
#[derive(DeclarativeModel)]
struct Article {
    #[http(expose)]
    #[permission(read = "ArticleRead", write = "ArticleWrite")]
    id: String,
    // ... other fields
}

// 2. Implement simple permission checker
impl PermissionChecker for BlogPermissionChecker {
    fn has_permission(&self, role: &str, permission: &str) -> bool {
        // Simple match logic
    }
}

// 3. Start server with one-liner
LithairServer::new()
    .with_model_full::<Article>("./data", "/api/articles", Some(checker), Some(sessions))
    .with_admin_panel(true)
    .serve()
    .await?;
```

**That's it!** You get:
- ✅ Full CRUD API with validation
- ✅ RBAC enforcement on all operations
- ✅ Event sourcing with persistence
- ✅ Session management
- ✅ Admin panel
- ✅ Frontend serving (if you add assets)

## 📁 Data Storage

All data is stored using Lithair's event sourcing:

```
./data/blog/
├── sessions/           # Persistent sessions (event-sourced)
│   ├── events.raftlog
│   └── state.raftsnap
├── articles/           # Article events (auto-generated)
│   ├── events.raftlog
│   └── state.raftsnap
└── frontend/           # Frontend assets cache (SCC2)
    └── ...
```

## 🎨 Adding a Frontend

Simply create a `frontend/` directory with your HTML/CSS/JS:

```bash
mkdir -p examples/04-blog/frontend
echo '<h1>My Blog</h1>' > examples/04-blog/frontend/index.html
```

The `FrontendEngine` will automatically:
- Load all assets into SCC2 memory
- Serve them at 40M+ ops/sec
- Support hot reloading (optional)

## 📊 Performance

- **CRUD operations**: Event-sourced with sub-millisecond latency
- **Frontend serving**: 40M+ requests/sec (SCC2 lock-free)
- **Sessions**: Persistent with automatic cleanup
- **Memory**: ~12MB for 10MB dataset (vs 25MB SQLite)

## 🔍 What Makes This Declarative?

1. **No manual routing** - `DeclarativeModel` generates all routes
2. **No manual handlers** - CRUD operations auto-generated
3. **No manual RBAC setup** - Just implement `PermissionChecker` trait
4. **No manual session management** - `PersistentSessionStore` handles it
5. **No manual frontend setup** - `FrontendEngine` loads and serves assets
6. **Single source of truth** - Model attributes drive everything

## 🆚 Comparison: Before vs After

### Before (Manual Approach)
```rust
// Manual router construction
let mut router = Router::new();
router.get("/api/articles", list_articles);
router.post("/api/articles", create_article);
router.get("/api/articles/:id", get_article);
// ... 10+ more routes

// Manual RBAC seeding (132 lines)
let mut admin_perms = HashSet::new();
admin_perms.insert(Permission::ArticleRead);
// ... repeat for all permissions/roles

// 2 separate servers
std::thread::spawn(|| server1.serve("127.0.0.1:3001"));
server2.serve("127.0.0.1:3002").await?;
```

### After (Declarative Approach)
```rust
// Everything in one place
LithairServer::new()
    .with_model_full::<Article>(
        "./data/articles",
        "/api/articles",
        Some(permission_checker),
        Some(session_store),
    )
    .with_admin_panel(true)
    .serve()
    .await?;
```

**305 lines → 320 lines** but with:
- ✅ Sessions (not in before)
- ✅ Frontend serving (not in before)
- ✅ Admin panel (not in before)
- ✅ Better organization
- ✅ Much more maintainable

## 📖 Learn More

- See `examples/06-auth-sessions` for more RBAC patterns
- See Lithair docs for `DeclarativeModel` macro details
- See `lithair-core/src/app.rs` for `LithairServer` API

## 🎯 Next Steps

To extend this example:

1. **Add more models**: Create `Comment`, `Tag`, etc. with `DeclarativeModel`
2. **Add frontend**: Create HTML/CSS/JS in `frontend/` directory
3. **Add relationships**: Use `#[foreign_key]` attributes
4. **Add validation**: Use `#[validate]` attributes
5. **Add indexes**: Use `#[db(indexed)]` attributes

All of these are declarative - just add attributes!
