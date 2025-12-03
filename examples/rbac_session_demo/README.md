# RBAC with Session Management Demo

This example demonstrates Lithair's integrated RBAC and Session management:

## 🎯 Features

- **Password Authentication** - Simple username/password login
- **Session Management** - Login once, get a token, reuse it
- **Hybrid Auth** - Supports both Cookie and Bearer token
- **Role-Based Permissions** - Customer, Employee, Administrator
- **Persistent Sessions** - Event sourcing with .raftlog files using EventStore

## 🚀 Quick Start

### Option 1: Web UI (Recommended)

```bash
# Start server with frontend
task examples:rbac-session:frontend

# Then open in browser
open http://localhost:3000
```

### Option 2: API Only

```bash
# From project root
task examples:rbac-session

# Or manually
cargo run -p rbac_session_demo
```

## 📖 Usage Flow

### 1. Login

```bash
# Login as Customer
curl -X POST http://localhost:3000/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"alice","password":"password123"}'

# Response:
# {
#   "session_token": "abc123...",
#   "role": "Customer",
#   "expires_in": 3600
# }
```

### 2. Use Session Token

```bash
# Save the token
TOKEN="abc123..."

# List products (any role)
curl http://localhost:3000/api/products \
  -H "Authorization: Bearer $TOKEN"

# Create product (Employee or Admin only)
curl -X POST http://localhost:3000/api/products \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"name":"New Product","price":99.99}'

# Delete product (Admin only)
curl -X DELETE http://localhost:3000/api/products/123 \
  -H "Authorization: Bearer $TOKEN"
```

### 3. Cookie-Based (Alternative)

The session token can also be sent as a cookie:

```bash
curl http://localhost:3000/api/products \
  -H "Cookie: session_id=$TOKEN"
```

## 👥 Demo Users

| Username | Password     | Role           | Permissions                    |
|----------|--------------|----------------|--------------------------------|
| alice    | password123  | Customer       | Read products                  |
| bob      | password123  | Employee       | Read + Create products         |
| admin    | password123  | Administrator  | All permissions                |

## 🔒 Permission Matrix

| Operation      | Customer | Employee | Administrator |
|----------------|----------|----------|---------------|
| List products  | ✅       | ✅       | ✅            |
| Create product | ❌       | ✅       | ✅            |
| Delete product | ❌       | ❌       | ✅            |

## 🎨 Architecture

```
┌─────────────┐
│   Client    │
└──────┬──────┘
       │ 1. POST /auth/login
       │    {username, password}
       ▼
┌─────────────────────┐
│  Session Middleware │
│  - Validates creds  │
│  - Creates session  │
│  - Returns token    │
└──────┬──────────────┘
       │ 2. session_token
       ▼
┌─────────────┐
│   Client    │ Stores token
└──────┬──────┘
       │ 3. GET /api/products
       │    Authorization: Bearer <token>
       ▼
┌─────────────────────┐
│  Session Middleware │
│  - Extracts token   │
│  - Loads session    │
│  - Checks role      │
└──────┬──────────────┘
       │ 4. Authorized request
       ▼
┌─────────────┐
│   Handler   │
└─────────────┘
```

## 🧪 Testing

```bash
# Run automated tests with all scenarios
task examples:rbac-session:test

# Or run manually
task examples:rbac-session
```

The automated test validates:
- ✅ Login with session token creation
- ✅ API access with Bearer token authentication
- ✅ Session persistence with event sourcing (.raftlog files)
- ✅ Logout and session cleanup

## 💡 Key Concepts

### Session Token
- **Cryptographically secure** UUID
- **Stored server-side** in PersistentSessionStore (EventStore with .raftlog files)
- **Event sourced** - Every session change is an immutable event
- **Automatic replay** - Events replayed on restart for perfect state reconstruction
- **Expires** after 1 hour
- **Contains** user_id and role

### Hybrid Authentication
- **Cookie**: Automatic, browser-friendly
- **Bearer**: Explicit, API-friendly
- **Priority**: Cookie checked first, then Bearer

### RBAC Integration
- Sessions store the user's role
- Each endpoint checks required permissions
- 401 Unauthorized if no session
- 403 Forbidden if insufficient permissions

## 🔐 Security Features

- ✅ HttpOnly cookies (XSS protection)
- ✅ Secure flag (HTTPS only in production)
- ✅ SameSite=Lax (CSRF protection)
- ✅ Session expiration (1 hour)
- ✅ Cryptographically secure session IDs
- ✅ Role-based access control

## 🎨 Web Frontend

This example includes a **modern web UI** demonstrating Lithair's frontend capabilities:

### Features

- **🔐 Visual Login** - Quick login with demo users
- **📊 RBAC Dashboard** - Real-time permission display
- **💾 Frontend Caching** - Products cached for 30 seconds
- **📝 Activity Log** - Live activity tracking
- **⚡ Real-time Updates** - Auto-refresh on changes
- **🎯 Permission-Based UI** - Buttons disabled based on role

### Frontend Architecture

```
┌─────────────────────────────────────────────────────┐
│  Browser (index.html + app.js)                      │
│  ┌─────────────────────────────────────────────┐   │
│  │ State Management                             │   │
│  │ - token, role, username                      │   │
│  │ - products (cached with TTL)                 │   │
│  │ - permissions matrix                         │   │
│  └─────────────────────────────────────────────┘   │
│                                                      │
│  ┌─────────────────────────────────────────────┐   │
│  │ API Client with Smart Caching                │   │
│  │ - Cache products (30s TTL)                   │   │
│  │ - Auto-invalidate on mutations               │   │
│  │ - Bearer token injection                     │   │
│  └─────────────────────────────────────────────┘   │
└──────────────────┬──────────────────────────────────┘
                   │ HTTP + JSON
                   ▼
┌─────────────────────────────────────────────────────┐
│  Lithair Server (http://localhost:3000)           │
│  - Static files: /, /frontend/**                    │
│  - Auth: POST /auth/login, /auth/logout             │
│  - CRUD: /api/products (with RBAC)                  │
└─────────────────────────────────────────────────────┘
```

### Cache System

The frontend implements **intelligent caching**:

```javascript
// Cache configuration
cache: {
    products: null,      // Cached data
    timestamp: null,     // When cached
    ttl: 30000          // 30 seconds cache
}

// Cache logic
if (useCache && cacheAge < ttl) {
    return cachedProducts;  // ⚡ Fast
} else {
    fetchFromServer();      // 🌐 Refresh
}
```

**Cache invalidation**:
- ✅ Automatic on CREATE/DELETE operations
- ✅ Manual refresh button
- ✅ Auto-refresh every 30 seconds
- ✅ Cache status indicator

### Using the Web UI

1. **Start server**:
   ```bash
   task examples:rbac-session:frontend
   ```

2. **Open browser**: `http://localhost:3000`

3. **Quick login** - Click on a demo user card:
   - **Alice (Customer)**: Can only READ products
   - **Bob (Employee)**: Can READ + CREATE products
   - **Admin**: Full access (READ + CREATE + DELETE)

4. **Observe RBAC**:
   - Permissions card shows your access level
   - Create button hidden for Customers
   - Delete buttons disabled for non-Admins
   - Activity log shows permission denials

5. **Test caching**:
   - Products load from cache (💾 icon)
   - Create product → cache auto-invalidates
   - Cache status shows age
   - Manual refresh available

### Files

```
frontend/
├── index.html          # Main UI (login + dashboard)
├── css/
│   └── styles.css      # Modern responsive design
└── js/
    └── app.js          # State + API + Cache + RBAC logic
```

## 📚 Learn More

- [Lithair Session Documentation](../../docs/sessions.md)
- [RBAC Guide](../../docs/rbac.md)
- [Security Best Practices](../../docs/security.md)
