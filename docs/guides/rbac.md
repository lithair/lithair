# RBAC (Role-Based Access Control) with Lithair

Lithair provides **automatic RBAC enforcement** where you declare permission rules and the framework handles all validation automatically.

## Core Concept

In Lithair, RBAC works through **declarative permission checking**:

1. You implement the `PermissionChecker` trait
2. You connect it via `.with_model_full()`
3. Lithair **automatically** enforces permissions on all CRUD operations

**You never write permission checks in your handlers** - it's all handled by the framework.

## Quick Example

```rust
use lithair_core::rbac::PermissionChecker;
use std::sync::Arc;

// 1. Define your permission matrix
struct AppPermissionChecker;

impl PermissionChecker for AppPermissionChecker {
    fn has_permission(&self, role: &str, permission: &str) -> bool {
        match (role, permission) {
            // Customer: read-only access
            ("Customer", "ProductRead") => true,

            // Employee: read + write (no delete)
            ("Employee", "ProductRead") => true,
            ("Employee", "ProductWrite") => true,

            // Admin: full access
            ("Admin", _) => true,

            // Deny everything else
            _ => false,
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let permission_checker = Arc::new(AppPermissionChecker);
    let session_store = Arc::new(
        PersistentSessionStore::new("./data/sessions").await?
    );

    // 2. Connect to server
    LithairServer::new()
        .with_sessions(SessionManager::new(session_store.clone()))
        .with_model_full::<Product>(
            "./data/products", // where to store database ( events )
            "/api/products", // CRUD route
            Some(permission_checker),        // ← Permission checker
            Some(session_store.clone()),     // ← Session store for role extraction
        )
        .serve()
        .await
}
```

## How It Works Internally

### Request Flow with RBAC

```
1. Client sends request with Bearer token
   GET /api/products
   Authorization: Bearer abc123...

2. Lithair extracts token from header
   ↓

3. Looks up session in session_store
   session = store.get("abc123...")
   ↓

4. Extracts role from session
   role = session.get("role")  // e.g., "Customer"
   ↓

5. Checks permission via your PermissionChecker
   has_permission("Customer", "ProductRead") → true
   ↓

6. If allowed → Process request
   If denied → Return 403 Forbidden
```

### Automatic Enforcement Points

Lithair automatically checks permissions at these points:

| HTTP Method | Operation | Required Permission | When Checked |
|-------------|-----------|---------------------|--------------|
| `POST`      | Create    | `ProductWrite`      | Before creating item |
| `PUT`       | Update    | `ProductWrite`      | Before updating item |
| `DELETE`    | Delete    | `ProductDelete`     | Before deleting item |
| `GET`       | Read      | Not enforced*       | N/A |

*Read operations don't require permissions by default (assumes authenticated users can read)

## Permission Naming Convention

Permission names follow the pattern: `{Resource}{Action}`

```rust
"ProductRead"      // Read products
"ProductWrite"     // Create/Update products
"ProductDelete"    // Delete products
"OrderRead"        // Read orders
"OrderWrite"       // Create/Update orders
"AdminAccess"      // Access admin panel
```

## Role Hierarchies

Implement role hierarchies in your `PermissionChecker`:

```rust
impl PermissionChecker for AppPermissionChecker {
    fn has_permission(&self, role: &str, permission: &str) -> bool {
        // Admin inherits all permissions
        if role == "Admin" {
            return true;
        }

        // Manager inherits Employee permissions
        if role == "Manager" {
            if has_employee_permission(permission) {
                return true;
            }
            // Plus manager-specific permissions
            match permission {
                "EmployeeManage" => true,
                "ReportView" => true,
                _ => false,
            }
        }

        // Employee permissions
        if role == "Employee" {
            return has_employee_permission(permission);
        }

        // Customer permissions
        if role == "Customer" {
            return matches!(permission, "ProductRead" | "OrderRead");
        }

        false
    }
}

fn has_employee_permission(permission: &str) -> bool {
    matches!(permission, "ProductRead" | "ProductWrite" | "OrderRead" | "OrderWrite")
}
```

## Multi-Resource RBAC

Handle multiple resources with a single permission checker:

```rust
impl PermissionChecker for MultiResourceChecker {
    fn has_permission(&self, role: &str, permission: &str) -> bool {
        match (role, permission) {
            // Product permissions
            ("Customer", "ProductRead") => true,
            ("Employee", "ProductRead" | "ProductWrite") => true,

            // Order permissions
            ("Customer", "OrderRead" | "OrderWrite") => true,
            ("Employee", "OrderRead" | "OrderWrite" | "OrderDelete") => true,

            // User management permissions
            ("Admin", "UserRead" | "UserWrite" | "UserDelete") => true,

            // Admin has all permissions
            ("Admin", _) => true,

            _ => false,
        }
    }
}
```

## Dynamic Role Assignment

Assign roles during login:

```rust
async fn handle_login(req: Request) -> Response {
    let body: LoginRequest = parse_json(&req)?;

    // Authenticate user
    let user = authenticate(&body.username, &body.password)?;

    // Determine role (from database, config, etc.)
    let role = match user.user_type {
        UserType::Admin => "Admin",
        UserType::Employee => "Employee",
        UserType::Customer => "Customer",
    };

    // Create session with role
    let mut session = Session::new();
    session.set("user_id", user.id)?;
    session.set("role", role.to_string())?;  // ← Store role
    session.set("username", user.username)?;

    let token = session_store.create(session).await?;

    Ok(json({
        "session_token": token,
        "role": role,
        "expires_in": 3600
    }))
}
```

## Permission Patterns

### Attribute-Based Access Control (ABAC)

Extend permissions with attributes:

```rust
impl PermissionChecker for AbacChecker {
    fn has_permission(&self, role: &str, permission: &str) -> bool {
        // Parse permission as "action:resource:attribute"
        let parts: Vec<&str> = permission.split(':').collect();

        match parts.as_slice() {
            ["write", "product", "own"] => {
                // Users can only write their own products
                role == "Employee"
            },
            ["write", "product", "any"] => {
                // Only admins can write any product
                role == "Admin"
            },
            ["delete", resource, "any"] => {
                // Only admins can delete anything
                role == "Admin"
            },
            _ => false,
        }
    }
}
```

### Time-Based Permissions

```rust
use chrono::{Utc, Timelike};

impl PermissionChecker for TimeBasedChecker {
    fn has_permission(&self, role: &str, permission: &str) -> bool {
        let hour = Utc::now().hour();

        match (role, permission) {
            // Night shift can only access during 22:00-06:00
            ("NightShift", _) => hour >= 22 || hour < 6,

            // Day shift during 06:00-22:00
            ("DayShift", _) => hour >= 6 && hour < 22,

            // Admin: always
            ("Admin", _) => true,

            _ => false,
        }
    }
}
```

### Context-Aware Permissions

```rust
impl PermissionChecker for ContextAwareChecker {
    fn has_permission(&self, role: &str, permission: &str) -> bool {
        // Parse context from permission string
        // Format: "action:resource:context_key=context_value"

        match (role, permission) {
            // Department-scoped access
            ("Manager", perm) if perm.contains("department=sales") => true,
            ("Manager", perm) if perm.contains("department=engineering") => false,

            // Location-based
            ("Employee", perm) if perm.contains("location=office") => true,
            ("Employee", perm) if perm.contains("location=remote") => {
                // Remote access restricted
                matches!(perm, p if p.starts_with("ProductRead"))
            },

            _ => false,
        }
    }
}
```

## Testing RBAC

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_customer_permissions() {
        let checker = AppPermissionChecker;

        assert!(checker.has_permission("Customer", "ProductRead"));
        assert!(!checker.has_permission("Customer", "ProductWrite"));
        assert!(!checker.has_permission("Customer", "ProductDelete"));
    }

    #[test]
    fn test_employee_permissions() {
        let checker = AppPermissionChecker;

        assert!(checker.has_permission("Employee", "ProductRead"));
        assert!(checker.has_permission("Employee", "ProductWrite"));
        assert!(!checker.has_permission("Employee", "ProductDelete"));
    }

    #[test]
    fn test_admin_all_access() {
        let checker = AppPermissionChecker;

        assert!(checker.has_permission("Admin", "ProductRead"));
        assert!(checker.has_permission("Admin", "ProductWrite"));
        assert!(checker.has_permission("Admin", "ProductDelete"));
        assert!(checker.has_permission("Admin", "AnyPermission"));
    }
}
```

### Integration Tests

```bash
# Test as Customer (should fail)
curl -X POST http://localhost:3000/api/products \
  -H "Authorization: Bearer $CUSTOMER_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"id":"1","name":"Test","price":99.99}'
# Expected: 403 Forbidden

# Test as Employee (should succeed)
curl -X POST http://localhost:3000/api/products \
  -H "Authorization: Bearer $EMPLOYEE_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"id":"1","name":"Test","price":99.99}'
# Expected: 200 OK

# Test DELETE as Employee (should fail)
curl -X DELETE http://localhost:3000/api/products/1 \
  -H "Authorization: Bearer $EMPLOYEE_TOKEN"
# Expected: 403 Forbidden

# Test DELETE as Admin (should succeed)
curl -X DELETE http://localhost:3000/api/products/1 \
  -H "Authorization: Bearer $ADMIN_TOKEN"
# Expected: 204 No Content
```

## Error Responses

### 401 Unauthorized

No valid session token provided:

```json
{
  "error": "Unauthorized"
}
```

### 403 Forbidden

Valid token but insufficient permissions:

```json
{
  "error": "Insufficient permissions"
}
```

## Best Practices

### 1. Principle of Least Privilege

Grant minimum permissions needed:

```rust
//  Bad: Too permissive
("Customer", _) => true,

//  Good: Explicit permissions
("Customer", "ProductRead") => true,
("Customer", "OrderRead") => true,
```

### 2. Explicit Deny by Default

Always deny unknown permissions:

```rust
impl PermissionChecker for SecureChecker {
    fn has_permission(&self, role: &str, permission: &str) -> bool {
        match (role, permission) {
            // Explicit grants
            ("Employee", "ProductRead") => true,
            ("Employee", "ProductWrite") => true,

            // Explicit deny everything else
            _ => false,  // ← Always include this
        }
    }
}
```

### 3. Centralize Permission Logic

Keep all permission logic in the `PermissionChecker`:

```rust
//  Bad: Permission logic in handlers
async fn create_product(req: Request) -> Response {
    if user.role != "Admin" && user.role != "Employee" {
        return forbidden();
    }
    // ...
}

//  Good: Logic in PermissionChecker
impl PermissionChecker for AppChecker {
    fn has_permission(&self, role: &str, permission: &str) -> bool {
        matches!((role, permission),
            ("Admin", _) | ("Employee", "ProductWrite"))
    }
}
```

### 4. Document Your Permission Model

```rust
/// Permission Matrix:
///
/// | Role       | ProductRead | ProductWrite | ProductDelete |
/// |------------|-------------|--------------|---------------|
/// | Customer   |            |             |              |
/// | Employee   |            |             |              |
/// | Manager    |            |             |              |
/// | Admin      |            |             |              |
impl PermissionChecker for DocumentedChecker {
    fn has_permission(&self, role: &str, permission: &str) -> bool {
        // Implementation
    }
}
```

## Advanced: Custom Permission Attributes

Future feature for field-level permissions:

```rust
// Coming soon!
#[derive(DeclarativeModel)]
struct Product {
    #[permission(read = "Public")]
    id: String,

    #[permission(read = "Public", write = "ProductManager")]
    name: String,

    #[permission(read = "Employee", write = "Admin")]
    cost_price: f64,
}
```

## Complete Working Example

See [`examples/rbac_session_demo`](../../examples/rbac_session_demo/README.md) for a fully functional RBAC system with:

- User login with username/password
- Session management with Bearer tokens
- Three roles (Customer, Employee, Administrator)
- Automated permission enforcement
- Full integration tests

```bash
# Run the example
task examples:rbac-session

# Run automated tests
task examples:rbac-session:test
```

## Summary

Lithair's RBAC system:

 **Declarative** - Define rules, framework enforces
 **Automatic** - Zero permission checks in handlers
 **Flexible** - Support any permission model (RBAC, ABAC, etc.)
 **Type-safe** - Compile-time guarantees
 **Testable** - Easy to unit test permission logic
 **Zero overhead** - Checks only on write operations

**You focus on business rules, Lithair handles security enforcement.**

---

**Next:** [Session Management Guide](./sessions.md) | [Examples](../../examples/README.md)
