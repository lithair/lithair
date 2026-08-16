# Sessions and authentication

Lithair ships built-in session management with persistent, event-sourced
storage, dual cookie + Bearer-token authentication, and optional per-model
gating that requires a valid session before any auto-generated `/api/{model}`
route will respond.

## Wiring sessions

Build a session store, wrap it in a `SessionManager`, and hand it to the
builder via `.with_sessions(...)`. Use `SessionManager::from_arc(...)`
(introduced in v0.7.1 to avoid a double-`Arc` footgun — see CHANGELOG) when
you already hold an `Arc` to the store, so the store can be shared with other
components.

```rust
use lithair_core::app::LithairServer;
use lithair_core::session::{PersistentSessionStore, SessionManager};
use lithair_core::DeclarativeModel;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(DeclarativeModel, Serialize, Deserialize, Clone, Debug)]
struct Article {
    #[http(expose)]
    id: String,
    #[http(expose)]
    title: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // PersistentSessionStore::new is synchronous and takes a PathBuf.
    let store = Arc::new(PersistentSessionStore::new("./data/sessions".into())?);
    let manager = SessionManager::from_arc(store);

    LithairServer::new()
        .with_sessions(manager)
        .with_model::<Article>("./data/articles", "/api/articles")
        .serve()
        .await
}
```

## Per-model gating

By default, the auto-generated REST routes are open. To require a valid
session on every `/api/{model}` request, call
`.with_models_require_session(true)` (added in v0.7.0):

```rust
LithairServer::new()
    .with_sessions(manager)
    .with_models_require_session(true)  // 401 without a valid session
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
POST /auth/login                                # -> returns session id
GET  /api/articles                              # cookie carried by browser
GET  /api/articles  Authorization: Bearer <id>  # token carried by SPA / CLI
```

`/auth` is only the default prefix. A login endpoint at a well-known path is an
unauthenticated oracle for wordlists (the `/wp-admin` problem), so
`.with_auth_path("/secure-a7f3k29")` — called *before* `with_rbac_config` /
`with_mfa_totp` — moves all eight auth routes (`login`, `logout`, `validate`,
`mfa/*`) under a caller-chosen prefix.

## See also

- [`examples/06-auth-sessions/`](https://github.com/lithair/lithair/tree/main/examples/06-auth-sessions) — minimal sessions + auth flow.
- [`examples/07-auth-rbac-mfa/`](https://github.com/lithair/lithair/tree/main/examples/07-auth-rbac-mfa) — sessions combined with role-based access control and TOTP MFA.
