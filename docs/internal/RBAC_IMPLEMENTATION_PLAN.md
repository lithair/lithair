# 🔐 Lithair RBAC Implementation Plan

**Date Started:** 2025-10-01  
**Status:** 🚧 In Progress  
**Goal:** Implement a complete, extensible RBAC system in Lithair core

---

## 🎯 Vision

Create a declarative, extensible RBAC system that:
- Works seamlessly with `DeclarativeServer`
- Supports multiple authentication providers (password, OAuth, SAML, custom)
- Provides field-level permissions
- Requires minimal configuration
- Is production-ready and secure

---

## 📋 Implementation Phases

### Phase 1: Core RBAC Infrastructure ✅ STARTED

**Goal:** Create the foundational RBAC system with basic password authentication

#### 1.1 Module Structure
- [x] `lithair-core/src/rbac/mod.rs` - Main module with exports
- [ ] `lithair-core/src/rbac/traits.rs` - Core traits for extensibility
- [ ] `lithair-core/src/rbac/permissions.rs` - Permission system
- [ ] `lithair-core/src/rbac/roles.rs` - Role management
- [ ] `lithair-core/src/rbac/context.rs` - Authentication context
- [ ] `lithair-core/src/rbac/middleware.rs` - HTTP middleware
- [ ] `lithair-core/src/rbac/providers/mod.rs` - Provider infrastructure

#### 1.2 Password Provider (Simple Auth)
- [ ] `lithair-core/src/rbac/providers/password.rs`
- [ ] Header-based authentication: `X-Auth-Password` + `X-Auth-Role`
- [ ] Similar to Apache Basic Auth but simpler
- [ ] No user database needed (stateless)

#### 1.3 Core Types

```rust
// Permission levels
pub enum PermissionLevel {
    Public,      // Anyone can access
    Read,        // Can read
    Write,       // Can write
    Delete,      // Can delete
    Admin,       // Admin only
}

// Field permission
pub struct FieldPermission {
    pub field_name: String,
    pub read: Vec<String>,   // Roles/Groups that can read
    pub write: Vec<String>,  // Roles/Groups that can write
}

// Auth context (supports both roles and groups)
pub struct AuthContext {
    pub user_id: Option<String>,      // User identifier (from LDAP, OAuth, etc.)
    pub roles: Vec<String>,            // User roles (e.g., "Admin", "Manager")
    pub groups: Vec<String>,           // User groups (e.g., "Engineering", "Sales")
    pub authenticated: bool,
    pub provider: String,
    pub metadata: HashMap<String, String>,  // Provider-specific data
}

// Provider trait
pub trait AuthProvider {
    fn authenticate(&self, request: &Request) -> Result<AuthContext>;
    fn name(&self) -> &str;
    
    // Optional: Fetch user groups from external source
    fn fetch_groups(&self, user_id: &str) -> Result<Vec<String>> {
        Ok(vec![])
    }
}
```

**Key Design Decision:**
- `AuthContext` contains both `roles` (application-level) and `groups` (from identity provider)
- Permissions can check against both: `#[permission(read = "Admin,Engineering")]`
- LDAP groups map directly to Lithair groups
- Roles can be derived from groups via mapping rules

---

### Phase 2: DeclarativeServer Integration

**Goal:** Integrate RBAC into `DeclarativeServer` with automatic field filtering

#### 2.1 Attribute Parsing
- [ ] Parse `#[rbac(enabled = true, roles = "...", default_role = "...")]`
- [ ] Parse `#[permission(read = "Role1,Role2", write = "Admin")]` on fields
- [ ] Store metadata in model configuration

#### 2.2 Middleware Integration
- [ ] Add RBAC middleware to `DeclarativeServer`
- [ ] Intercept all HTTP requests
- [ ] Extract authentication from headers
- [ ] Store `AuthContext` in request extensions

#### 2.3 Response Filtering
- [ ] Intercept GET responses
- [ ] Parse JSON response body
- [ ] Filter fields based on user role and field permissions
- [ ] Return filtered JSON

#### 2.4 Request Validation
- [ ] Validate POST/PUT/DELETE permissions
- [ ] Check if user role has write/delete permissions
- [ ] Return 403 Forbidden if unauthorized

---

### Phase 3: Testing & Examples

**Goal:** Validate RBAC with comprehensive tests and working examples

#### 3.1 Unit Tests
- [ ] Test password provider authentication
- [ ] Test permission checking logic
- [ ] Test field filtering
- [ ] Test role validation

#### 3.2 Integration Tests
- [ ] Test with `DeclarativeServer`
- [ ] Test multiple roles
- [ ] Test field-level filtering
- [ ] Test unauthorized access

#### 3.3 Example Updates
- [ ] Update `rbac_sso_demo` to use real RBAC
- [ ] Add Taskfile tasks for testing different roles
- [ ] Document usage in README

---

### Phase 4: Advanced Providers (Future)

**Goal:** Support enterprise authentication providers

#### 4.1 OAuth 2.0 Providers (Social Login)
- [ ] **Google OAuth** (`providers/google.rs`)
  - OAuth 2.0 flow
  - Google Workspace integration
  - Profile + email scopes
- [ ] **GitHub OAuth** (`providers/github.rs`)
  - OAuth 2.0 flow
  - Organization membership
  - Team-based groups
- [ ] **Microsoft/Azure AD** (`providers/microsoft.rs`)
  - OAuth 2.0 + Microsoft Graph
  - Azure AD groups
  - Office 365 integration
- [ ] **Facebook OAuth** (`providers/facebook.rs`)
- [ ] **Twitter/X OAuth** (`providers/twitter.rs`)
- [ ] **LinkedIn OAuth** (`providers/linkedin.rs`)
- [ ] **Apple Sign In** (`providers/apple.rs`)
- [ ] **Discord OAuth** (`providers/discord.rs`)
- [ ] **Slack OAuth** (`providers/slack.rs`)

#### 4.2 Enterprise Providers
- [ ] **SAML v2** (`providers/saml.rs`)
  - SP-initiated and IdP-initiated flows
  - Assertion validation
  - Attribute mapping
  - Support for Okta, OneLogin, Auth0
- [ ] **LDAP/Active Directory** (`providers/ldap.rs`) ⭐ Priority
  - Connect to LDAP server
  - Authenticate users (BIND)
  - Fetch user groups automatically (memberOf)
  - Map LDAP groups to Lithair roles/groups
  - Support nested groups
  - Cache group memberships for performance
  - StartTLS support
- [ ] **OpenID Connect (OIDC)** (`providers/oidc.rs`)
  - Generic OIDC provider
  - Discovery endpoint support
  - ID token validation
  - UserInfo endpoint
  - Works with Keycloak, Auth0, Okta, etc.
- [ ] **Kerberos** (`providers/kerberos.rs`)
  - SPNEGO/Negotiate authentication
  - Active Directory integration
  - Single Sign-On (SSO)
- [ ] **CAS (Central Authentication Service)** (`providers/cas.rs`)
  - University/education systems

#### 4.3 Modern Auth Providers
- [ ] **Auth0** (`providers/auth0.rs`)
  - Auth0-specific features
  - Rules and hooks integration
- [ ] **Okta** (`providers/okta.rs`)
  - Okta-specific API
  - Group management
- [ ] **Keycloak** (`providers/keycloak.rs`)
  - Open-source IAM
  - Realm and client configuration
- [ ] **AWS Cognito** (`providers/cognito.rs`)
  - User pools
  - Identity pools
  - AWS integration
- [ ] **Firebase Auth** (`providers/firebase.rs`)
  - Google Firebase
  - Mobile app integration

#### 4.4 Token-Based Providers
- [ ] **JWT tokens** (`providers/jwt.rs`)
  - HS256, RS256, ES256
  - Custom claims
  - Token refresh
- [ ] **API keys** (`providers/apikey.rs`)
  - Static API keys
  - Key rotation
  - Rate limiting per key
- [ ] **Bearer tokens** (`providers/bearer.rs`)
  - Generic bearer token validation
- [ ] **OAuth 2.0 Client Credentials** (`providers/oauth_client.rs`)
  - Machine-to-machine auth
  - Service accounts

#### 4.5 Multi-Factor Authentication (MFA) 🔐
- [ ] **TOTP (Time-based OTP)** (`mfa/totp.rs`)
  - Google Authenticator
  - Microsoft Authenticator
  - Authy
  - QR code generation
- [ ] **SMS OTP** (`mfa/sms.rs`)
  - Twilio integration
  - AWS SNS
  - Custom SMS provider
- [ ] **Email OTP** (`mfa/email.rs`)
  - Email-based codes
  - Magic links
- [ ] **WebAuthn/FIDO2** (`mfa/webauthn.rs`)
  - Hardware keys (YubiKey, etc.)
  - Biometric authentication
  - Passkeys
- [ ] **Push Notifications** (`mfa/push.rs`)
  - Mobile app push approval
  - Duo Security style
- [ ] **Backup Codes** (`mfa/backup_codes.rs`)
  - One-time recovery codes
  - Printable codes

#### 4.6 Passwordless Authentication
- [ ] **Magic Links** (`providers/magic_link.rs`)
  - Email-based login
  - Time-limited tokens
- [ ] **WebAuthn** (`providers/webauthn.rs`)
  - Passwordless with biometrics
  - Hardware security keys
- [ ] **SMS Login** (`providers/sms_login.rs`)
  - Phone number authentication

---

## 🏗️ Architecture

### Current Architecture

```
┌─────────────────────────────────────────┐
│         HTTP Request                     │
│  Headers: X-Auth-Password, X-Auth-Role   │
└──────────────┬──────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────┐
│      RbacMiddleware                      │
│  - Extract auth headers                  │
│  - Call AuthProvider.authenticate()      │
│  - Store AuthContext in request          │
└──────────────┬──────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────┐
│      DeclarativeServer                   │
│  - Process request normally              │
│  - Generate response                     │
└──────────────┬──────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────┐
│      Response Filter                     │
│  - Read AuthContext from request         │
│  - Parse JSON response                   │
│  - Filter fields by permissions          │
│  - Return filtered response              │
└──────────────┬──────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────┐
│         HTTP Response                    │
│  JSON with only authorized fields        │
└─────────────────────────────────────────┘
```

### Provider Architecture

```rust
pub trait AuthProvider: Send + Sync {
    /// Authenticate a request and return auth context
    fn authenticate(&self, request: &Request) -> Result<AuthContext, AuthError>;
    
    /// Provider name for logging
    fn name(&self) -> &str;
    
    /// Optional: Validate token/credentials
    fn validate(&self, token: &str) -> Result<bool, AuthError> {
        Ok(true)
    }
}

// Password Provider Implementation
pub struct PasswordProvider {
    pub password: String,
    pub default_role: String,
}

impl AuthProvider for PasswordProvider {
    fn authenticate(&self, request: &Request) -> Result<AuthContext> {
        let password = request.headers()
            .get("X-Auth-Password")
            .and_then(|h| h.to_str().ok());
            
        let role = request.headers()
            .get("X-Auth-Role")
            .and_then(|h| h.to_str().ok())
            .unwrap_or(&self.default_role);
            
        if password == Some(&self.password) {
            Ok(AuthContext {
                role: role.to_string(),
                authenticated: true,
                provider: "password".to_string(),
            })
        } else {
            Ok(AuthContext {
                role: self.default_role.clone(),
                authenticated: false,
                provider: "password".to_string(),
            })
        }
    }
    
    fn name(&self) -> &str {
        "password"
    }
}
```

---

## 📝 Usage Examples

### Declarative Model with RBAC

```rust
use lithair_macros::DeclarativeModel;
use uuid::Uuid;

#[derive(DeclarativeModel)]
#[rbac(
    enabled = true,
    roles = "Public,Customer,Manager,Admin",
    default_role = "Public",
    provider = "password",
    password = env("RBAC_PASSWORD")
)]
pub struct Product {
    #[db(primary_key)]
    #[permission(read = "Public")]
    pub id: Uuid,
    
    #[permission(read = "Public", write = "Manager")]
    pub name: String,
    
    #[permission(read = "Public", write = "Manager")]
    pub price: f64,
    
    #[permission(read = "Manager", write = "Manager")]
    pub stock: i32,
    
    #[permission(read = "Admin", write = "Admin")]
    pub cost: f64,
}
```

### Testing Different Roles

```bash
# Public access (no auth) - sees: id, name, price
curl http://localhost:8080/api/products

# Customer access (with password) - sees: id, name, price
curl http://localhost:8080/api/products \
  -H "X-Auth-Password: secret" \
  -H "X-Auth-Role: Customer"

# Manager access - sees: id, name, price, stock
curl http://localhost:8080/api/products \
  -H "X-Auth-Password: secret" \
  -H "X-Auth-Role: Manager"

# Admin access - sees: id, name, price, stock, cost
curl http://localhost:8080/api/products \
  -H "X-Auth-Password: secret" \
  -H "X-Auth-Role: Admin"
```

### Taskfile Integration

```yaml
examples:rbac:test:customer:
  desc: Test RBAC as Customer role
  cmds:
    - |
      curl http://127.0.0.1:{{.PORT}}/api/products \
        -H "X-Auth-Password: demo123" \
        -H "X-Auth-Role: Customer"

examples:rbac:test:manager:
  desc: Test RBAC as Manager role
  cmds:
    - |
      curl http://127.0.0.1:{{.PORT}}/api/products \
        -H "X-Auth-Password: demo123" \
        -H "X-Auth-Role: Manager"

examples:rbac:test:admin:
  desc: Test RBAC as Admin role
  cmds:
    - |
      curl http://127.0.0.1:{{.PORT}}/api/products \
        -H "X-Auth-Password: demo123" \
        -H "X-Auth-Role: Admin"
```

---

## 🎯 Success Criteria

### Phase 1 Complete When:
- [x] RBAC module structure created
- [ ] Password provider implemented
- [ ] Basic middleware working
- [ ] Can authenticate requests
- [ ] Can filter response fields
- [ ] Unit tests passing

### Phase 2 Complete When:
- [ ] `DeclarativeServer` reads `#[rbac]` attributes
- [ ] `DeclarativeServer` reads `#[permission]` attributes
- [ ] Middleware automatically applied
- [ ] Field filtering works for GET requests
- [ ] Permission validation works for POST/PUT/DELETE
- [ ] Integration tests passing

### Phase 3 Complete When:
- [ ] `rbac_sso_demo` uses real RBAC
- [ ] Can test different roles via Taskfile
- [ ] Documentation updated
- [ ] All tests passing
- [ ] Example demonstrates field-level filtering

---

## 📊 Current Progress

**Overall:** 5% Complete

| Phase | Status | Progress |
|-------|--------|----------|
| Phase 1: Core Infrastructure | 🚧 In Progress | 10% |
| Phase 2: DeclarativeServer Integration | ⏳ Not Started | 0% |
| Phase 3: Testing & Examples | ⏳ Not Started | 0% |
| Phase 4: Advanced Providers | 📅 Future | 0% |

---

## 🔄 Next Steps

1. **Immediate (Today):**
   - [ ] Create `traits.rs` with core traits
   - [ ] Create `permissions.rs` with permission types
   - [ ] Create `roles.rs` with role management
   - [ ] Create `context.rs` with auth context

2. **Short-term (This Week):**
   - [ ] Implement `PasswordProvider`
   - [ ] Create basic middleware
   - [ ] Test with simple example

3. **Medium-term (This Month):**
   - [ ] Integrate with `DeclarativeServer`
   - [ ] Implement field filtering
   - [ ] Update `rbac_sso_demo`

4. **Long-term (Future):**
   - [ ] Add OAuth providers
   - [ ] Add SAML support
   - [ ] Production hardening

---

## 🔐 Multi-Factor Authentication (MFA) Architecture

### How MFA Integrates with RBAC

```
┌─────────────────────────────────────────┐
│         HTTP Request                     │
│  Step 1: Primary Authentication          │
└──────────────┬──────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────┐
│      AuthProvider (LDAP, OAuth, etc.)    │
│  - Validates username/password           │
│  - Returns: user_id, roles, groups       │
│  - Sets: mfa_required = true             │
└──────────────┬──────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────┐
│      MFA Challenge                       │
│  IF mfa_required:                        │
│    - Generate challenge (TOTP, SMS, etc.)│
│    - Return 401 + X-MFA-Required header  │
│    - Store pending session               │
└──────────────┬──────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────┐
│         HTTP Request                     │
│  Step 2: MFA Verification                │
│  Headers: X-MFA-Code: 123456             │
└──────────────┬──────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────┐
│      MFA Validator                       │
│  - Verify TOTP code                      │
│  - OR verify SMS code                    │
│  - OR verify WebAuthn signature          │
│  - Complete authentication               │
└──────────────┬──────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────┐
│      Full AuthContext                    │
│  authenticated: true                     │
│  mfa_verified: true                      │
│  roles: [...]                            │
│  groups: [...]                           │
└─────────────────────────────────────────┘
```

### MFA Configuration

```rust
#[derive(DeclarativeModel)]
#[rbac(
    enabled = true,
    provider = "ldap",
    mfa = {
        enabled: true,
        methods: ["totp", "sms", "webauthn"],
        required_for_roles: ["Admin", "Manager"],
        grace_period: "7 days",
        remember_device: true
    }
)]
pub struct Product {
    // ... fields
}
```

### MFA Flow Example

```rust
// 1. Initial login
POST /auth/login
Body: { "username": "admin", "password": "..." }
Response: 401 Unauthorized
Headers:
  X-MFA-Required: true
  X-MFA-Methods: totp,sms
  X-MFA-Session: abc123...

// 2. Request MFA code (if SMS)
POST /auth/mfa/send
Headers: X-MFA-Session: abc123...
Body: { "method": "sms" }
Response: 200 OK

// 3. Verify MFA code
POST /auth/mfa/verify
Headers: X-MFA-Session: abc123...
Body: { "code": "123456" }
Response: 200 OK
Body: { "token": "jwt_token...", "mfa_verified": true }

// 4. Use authenticated token
GET /api/products
Headers: Authorization: Bearer jwt_token...
Response: 200 OK (with full access)
```

### MFA Provider Trait

```rust
pub trait MfaProvider: Send + Sync {
    /// Generate MFA challenge
    fn generate_challenge(&self, user_id: &str) -> Result<MfaChallenge>;
    
    /// Verify MFA response
    fn verify(&self, user_id: &str, code: &str) -> Result<bool>;
    
    /// MFA method name
    fn method(&self) -> &str;
    
    /// Setup MFA for user (returns QR code, secret, etc.)
    fn setup(&self, user_id: &str) -> Result<MfaSetup>;
}

pub struct MfaChallenge {
    pub session_id: String,
    pub method: String,
    pub expires_at: DateTime<Utc>,
    pub metadata: HashMap<String, String>,  // QR code, phone number, etc.
}

pub struct MfaSetup {
    pub secret: String,
    pub qr_code: Option<String>,  // Base64 PNG
    pub backup_codes: Vec<String>,
}
```

### Declarative MFA Configuration

```rust
// Per-model MFA requirements
#[rbac(
    mfa = {
        enabled: true,
        required_for_roles: ["Admin"],
        methods: ["totp", "webauthn"]
    }
)]

// Per-route MFA requirements
#[http_route(DELETE, "/api/products/{id}")]
#[permission(delete = "Admin")]
#[mfa(required = true, methods = ["totp", "webauthn"])]
async fn delete_product() { }

// Sensitive operations require fresh MFA
#[http_route(POST, "/admin/users/{id}/reset-password")]
#[permission(write = "Admin")]
#[mfa(required = true, max_age = "5 minutes")]
async fn reset_password() { }
```

---

## 🔄 Multiple Providers & OAuth Callbacks

### Multi-Provider Configuration

Lithair supports **multiple authentication providers simultaneously**, allowing users to choose their preferred login method.

```rust
#[derive(DeclarativeModel)]
#[rbac(
    enabled = true,
    providers = [
        {
            type: "google",
            client_id: env("GOOGLE_CLIENT_ID"),
            client_secret: env("GOOGLE_CLIENT_SECRET"),
            scopes: ["email", "profile"],
            priority: 1
        },
        {
            type: "github",
            client_id: env("GITHUB_CLIENT_ID"),
            client_secret: env("GITHUB_CLIENT_SECRET"),
            scopes: ["user:email"],
            priority: 2
        },
        {
            type: "ldap",
            server: env("LDAP_SERVER"),
            base_dn: env("LDAP_BASE_DN"),
            priority: 3
        },
        {
            type: "password",
            password: env("ADMIN_PASSWORD"),
            priority: 4
        }
    ],
    default_provider: "google"
)]
pub struct Product {
    // ... fields
}
```

### Auto-Generated OAuth Routes

Lithair **automatically generates** all OAuth callback routes for each configured provider:

```
Auto-generated routes for each provider:

Google:
  GET  /auth/google/login          → Redirect to Google OAuth
  GET  /auth/google/callback       → Handle Google callback
  POST /auth/google/logout         → Logout from Google

GitHub:
  GET  /auth/github/login          → Redirect to GitHub OAuth
  GET  /auth/github/callback       → Handle GitHub callback
  POST /auth/github/logout         → Logout from GitHub

LDAP:
  POST /auth/ldap/login            → LDAP authentication
  POST /auth/ldap/logout           → Logout from LDAP

Password:
  POST /auth/password/login        → Simple password auth
  POST /auth/password/logout       → Logout

Generic:
  GET  /auth/providers             → List available providers
  GET  /auth/status                → Current auth status
  POST /auth/logout                → Logout from all providers
```

### OAuth Callback Flow

```
┌─────────────────────────────────────────┐
│  User clicks "Login with Google"         │
└──────────────┬──────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────┐
│  GET /auth/google/login                  │
│  Lithair generates OAuth URL:          │
│  - redirect_uri: https://app.com/auth/   │
│                  google/callback          │
│  - state: random_token (CSRF protection) │
│  - scopes: email, profile                │
└──────────────┬──────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────┐
│  Redirect to Google OAuth                │
│  https://accounts.google.com/o/oauth2/   │
│  auth?client_id=...&redirect_uri=...     │
└──────────────┬──────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────┐
│  User authenticates with Google          │
│  (Google's login page)                   │
└──────────────┬──────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────┐
│  Google redirects back:                  │
│  GET /auth/google/callback?code=abc&     │
│      state=random_token                  │
└──────────────┬──────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────┐
│  Lithair GoogleProvider:               │
│  1. Verify state (CSRF protection)       │
│  2. Exchange code for access_token       │
│  3. Fetch user info from Google API      │
│  4. Extract email, name, groups          │
│  5. Map to Lithair roles               │
│  6. Generate internal JWT token          │
└──────────────┬──────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────┐
│  Redirect to app with JWT:               │
│  GET /?token=jwt_token                   │
│  OR Set-Cookie: auth_token=jwt_token     │
└─────────────────────────────────────────┘
```

### Callback URL Management

Lithair **automatically manages callback URLs** based on the server configuration:

```rust
// Automatic callback URL generation
impl GoogleProvider {
    fn get_callback_url(&self, request: &Request) -> String {
        // Auto-detect from request
        let scheme = if request.is_tls() { "https" } else { "http" };
        let host = request.headers()
            .get("Host")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("localhost:8080");
        
        format!("{}://{}/auth/google/callback", scheme, host)
    }
}

// Or configured explicitly
#[rbac(
    providers = [{
        type: "google",
        callback_url: "https://myapp.com/auth/google/callback",
        // OR use auto-detection
        callback_url: "auto"
    }]
)]
```

### Provider Priority & Fallback

When multiple providers are configured, Lithair uses **priority** for fallback:

```rust
#[rbac(
    providers = [
        { type: "google", priority: 1 },      // Try Google first
        { type: "ldap", priority: 2 },        // Fallback to LDAP
        { type: "password", priority: 3 }     // Last resort
    ]
)]
```

**Fallback logic:**
1. User tries to access protected resource
2. If no auth, try provider with priority 1
3. If fails, try provider with priority 2
4. Continue until authenticated or all fail

### User Account Linking

Users can link multiple providers to the same account:

```rust
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub linked_providers: Vec<LinkedProvider>,
}

pub struct LinkedProvider {
    pub provider: String,        // "google", "github", etc.
    pub provider_user_id: String, // User ID from provider
    pub linked_at: DateTime<Utc>,
    pub metadata: HashMap<String, String>,
}

// Example:
// User logs in with Google → Creates account
// User links GitHub → Same account, 2 providers
// User can now login with either Google OR GitHub
```

### Frontend Integration

Lithair can auto-generate a login page with all providers:

```html
<!-- Auto-generated by Lithair -->
<!DOCTYPE html>
<html>
<head>
    <title>Login - Lithair App</title>
</head>
<body>
    <h1>Choose Login Method</h1>
    
    <!-- OAuth Providers -->
    <a href="/auth/google/login">
        <button>🔵 Login with Google</button>
    </a>
    <a href="/auth/github/login">
        <button>⚫ Login with GitHub</button>
    </a>
    <a href="/auth/microsoft/login">
        <button>🔷 Login with Microsoft</button>
    </a>
    
    <!-- LDAP/Password -->
    <form action="/auth/ldap/login" method="POST">
        <input type="text" name="username" placeholder="Username">
        <input type="password" name="password" placeholder="Password">
        <button type="submit">🔑 Login with LDAP</button>
    </form>
</body>
</html>
```

### Configuration Example

Complete multi-provider setup:

```rust
#[derive(DeclarativeModel)]
#[rbac(
    enabled = true,
    
    // Multiple providers
    providers = [
        {
            type: "google",
            client_id: env("GOOGLE_CLIENT_ID"),
            client_secret: env("GOOGLE_CLIENT_SECRET"),
            callback_url: "auto",  // Auto-detect
            scopes: ["email", "profile"],
            priority: 1
        },
        {
            type: "github",
            client_id: env("GITHUB_CLIENT_ID"),
            client_secret: env("GITHUB_CLIENT_SECRET"),
            callback_url: "https://myapp.com/auth/github/callback",
            scopes: ["user:email", "read:org"],
            priority: 2
        },
        {
            type: "ldap",
            server: env("LDAP_SERVER"),
            base_dn: env("LDAP_BASE_DN"),
            priority: 3
        }
    ],
    
    // Allow account linking
    allow_account_linking: true,
    
    // Session management
    session = {
        duration: "7 days",
        refresh_token: true,
        remember_me: true
    },
    
    // Auto-generate login page
    login_page = {
        enabled: true,
        path: "/login",
        theme: "modern",
        logo: "/assets/logo.png"
    }
)]
pub struct Product {
    // ... fields
}
```

### Environment Variables

```bash
# .env file
# Google OAuth
GOOGLE_CLIENT_ID=xxx.apps.googleusercontent.com
GOOGLE_CLIENT_SECRET=xxx

# GitHub OAuth
GITHUB_CLIENT_ID=xxx
GITHUB_CLIENT_SECRET=xxx

# LDAP
LDAP_SERVER=ldap://company.com
LDAP_BASE_DN=dc=company,dc=com
LDAP_BIND_DN=cn=admin,dc=company,dc=com
LDAP_BIND_PASSWORD=xxx

# Session
SESSION_SECRET=random_secret_key
```

### Testing Multiple Providers

```bash
# Test Google login
curl http://localhost:8080/auth/google/login
# → Redirects to Google

# Test GitHub login
curl http://localhost:8080/auth/github/login
# → Redirects to GitHub

# Test LDAP login
curl -X POST http://localhost:8080/auth/ldap/login \
  -H "Content-Type: application/json" \
  -d '{"username": "jdoe", "password": "secret"}'

# List available providers
curl http://localhost:8080/auth/providers
# → {"providers": ["google", "github", "ldap"]}
```

---

## 🏢 LDAP/Active Directory Support

### Architecture

```
┌─────────────────────────────────────────┐
│         HTTP Request                     │
│  Headers: Authorization: Basic ...       │
└──────────────┬──────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────┐
│      LdapProvider                        │
│  1. Extract username/password            │
│  2. Connect to LDAP server               │
│  3. Authenticate user (BIND)             │
│  4. Fetch user groups (memberOf)         │
│  5. Map groups to roles                  │
└──────────────┬──────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────┐
│      AuthContext                         │
│  user_id: "jdoe"                         │
│  roles: ["Employee"]                     │
│  groups: ["Engineering", "Developers"]   │
│  authenticated: true                     │
└─────────────────────────────────────────┘
```

### Example Configuration

```rust
#[derive(DeclarativeModel)]
#[rbac(
    enabled = true,
    provider = "ldap",
    ldap_server = env("LDAP_SERVER"),
    ldap_base_dn = env("LDAP_BASE_DN"),
    ldap_bind_dn = env("LDAP_BIND_DN"),
    ldap_bind_password = env("LDAP_BIND_PASSWORD"),
    group_mapping = {
        "CN=Admins,OU=Groups,DC=company,DC=com" => "Admin",
        "CN=Developers,OU=Groups,DC=company,DC=com" => "Developer",
        "CN=Engineering,OU=Groups,DC=company,DC=com" => "Engineering"
    }
)]
pub struct Product {
    #[permission(read = "Public")]
    pub id: Uuid,
    
    #[permission(read = "Public", write = "Developer,Engineering")]
    pub name: String,
    
    #[permission(read = "Engineering", write = "Engineering")]
    pub technical_specs: String,
    
    #[permission(read = "Admin", write = "Admin")]
    pub cost: f64,
}
```

### LDAP Provider Implementation

```rust
pub struct LdapProvider {
    pub server: String,
    pub base_dn: String,
    pub bind_dn: String,
    pub bind_password: String,
    pub group_mapping: HashMap<String, String>,
    pub cache: Arc<RwLock<HashMap<String, (Vec<String>, Instant)>>>,
    pub cache_ttl: Duration,
}

impl AuthProvider for LdapProvider {
    fn authenticate(&self, request: &Request) -> Result<AuthContext> {
        // 1. Extract Basic Auth credentials
        let (username, password) = extract_basic_auth(request)?;
        
        // 2. Connect to LDAP
        let mut ldap = LdapConn::new(&self.server)?;
        
        // 3. Bind as service account
        ldap.simple_bind(&self.bind_dn, &self.bind_password)?;
        
        // 4. Search for user
        let user_dn = format!("uid={},{}", username, self.base_dn);
        
        // 5. Authenticate user
        ldap.simple_bind(&user_dn, password)?;
        
        // 6. Fetch user groups
        let groups = self.fetch_user_groups(&mut ldap, &user_dn)?;
        
        // 7. Map LDAP groups to roles
        let roles = self.map_groups_to_roles(&groups);
        
        Ok(AuthContext {
            user_id: Some(username.to_string()),
            roles,
            groups,
            authenticated: true,
            provider: "ldap".to_string(),
            metadata: HashMap::new(),
        })
    }
    
    fn fetch_groups(&self, user_id: &str) -> Result<Vec<String>> {
        // Check cache first
        if let Some((groups, timestamp)) = self.cache.read().get(user_id) {
            if timestamp.elapsed() < self.cache_ttl {
                return Ok(groups.clone());
            }
        }
        
        // Fetch from LDAP and cache
        let groups = self.fetch_from_ldap(user_id)?;
        self.cache.write().insert(
            user_id.to_string(),
            (groups.clone(), Instant::now())
        );
        
        Ok(groups)
    }
}
```

### Usage Example

```bash
# User authenticates with LDAP credentials
# LDAP groups: CN=Engineering,OU=Groups,DC=company,DC=com
# Mapped role: Engineering

curl http://localhost:8080/api/products \
  -u "jdoe:password123"

# Response includes fields visible to "Engineering" group:
# - id (Public)
# - name (Public)
# - technical_specs (Engineering)
# But NOT:
# - cost (Admin only)
```

### Group Mapping Strategies

1. **Direct Mapping**: LDAP group DN → Lithair role
2. **Pattern Matching**: `CN=*-Admins,*` → `Admin`
3. **Nested Groups**: Support group inheritance
4. **Dynamic Roles**: Derive roles from multiple groups

---

## 📚 References

- [RBAC Wikipedia](https://en.wikipedia.org/wiki/Role-based_access_control)
- [OAuth 2.0 RFC](https://tools.ietf.org/html/rfc6749)
- [SAML 2.0 Spec](http://docs.oasis-open.org/security/saml/Post2.0/sstc-saml-tech-overview-2.0.html)
- [JWT RFC](https://tools.ietf.org/html/rfc7519)
- [LDAP RFC](https://tools.ietf.org/html/rfc4511)

---

## 🤝 Contributing

This is a major feature implementation. If you want to contribute:
1. Read this plan thoroughly
2. Pick a specific task from "Next Steps"
3. Follow the architecture defined above
4. Write tests for your code
5. Update this document with your progress

---

**Last Updated:** 2025-10-01  
**Next Review:** After Phase 1 completion
