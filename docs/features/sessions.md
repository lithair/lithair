# Sessions and authentication

Lithair ships built-in session management with persistent, event-sourced
storage, dual cookie + Bearer-token authentication, and optional per-model
gating that requires a valid session before any auto-generated `/api/{model}`
route will respond.

## Quick path: defaults

For most apps, `.with_auth()` activates a sensible default stack (session
store under the configured data directory, cookie-based session id, Bearer
token support).

```rust
use lithair_core::app::LithairServer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    LithairServer::new()
        .with_auth()
        .with_model::<Article>("./data/articles", "/api/articles")
        .serve()
        .await
}
```

## Programmatic path: explicit control

When you need to share the session store with other components (or wire it
up before the server starts), build a `SessionManager` yourself and hand it
to the builder via `SessionManager::from_arc(...)` (introduced in v0.7.1 to
avoid a double-`Arc` footgun — see CHANGELOG).

```rust
use lithair_core::app::LithairServer;
use lithair_core::session::{PersistentSessionStore, SessionManager};
use std::sync::Arc;

let store = Arc::new(PersistentSessionStore::new("./data/sessions").await?);
let manager = SessionManager::from_arc(store);

LithairServer::new()
    .with_sessions(manager)
    .with_model::<Article>("./data/articles", "/api/articles")
    .serve()
    .await
```

## Per-model gating

By default, the auto-generated REST routes are open. To require a valid
session on every `/api/{model}` request, call
`.with_models_require_session(true)` (added in v0.7.0):

```rust
LithairServer::new()
    .with_sessions(manager)
    .with_models_require_session(true)  // 401 without auth
    .with_model::<Article>("./data/articles", "/api/articles")
    .serve()
    .await
```

Unauthenticated requests get a 401. Authenticated requests carry the session
through to handlers, so custom routes built via `with_handler` can inspect
the caller. As of v0.9.0, `with_handler` honors this gate too.

## Auth flow shape

The wire shape is intentionally boring: clients log in (returning a session
id), present that id either as a cookie or as a `Bearer <id>` header on
subsequent requests, and the server resolves it against the session store on
each request.

```http
POST /auth/login                                # → returns session id
GET  /api/articles                              # cookie carried by browser
GET  /api/articles  Authorization: Bearer <id>  # token carried by SPA / CLI
```

## See also

- [`examples/06-auth-sessions/`](https://github.com/lithair/lithair/tree/main/examples/06-auth-sessions) — minimal sessions + auth flow.
- [`examples/07-auth-rbac-mfa/`](https://github.com/lithair/lithair/tree/main/examples/07-auth-rbac-mfa) — sessions combined with role-based access control and TOTP MFA.
