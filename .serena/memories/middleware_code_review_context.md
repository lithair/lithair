# Code Review Context: lithair-core/src/security/middleware.rs

## File Overview
**File**: `/home/arcker/projects/lithair/lithair/lithair-core/src/security/middleware.rs`
**Purpose**: RBAC Middleware for Lithair HTTP Server
**Size**: 584 lines (mostly complete implementation)
**Lines of Code**: ~508 lines of actual code (includes comments, tests)

---

## 1. SECURITY TYPE DEPENDENCIES (from mod.rs)

### Core Types Re-exported by security/mod.rs:
```rust
pub use core::{
    AuthContext, Permission, Role, RoleId, SecurityError, SecurityEvent, SecurityState, Session,
    SessionId, User, UserId,
};
pub use middleware::{JwtClaims, RBACMiddleware};
pub use password::{hash_password, verify_password, PasswordError, PasswordHasherService};
```

### Key Types from security/core.rs:

**AuthContext<P>** (lines 171-178):
- Generic over Permission type
- Fields: user_id, session_id, permissions (HashSet), team_id, organization_id
- Immutable authentication state after validation

**Permission** (trait, lines 66-84):
- Must implement: Clone + PartialEq + Eq + Hash + Send + Sync + Debug + 'static
- Required methods: identifier(), description()
- Optional methods: category(), level()
- Framework-agnostic - applications define their own permission types

**SecurityState<P>** (lines 309-438):
- Contains: users (HashMap), roles, user_roles, active_sessions, object_ownership, team_memberships, organization_memberships
- Key methods: get_user_permissions(), user_owns_object(), users_same_team(), apply_security_event()

**Session** (lines 160-168):
- Fields: id, user_id, created_at, expires_at, last_activity, ip_address, user_agent
- Used for server-side session tracking

**SecurityError** (enum, lines 441-451):
- Variants: AuthenticationFailed, AccessDenied, InvalidToken, SessionExpired, UserNotFound, RoleNotFound, PermissionNotFound, InvalidCredentials

**User** (lines 145-156):
- Fields: id, email, name, password_hash, team_id, organization_id, created_at, last_login, is_active
- Used for user storage and lookup

---

## 2. PASSWORD VERIFICATION (super::password module)

### Module: `/home/arcker/projects/lithair/lithair/lithair-core/src/security/password.rs`

**PasswordHasherService** (struct):
- Uses Argon2id (OWASP recommended)
- Methods:
  - `hash_password(password: &str) -> Result<String, PasswordError>`
  - `verify_password(password: &str, hash: &str) -> Result<bool, PasswordError>`
  - `needs_rehash(hash: &str) -> bool`

**Global convenience functions**:
```rust
pub fn verify_password(password: &str, hash: &str) -> Result<bool, PasswordError>
pub fn hash_password(password: &str) -> Result<String, PasswordError>
```

**Integration in middleware.rs** (line 11, 490-502):
- Imported as: `use super::password::verify_password as argon2_verify;`
- Used in: `verify_password()` method (lines 490-502)
- Handles both Argon2 hashes and legacy plaintext for migration
- Falls back to plaintext if hash doesn't start with "$argon2"

---

## 3. HTTP REQUEST INTERFACE (crate::http::HttpRequest)

### Module: `/home/arcker/projects/lithair/lithair/lithair-core/src/http/request.rs`

**HttpRequest struct** (lines 113-121):
- Fields: method, path, query_params, version, headers (HashMap), body, remote_addr
- All fields private - access via methods

**Key accessor methods used in middleware.rs**:
```rust
pub fn header(&self, name: &str) -> Option<&str>
  // Returns header value, case-insensitive lookup via name.to_lowercase()
  // Line 280-281: self.headers.get(&name.to_lowercase()).map(|s| s.as_str())
```

**Headers format**:
- Type: `HashMap<String, String>`
- Keys are normalized to lowercase
- Used for extracting "Authorization" header (Bearer tokens)

**Middleware usage** (line 152):
```rust
request.header("Authorization").ok_or(SecurityError::AuthenticationFailed)?;
```

---

## 4. EXTERNAL DEPENDENCIES (Cargo.toml)

File: `/home/arcker/projects/lithair/lithair/lithair-core/Cargo.toml`

### Cryptography Dependencies:
```toml
sha2 = "0.10"                  # SHA256 for JWT signatures
hmac = "0.12"                  # HMAC for JWT signatures (crypto-secure)
argon2 = "0.5"                 # Password hashing (OWASP recommended)
base64 = "0.22"                # Proper base64 encoding/decoding
uuid = { version = "1", features = ["v4", "serde"] }  # UUIDs for session IDs
```

### Type Aliases in middleware.rs (line 23):
```rust
type HmacSha256 = Hmac<Sha256>;  // From hmac and sha2 crates
```

### Imports in middleware.rs:
```rust
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;
```

---

## 5. FILES REFERENCING RBACMiddleware (Import Usage)

### Files where RBACMiddleware is used/imported:

1. **lithair-core/src/lib.rs** (re-export):
   - Exports: `AuthContext, Permission, RBACMiddleware, Role, SecurityError, SecurityEvent, SecurityState`

2. **lithair-core/src/security/mod.rs** (definition module):
   - Re-exports: `pub use middleware::{JwtClaims, RBACMiddleware};`

3. **lithair-core/src/security/middleware.rs** (definition):
   - Tests: Uses TestPermission enum (lines 521-525) for unit tests

4. **lithair-core/src/session/mod.rs**:
   - Re-exports: `pub use middleware::SessionMiddleware;` (different middleware)

5. **lithair-core/src/rbac/mod.rs**:
   - Re-exports: `pub use middleware::RbacMiddleware;` (DIFFERENT - simpler middleware)

### Note: No examples found using RBACMiddleware
- `grep -r "RBACMiddleware" /examples --include="*.rs"` returned no results
- Middleware exists but may not be actively used in current examples

---

## 6. DUPLICATE/RELATED MIDDLEWARE ANALYSIS

### THREE different middleware systems exist:

#### 1. **security/middleware.rs - RBACMiddleware<P: Permission>** ✓ (our target)
- **Location**: `/lithair-core/src/security/middleware.rs` (508 lines)
- **Generic**: Yes - generic over Permission type
- **Features**: 
  - Full JWT token creation and validation
  - Complete authentication workflow (email/password)
  - Authorization with object-level and team-based checks
  - Session management
  - Argon2 password verification
  - Event logging (to stdout, not persisted)
- **Design**: Memory-based (Arc<RwLock<SecurityState>>)
- **Status**: Fully implemented

#### 2. **session/middleware.rs - SessionMiddleware<S: SessionStore>**
- **Location**: `/lithair-core/src/session/middleware.rs` (150+ lines)
- **Purpose**: Extract and manage sessions from HTTP requests
- **Features**:
  - Cookie extraction
  - Bearer token extraction
  - Session store abstraction
  - Async support
- **Design**: Uses trait-based SessionStore
- **Note**: Complementary to RBACMiddleware - handles session loading

#### 3. **rbac/middleware.rs - RbacMiddleware** (simpler)
- **Location**: `/lithair-core/src/rbac/middleware.rs` (62 lines)
- **Purpose**: Thin wrapper around AuthProvider trait
- **Features**:
  - Delegates to pluggable AuthProvider
  - Simple authentication wrapper
- **Design**: Provider pattern
- **Note**: Very minimal - defers all logic to provider

### Functional Relationship:
```
RBACMiddleware (security/middleware.rs) - Full-featured auth/authz
SessionMiddleware (session/middleware.rs) - Session extraction
RbacMiddleware (rbac/middleware.rs) - Thin auth wrapper
```

---

## 7. CARGO.toml - Dependency Analysis

### Security-critical Dependencies:
| Package | Version | Purpose | Security Notes |
|---------|---------|---------|-----------------|
| `hmac` | 0.12 | JWT signature verification | Constant-time MAC |
| `sha2` | 0.10 | JWT signature hash | RFC 6234 compliant |
| `argon2` | 0.5 | Password hashing | OWASP 2024 recommended |
| `base64` | 0.22 | Base64url encoding | RFC 4648 compliant |
| `uuid` | 1 (v4) | Session IDs | Cryptographically random |
| `rand` | 0.8 | Randomness for salts | System entropy |

### No external auth libraries:
- No `jsonwebtoken` crate (JWT handling done manually)
- No `actix-web` or `axum` auth middleware (standalone implementation)
- No OAuth/OIDC libraries (only basic auth + JWT)

---

## 8. JWT IMPLEMENTATION NOTES

### JWT Structure in middleware.rs:
- **Header**: Hardcoded `{"alg":"HS256","typ":"JWT"}` (line 324)
- **Payload**: Custom JSON parsing (lines 328-331)
- **Signature**: HMAC-SHA256 with base64url encoding (lines 433-443)

### Manual JSON Parsing (lines 445-488):
- `extract_json_number(payload: &str, key: &str) -> Option<u64>`
- `extract_json_string(payload: &str, key: &str) -> Option<String>`
- No external JSON parser dependency (custom implementation)

### Security concern: Line 163 comment
```rust
// Simple JWT validation (in production, use a proper JWT library)
```
- Manual JWT handling is acknowledged as simplified
- Suggests future migration to proper JWT library

---

## 9. KEY METHOD SUMMARY

### Public Methods:
| Method | Lines | Purpose |
|--------|-------|---------|
| `new()` | 47-54 | Create middleware instance |
| `with_session_timeout()` | 80-83 | Builder pattern for timeout |
| `authenticate_request()` | 86-112 | Full HTTP request auth flow |
| `authenticate_token()` | 57-77 | Direct JWT token auth |
| `authorize_action()` | 115-147 | Check permissions on resource |
| `create_jwt_token()` | 314-338 | Generate new JWT |
| `authenticate_user()` | 341-409 | Email/password auth + session create |
| `logout_user()` | 412-421 | Revoke session |

### Private Helper Methods:
- `extract_jwt_token()` - Parse Bearer header
- `validate_jwt()` - JWT signature & expiration
- `validate_session()` - Check session is active
- `get_user_permissions()` - Fetch user permissions
- `get_user_context()` - Get team/org info
- `check_ownership_permission()` - Simple owner check
- `check_team_permission()` - Team membership check
- `log_access_granted/denied()` - Audit logging
- `base64url_encode/decode()` - Base64 utilities
- `create_jwt_signature()` - HMAC-SHA256 signing
- `parse_jwt_claims()` - Extract payload data
- `verify_password()` - Argon2 or plaintext check
- `generate_session_id()` - UUID v4 with prefix

---

## 10. SYNCHRONIZATION & THREAD-SAFETY

### Arc<RwLock<SecurityState<P>>> pattern:
- Line 39: `security_state: Arc<RwLock<SecurityState<P>>>`
- Read-heavy operations: `.read().unwrap()` for lookups
- Write operations: `.write().unwrap()` for session insert/delete
- No async support (synchronous lock, not async-aware)

### RwLock usage in middleware.rs:
- Line 200: `let state = self.security_state.read().unwrap();` (validate_session)
- Line 219: `let state = self.security_state.read().unwrap();` (get_user_permissions)
- Line 225: `let state = self.security_state.read().unwrap();` (get_user_context)
- Line 258: `let state = self.security_state.read().unwrap();` (check_team_permission)
- Line 389: `let mut state = self.security_state.write().unwrap();` (store session)
- Line 413: `let mut state = self.security_state.write().unwrap();` (logout)

### Unwrap considerations:
- All `.unwrap()` calls could panic if lock is poisoned
- No error handling for lock acquisition failures

---

## 11. TESTING COVERAGE

### Unit Tests in middleware.rs (lines 513-583):
1. `test_rbac_middleware_creation()` - Basic instantiation
2. `test_jwt_token_creation()` - Token generation
3. `test_session_id_generation()` - Session ID uniqueness

### Test fixture:
```rust
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum TestPermission {
    ReadData, WriteData, DeleteData,
}
```

### Missing tests:
- JWT validation with bad signatures
- Expired token handling
- Session expiration
- Password verification
- Authorization failures
- Integration tests with actual HTTP requests

---

## 12. SECURITY OBSERVATIONS

### Strengths:
- ✅ Argon2id for password hashing (OWASP recommended)
- ✅ HMAC-SHA256 for JWT (cryptographically secure)
- ✅ UUID v4 for session IDs (cryptographically random)
- ✅ Base64url encoding (RFC 4648 compliant)
- ✅ Generic Permission system (framework-agnostic)

### Concerns:
- ⚠️ Manual JWT implementation (acknowledged in code comment)
- ⚠️ Logging to stdout (println!) instead of event persistence
- ⚠️ `.unwrap()` calls on locks (no poison handling)
- ⚠️ Limited test coverage for security paths
- ⚠️ No rate limiting (noted as "Anti-DDoS" module separate)
- ⚠️ Session timeout hardcoded to 24 hours (builder changes it, but default high)
- ⚠️ No HTTPS enforcement (responsibility of caller)

---

## 13. CODE ORGANIZATION

```
security/
├── mod.rs                  (re-exports)
├── core.rs                 (types: AuthContext, Permission, User, Session, SecurityState)
├── password.rs             (Argon2 hashing)
└── middleware.rs           (RBACMiddleware - THIS FILE)

session/
└── middleware.rs           (SessionMiddleware - complementary)

rbac/
└── middleware.rs           (RbacMiddleware - alternative, simpler)
```

---

## COMPILATION ENVIRONMENT

**File location**: `/home/arcker/projects/lithair/lithair/lithair-core/src/security/middleware.rs`
**Workspace**: Lithair declarative memory-first web framework
**Rust Edition**: Workspace edition (likely 2021)
**Target audience**: Security module for Lithair applications
