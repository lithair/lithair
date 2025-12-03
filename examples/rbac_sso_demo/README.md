# 🔐 Lithair RBAC + SSO Demo

**Feature:** Enterprise-grade Role-Based Access Control with Multi-Provider SSO

This example demonstrates Lithair's **declarative approach** to authentication and authorization, showing how to build a secure API with:
- Field-level permissions
- Route-level RBAC
- Multiple identity providers (Google, GitHub, Microsoft, Local JWT)
- Custom middleware (admin password, IP whitelist, rate limiting)

---

## 🎯 What This Example Demonstrates

### 1. Declarative RBAC
```rust
#[derive(DeclarativeModel)]
pub struct Product {
    #[permission(read = "Public")]
    pub id: Uuid,
    
    #[permission(read = "Public", write = "ProductManager")]
    pub name: String,
    
    #[permission(read = "StockManager", write = "StockManager")]
    pub stock: i32,  // Only visible to StockManager role
}
```

### 2. Multi-Provider SSO
```rust
#[derive(IdentityProvider)]
pub enum AuthProvider {
    #[idp(provider = "google", scopes = ["email", "profile"])]
    Google,
    
    #[idp(provider = "github", scopes = ["user:email"])]
    GitHub,
    
    #[idp(provider = "microsoft")]
    Microsoft,
    
    #[idp(provider = "jwt")]
    LocalJWT,
}
```

### 3. Route Protection
```rust
#[http_route(GET, "/api/products")]
#[permission(read = "Public")]
#[auth(providers = [Google, GitHub, LocalJWT])]
async fn list_products() { }

#[http_route(DELETE, "/api/products/{id}")]
#[permission(delete = "Administrator")]
#[auth(providers = [Google, Microsoft])]  // Admin via SSO only
#[middleware(ip_whitelist)]
async fn delete_product() { }
```

### 4. Custom Middleware
```rust
#[middleware(path = "/admin/*")]
async fn admin_password_protection(req: Request) -> Result<(), Response> {
    // Additional password protection for admin routes
}
```

---

## 🚀 Quick Start

### Option 1: Full Automated Demo (Recommended)

```bash
# Run complete demo: start server + seed data + test RBAC
task examples:rbac:full

# This will:
# 1. Clean old data
# 2. Start server in background
# 3. Seed 3 sample products
# 4. Test authentication with different roles
# 5. Stop server automatically
```

### Option 2: Manual Step-by-Step

```bash
# Step 1: Start the server
task examples:rbac PORT=18321

# Step 2: In another terminal, seed products
task examples:rbac:seed PORT=18321

# Step 3: Test RBAC authentication
task examples:rbac:test PORT=18321
```

### Option 3: Direct Cargo Run

```bash
cd examples/rbac_sso_demo
cargo run -- --port 8080
```

### Available Taskfile Commands

| Command | Description |
|---------|-------------|
| `task examples:rbac` | Start RBAC server (interactive) |
| `task examples:rbac:seed` | Seed sample products via API |
| `task examples:rbac:test` | Test password authentication |
| `task examples:rbac:full` | Complete automated demo |

### 3. Test Authentication

```bash
# Login with Google SSO
curl http://localhost:8080/auth/google/login
# → Redirects to Google OAuth consent screen

# Login with GitHub SSO
curl http://localhost:8080/auth/github/login
# → Redirects to GitHub OAuth

# Login with JWT (local)
curl -X POST http://localhost:8080/auth/jwt/login \
  -H "Content-Type: application/json" \
  -d '{"email": "admin@example.com", "password": "admin123"}'
# → Returns JWT token
```

### 4. Test RBAC

```bash
# Public endpoint (no auth required)
curl http://localhost:8080/api/products

# Protected endpoint (requires authentication)
curl http://localhost:8080/api/products \
  -H "Authorization: Bearer <your_jwt_token>"

# Admin endpoint (requires Administrator role + admin password)
curl http://localhost:8080/admin/dashboard \
  -H "Authorization: Bearer <admin_token>" \
  -H "X-Admin-Password: secret_admin_pass"

# Delete product (requires Administrator role + whitelisted IP)
curl -X DELETE http://localhost:8080/api/products/123 \
  -H "Authorization: Bearer <admin_token>"
```

---

## 📋 User Roles & Permissions

| Role | Permissions | Description |
|------|-------------|-------------|
| **Customer** | `ProductRead` | Can view products (without stock info) |
| **Employee** | `ProductRead`, `ProductWrite` | Can view and create products |
| **StockManager** | `ProductRead`, `StockRead`, `StockWrite` | Can manage inventory |
| **ProductManager** | `ProductRead`, `ProductWrite`, `StockRead` | Can manage products |
| **Administrator** | All permissions | Full access + admin dashboard |

---

## 🔐 Authentication Flow

### SSO Flow (Google/GitHub/Microsoft)

```
1. User clicks "Login with Google"
   ↓
2. GET /auth/google/login
   ↓ (Lithair redirects to Google)
3. Google OAuth consent screen
   ↓ (User approves)
4. Google redirects to /auth/google/callback?code=xxx
   ↓ (Lithair exchanges code for user info)
5. Lithair creates/updates User in database
   ↓
6. Lithair generates internal JWT token
   ↓
7. Returns JWT to client
   ↓
8. Client uses JWT for subsequent requests
```

### JWT Flow (Local)

```
1. POST /auth/jwt/login with {email, password}
   ↓
2. Lithair validates credentials
   ↓
3. Lithair generates JWT token
   ↓
4. Returns JWT to client
```

---

## 🎨 Frontend Integration

### HTML Login Buttons

```html
<!DOCTYPE html>
<html>
<head>
    <title>Lithair RBAC Demo</title>
</head>
<body>
    <h1>Login Options</h1>
    
    <!-- SSO Buttons (auto-generated by Lithair) -->
    <button onclick="window.location='/auth/google/login'">
        🔵 Login with Google
    </button>
    <button onclick="window.location='/auth/github/login'">
        ⚫ Login with GitHub
    </button>
    <button onclick="window.location='/auth/microsoft/login'">
        🔷 Login with Microsoft
    </button>
    
    <!-- Local JWT Login -->
    <form id="jwt-login">
        <input type="email" name="email" placeholder="Email" required>
        <input type="password" name="password" placeholder="Password" required>
        <button type="submit">🔑 Login with Email</button>
    </form>
</body>
</html>
```

### JavaScript API Calls

```javascript
// Store JWT token
const token = localStorage.getItem('jwt_token');

// Authenticated API call
fetch('/api/products', {
    headers: {
        'Authorization': `Bearer ${token}`
    }
})
.then(res => res.json())
.then(products => {
    // Customer sees: {id, name, price}
    // Manager sees: {id, name, price, stock}
    console.log(products);
});
```

---

## 🛡️ Security Features

### 1. Field-Level Permissions
Different users see different fields based on their role:

```json
// Customer view
{
  "id": "123",
  "name": "Product A",
  "price": 99.99
}

// StockManager view
{
  "id": "123",
  "name": "Product A",
  "price": 99.99,
  "stock": 50  // ← Only visible to StockManager
}
```

### 2. Route-Level Protection
Routes are protected by role:

| Endpoint | Public | Customer | Manager | Admin |
|----------|--------|----------|---------|-------|
| `GET /api/products` | ✅ | ✅ | ✅ | ✅ |
| `POST /api/products` | ❌ | ❌ | ✅ | ✅ |
| `DELETE /api/products/{id}` | ❌ | ❌ | ❌ | ✅ |
| `GET /admin/dashboard` | ❌ | ❌ | ❌ | ✅* |

*Requires admin password

### 3. Custom Middleware
Additional protection layers:

- **Admin Password**: `/admin/*` routes require extra password
- **IP Whitelist**: DELETE operations only from whitelisted IPs
- **Rate Limiting**: Per-user request limits

---

## 🔧 Adding New Identity Providers

### Step 1: Add Provider to Enum

```rust
// src/auth/providers.rs
#[derive(IdentityProvider)]
pub enum AuthProvider {
    // ... existing providers ...
    
    #[idp(
        provider = "okta",
        domain = env("OKTA_DOMAIN"),
        client_id = env("OKTA_CLIENT_ID"),
        client_secret = env("OKTA_CLIENT_SECRET"),
        redirect_uri = "/auth/okta/callback"
    )]
    Okta,
}
```

### Step 2: Add Credentials to .env

```bash
OKTA_DOMAIN=your-domain.okta.com
OKTA_CLIENT_ID=your_client_id
OKTA_CLIENT_SECRET=your_client_secret
```

### Step 3: Done! 🎉

Lithair automatically generates:
- `GET /auth/okta/login` - Login endpoint
- `GET /auth/okta/callback` - OAuth callback handler
- Token validation logic
- User mapping

---

## 📊 API Endpoints

### Authentication

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/auth/google/login` | Initiate Google SSO |
| `GET` | `/auth/google/callback` | Google OAuth callback |
| `GET` | `/auth/github/login` | Initiate GitHub SSO |
| `GET` | `/auth/github/callback` | GitHub OAuth callback |
| `GET` | `/auth/microsoft/login` | Initiate Microsoft SSO |
| `GET` | `/auth/microsoft/callback` | Microsoft OAuth callback |
| `POST` | `/auth/jwt/login` | Local JWT login |
| `POST` | `/auth/logout` | Logout (invalidate token) |

### Products API

| Method | Endpoint | Permission | Description |
|--------|----------|------------|-------------|
| `GET` | `/api/products` | Public | List all products |
| `GET` | `/api/products/{id}` | Public | Get product by ID |
| `POST` | `/api/products` | ProductManager | Create product |
| `PUT` | `/api/products/{id}` | ProductManager | Update product |
| `DELETE` | `/api/products/{id}` | Administrator | Delete product |

### Admin

| Method | Endpoint | Permission | Extra Protection |
|--------|----------|------------|------------------|
| `GET` | `/admin/dashboard` | Administrator | Admin password |
| `GET` | `/admin/users` | Administrator | Admin password |
| `POST` | `/admin/users/{id}/role` | Administrator | Admin password + IP whitelist |

---

## 🧪 Testing

### Run Tests

```bash
cargo test
```

### Manual Testing Scenarios

#### Scenario 1: Customer Access
```bash
# Login as customer
TOKEN=$(curl -X POST http://localhost:8080/auth/jwt/login \
  -H "Content-Type: application/json" \
  -d '{"email": "customer@example.com", "password": "customer123"}' \
  | jq -r '.token')

# View products (should see id, name, price only)
curl http://localhost:8080/api/products \
  -H "Authorization: Bearer $TOKEN"

# Try to create product (should fail with 403)
curl -X POST http://localhost:8080/api/products \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name": "New Product", "price": 49.99}'
```

#### Scenario 2: Manager Access
```bash
# Login as manager
TOKEN=$(curl -X POST http://localhost:8080/auth/jwt/login \
  -H "Content-Type: application/json" \
  -d '{"email": "manager@example.com", "password": "manager123"}' \
  | jq -r '.token')

# View products (should see all fields including stock)
curl http://localhost:8080/api/products \
  -H "Authorization: Bearer $TOKEN"

# Create product (should succeed)
curl -X POST http://localhost:8080/api/products \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name": "New Product", "price": 49.99, "stock": 100}'
```

#### Scenario 3: Admin Access
```bash
# Login as admin
TOKEN=$(curl -X POST http://localhost:8080/auth/jwt/login \
  -H "Content-Type: application/json" \
  -d '{"email": "admin@example.com", "password": "admin123"}' \
  | jq -r '.token')

# Access admin dashboard (requires admin password)
curl http://localhost:8080/admin/dashboard \
  -H "Authorization: Bearer $TOKEN" \
  -H "X-Admin-Password: secret_admin_pass"

# Delete product (requires admin role + whitelisted IP)
curl -X DELETE http://localhost:8080/api/products/123 \
  -H "Authorization: Bearer $TOKEN"
```

---

## 📁 Project Structure

```
rbac_sso_demo/
├── Cargo.toml
├── README.md
├── .env.example
└── src/
    ├── main.rs                 # Entry point
    ├── models/
    │   ├── mod.rs
    │   ├── product.rs          # Product model with #[permission]
    │   └── user.rs             # User model with #[auth]
    ├── auth/
    │   ├── mod.rs
    │   ├── providers.rs        # AuthProvider enum
    │   ├── permissions.rs      # Permission definitions
    │   └── roles.rs            # Role definitions
    ├── middleware/
    │   ├── mod.rs
    │   ├── admin_password.rs   # Admin password middleware
    │   ├── ip_whitelist.rs     # IP whitelist middleware
    │   └── rate_limit.rs       # Rate limiting middleware
    └── routes/
        ├── mod.rs
        ├── products.rs         # Product routes with RBAC
        └── admin.rs            # Admin routes with extra protection
```

---

## 🎯 Key Takeaways

1. **Declarative = Readable**: Permissions are defined where they belong (on fields and routes)
2. **Type-Safe**: Compiler validates all permissions at build time
3. **Flexible**: Mix RBAC with custom middleware for complex requirements
4. **Scalable**: Add new providers in 3 lines, no boilerplate
5. **Secure by Default**: All routes require explicit permission declarations

---

## 🚀 Next Steps

- Try adding a new identity provider (Okta, Auth0, etc.)
- Implement custom permissions for your use case
- Add field-level encryption for sensitive data
- Integrate with your existing user database

---

## 📚 Related Documentation

- [RBAC Architecture](../../docs/security/rbac.md)
- [Identity Providers](../../docs/security/identity-providers.md)
- [Custom Middleware](../../docs/guides/custom-middleware.md)
- [Declarative Attributes](../../docs/reference/declarative-attributes.md)

---

**Built with Lithair** - Declarative, Type-Safe, Production-Ready
