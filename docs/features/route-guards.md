# Route Guards - Declarative Route Protection

## 🎯 Philosophy

Following Lithair's **90% Rule**, route guards provide declarative protection for common scenarios:

- ✅ Authentication checks (`RequireAuth`)
- ✅ Role-based access (`RequireRole`)

**Zero boilerplate for the common cases.**

## 🚀 Quick Start

### Basic Authentication Protection

```rust
use lithair_core::http::RouteGuard;

LithairServer::new()
    .with_rbac_config(rbac_config)
    .with_route_guard("/admin/*", RouteGuard::RequireAuth {
        redirect_to: Some("/admin/login/".to_string()),
        exclude: vec!["/admin/login/".to_string()],
    })
    .with_frontend("public")
    .serve()
    .await?;
```

**That's it!** No custom middleware, no manual token validation, no boilerplate.

## 📋 Available Guards

### 1. RequireAuth - Session-Based Authentication

Validates session tokens from `Authorization` header or cookies.

```rust
RouteGuard::RequireAuth {
    redirect_to: Some("/login".to_string()),  // Redirect URL (None = 401 JSON)
    exclude: vec!["/login", "/public/*"],      // Paths to exclude
}
```

**Use cases:**

- Admin panels
- User dashboards
- Protected content areas

### 2. RequireRole - Role-Based Access

Requires the session's role to be in the allowed set. The role is read from
`session.data["role"]` — the value your login handler sets via
`session.set("role", …)`. A matching role passes; anything else (wrong role or
no session) gets `403`.

```rust
RouteGuard::RequireRole {
    roles: vec!["Admin".to_string(), "Manager".to_string()],
    redirect_to: Some("/unauthorized".to_string()),  // None = 403 JSON
}
```

**Use cases:**

- Admin-only sections
- Role-scoped admin planes (see `with_admin_roles`)
- Role-specific features

## 🔧 Advanced Usage

### Multiple Guards on Different Paths

```rust
LithairServer::new()
    .with_rbac_config(rbac_config)

    // Protect admin panel
    .with_route_guard("/admin/*", RouteGuard::RequireAuth {
        redirect_to: Some("/admin/login/".to_string()),
        exclude: vec!["/admin/login/".to_string()],
    })

    // Protect settings with a role check
    .with_route_guard("/settings/*", RouteGuard::RequireRole {
        roles: vec!["Admin".to_string()],
        redirect_to: Some("/unauthorized".to_string()),
    })

    .serve()
    .await?;
```

### Method-Specific Guards

```rust
use hyper::Method;

// Only protect POST/PUT/DELETE, allow public GET
.with_route_guard_methods(
    "/api/articles/*",
    vec![Method::POST, Method::PUT, Method::DELETE],
    RouteGuard::RequireAuth {
        redirect_to: None,  // Return 401 JSON for API
        exclude: vec![],
    }
)
```

## 🎨 Integration with RBAC

Guards automatically integrate with Lithair's RBAC system:

```rust
// Session store from with_rbac_config is used automatically
LithairServer::new()
    .with_rbac_config(rbac_config)  // Creates session_manager
    .with_route_guard("/admin/*", RouteGuard::RequireAuth {
        // Automatically uses session_manager from rbac_config!
        redirect_to: Some("/login".to_string()),
        exclude: vec!["/login".to_string()],
    })
    .serve()
    .await?;
```

## 📊 Comparison: Before vs After

### ❌ Before (Custom Middleware)

```rust
// 50+ lines of boilerplate
async fn admin_guard(req: Request) -> Result<Response> {
    let token = extract_token(&req)?;
    let session_store = get_session_store()?;

    if let Some(session) = session_store.get(&token).await? {
        if session.is_valid() {
            Ok(next_handler(req).await?)
        } else {
            Ok(redirect_to_login())
        }
    } else {
        Ok(redirect_to_login())
    }
}

// Register middleware manually for each route
router.add_middleware("/admin/*", admin_guard);
```

### ✅ After (Declarative Guards)

```rust
// 4 lines, zero boilerplate
.with_route_guard("/admin/*", RouteGuard::RequireAuth {
    redirect_to: Some("/login".to_string()),
    exclude: vec!["/login".to_string()],
})
```

**90% simpler. 100% clearer.**

## 🔍 How It Works

1. **Registration**: Guards are registered during server build
2. **Evaluation**: On each request, matching guards are evaluated
3. **Session Check**: Validates token against session store
4. **Action**: Either allows request or returns denial response

```
Request → Match Guards → Validate Session → Allow/Deny
```

## 🚀 Future Enhancements

- [ ] Rate limiting guard
- [ ] `RequireScope` for OAuth2 scopes
- [ ] `IPWhitelist` for IP-based restrictions
- [ ] Guard composition (`And`, `Or`, `Not`)

## 📚 Examples

See working examples in:

- `examples/04-blog/` - Admin panel protection
- `examples/05-ecommerce/` - Multi-level guards
- `Lithair-Blog/` - Production usage

## 💡 Philosophy Recap

**The 90% Rule in action:**

- 🎯 **Most routes** need a simple auth check → `RouteGuard::RequireAuth`
- 🔐 **Role-scoped routes** → `RouteGuard::RequireRole`
- ✅ **Zero boilerplate** for the common cases

**"Why write 50 lines of middleware when `.with_route_guard()` does it better?"**
