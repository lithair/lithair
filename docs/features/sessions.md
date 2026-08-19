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

The login answers with the id in the JSON body **and** sets it as a
`session_token` cookie (`Path=/; Max-Age=<session_duration>; Secure;
HttpOnly; SameSite=Lax` by default), so a browser is authenticated without any
JavaScript touching the token. `ServerRbacConfig::session_duration` is the
lifetime of both the session and its cookie on this path (`[sessions]
max_age` only concerns `with_sessions` / `SessionMiddleware`).

The logout is idempotent and expiry-aware. It ends every session the request
names — the Bearer header **and** the cookie when both are present with
different values — and always answers with the clearing `Set-Cookie`
(`Max-Age=0`, exactly the login's attributes: a browser only drops a cookie
whose scope matches), on the 401 paths too, so a browser holding a dead or
expired cookie leaves clean. It is 200 when at least one of those sessions
was live, 401 otherwise (no token, or only unknown/expired ones — which are
deleted from the store anyway). `/auth/validate` applies the same liveness
rule as the gate: an expired session answers `{"valid":false}`.

`with_rbac_config` wraps its store in a `SessionManager`, so expired sessions
are swept from the store every `[sessions] cleanup_interval` seconds
(`LT_SESSION_CLEANUP_INTERVAL`, 300 by default).

`[sessions] cookie_enabled = false` (`LT_SESSION_COOKIE_ENABLED=false`) is
Bearer-only mode: the login sends no `Set-Cookie`, the logout no clear, and
nothing reads the `Cookie:` header — the token travels in the JSON body and
the `Authorization: Bearer` header only.

One struct, `lithair_core::session::CookieConfig`, is the authority for that
cookie: the login/logout emit it, and the model gate, the route guards,
`/auth/validate` and `SessionMiddleware` all read the cookie under its name.
Its attributes come from `[sessions]` in `config.toml` or the
`LT_SESSION_COOKIE_SECURE` / `LT_SESSION_COOKIE_HTTPONLY` /
`LT_SESSION_COOKIE_SAMESITE` env vars (`secure=false` for a plain-HTTP LAN
deployment behind no TLS, for instance), and `.with_session_cookie(CookieConfig
{ .. })` on the builder wins over both — including `host_prefix: true`, which
emits `__Host-session_token` (Secure forced, `Path=/`, no `Domain`; the build
refuses a `Domain` with it). `SessionMiddleware` uses the same default name;
an app that relied on the old `session_id` default sets
`SessionConfig::default().with_cookie_name("session_id")` explicitly.

## Cross-site request check (CSRF)

A cookie rides along on cross-site requests, so every state-changing endpoint
that accepts the session cookie is CSRF-relevant: a page on another site
could drive `POST {auth}/logout` (forced-logout DoS), the session-gated
`POST/PUT/PATCH/DELETE /api/{model}` writes, or the `{auth}/mfa/*` mutation
routes with the victim's cookie. `SameSite=Lax` on the cookie Lithair issues
already blocks that — but it is a property of *our* cookie, not of the
endpoint: the extractor honors any same-name cookie however it was set, and
one config flip to `SameSite=None` would silently drop the shield. The
cross-site check (issue #225) makes the protection endpoint-owned.

For **unsafe methods** (`POST`/`PUT`/`PATCH`/`DELETE`) whose credential is
the **session cookie** — a Bearer request is never checked, the
`Authorization` header is not forgeable cross-site — the server reads the
browser's fetch metadata first:

- `Sec-Fetch-Site: same-origin` / `same-site` / `none` → allowed;
- `Sec-Fetch-Site: cross-site` → `403 {"error":"cross-site request
  rejected"}` (one `warn` log line per rejection);
- header absent (older browser, curl, native app) → fallback: the `Origin`
  header's host — and port, when it names one — is compared against `Host`;
  absent that, the `Referer`'s host; a mismatch (including `Origin: null`)
  is rejected;
- none of the three headers present → allowed: non-browser clients (curl,
  scripts, native apps) don't send fetch metadata and keep working.

The rejected logout deletes no session and emits no clearing `Set-Cookie` —
the 403 must not be a forced-logout vector itself. `OPTIONS` stays exempt
(CORS preflight carries no credentials). No synchronizer tokens: fetch
metadata plus the origin fallback covers every current browser without
per-session state or template plumbing.

One knob turns it off, for setups that legitimately POST cross-site (a
separate front domain) until CORS is configured properly: `[sessions]
cross_site_check = "Off"` in `config.toml`, `LT_SESSION_CROSS_SITE_CHECK=Off`
in the environment, or `.with_session_cookie(CookieConfig { cross_site_check:
CrossSiteCheck::Off, .. })` on the builder. The default is `"Enforce"`.

`/auth` is only the default prefix. A login endpoint at a well-known path is an
unauthenticated oracle for wordlists (the `/wp-admin` problem), so
`.with_auth_path("/secure-a7f3k29")` — called *before* `with_rbac_config` /
`with_mfa_totp` — moves all eight auth routes (`login`, `logout`, `validate`,
`mfa/*`) under a caller-chosen prefix.

## See also

- [`examples/06-auth-sessions/`](https://github.com/lithair/lithair/tree/main/examples/06-auth-sessions) — minimal sessions + auth flow.
- [`examples/07-auth-rbac-mfa/`](https://github.com/lithair/lithair/tree/main/examples/07-auth-rbac-mfa) — sessions combined with role-based access control and TOTP MFA.
