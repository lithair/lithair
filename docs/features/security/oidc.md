# OpenID Connect login for custom handlers (experimental)

Feature `oidc` of `lithair-core` adds an OpenID Connect relying party
(authorization code flow with PKCE). **The provider authenticates; the
application authorizes.** A successful login yields only a verified
`(issuer, subject)` pair bound to a fresh server-side session. Provider claims
(email, groups, tenant, roles) never become permissions, and provider tokens are
discarded once the ID token is verified.

Protocol and ID token cryptography are delegated to
[`openidconnect`](https://docs.rs/openidconnect/4.0.1) 4.0.1. Its HTTP client is
Lithair's own `reqwest`: TLS-validated, redirects never followed, a per-request
timeout (default 5 s) and a 1 MiB response limit.

## Activation

```toml
lithair-core = { version = "1.15", features = ["oidc"] }
```

```rust,ignore
use lithair_core::app::{LithairServer, Method, RouteRequest};
use lithair_core::oidc::{Oidc, OidcConfig};
use lithair_core::session::{MemorySessionStore, SessionManager};
use std::sync::Arc;

let config = OidcConfig::new("https://idp.example.com", "my-client", "https://app.example.com")
    .with_client_secret_env("OIDC_CLIENT_SECRET")?;
let store = Arc::new(MemorySessionStore::new());
let oidc = Oidc::discover(config, store.clone()).await?; // fails closed

LithairServer::new()
    .with_sessions(SessionManager::from_arc(store))
    .with_oidc(oidc.clone())
    .with_route_async(Method::POST, "/api/orders", move |req: RouteRequest| {
        let oidc = oidc.clone();
        async move {
            let identity = match oidc.require(&req).await {
                Ok(identity) => identity,          // issuer + subject, verified
                Err(rejection) => return Ok(rejection), // 401/403/400/503
            };
            // Resolve the application principal from (issuer, subject) and
            // check current tenant membership inside the command transaction,
            // including on retries. Nothing is granted automatically.
            todo!()
        }
    })
    .serve()
    .await?;
```

Register the provider callback `https://app.example.com/auth/oidc/callback`.
`with_oidc` serves:

| Route | Purpose |
| --- | --- |
| `GET /auth/oidc/login[?return_to=/path]` | Start a login; 302 to the provider |
| `GET /auth/oidc/callback` | Finish it; 303 to `return_to` (default `/`) |
| `POST /auth/oidc/logout` | End the session server-side; 204 |

`Oidc::identity(&req)` returns `Ok(None)` without a live login, or an
`IdentityError` for a cross-site request, an ambiguous cookie or a failing
store. `Oidc::require` maps those to ready HTTP rejections.

## Configuration and failure

`Oidc::discover` fetches the provider metadata and keys and refuses to start on:
a missing client ID or client secret; an issuer, authorization, token or JWKS
URL that is not `https`; a public origin with a path, query or credentials; a
discovery document whose issuer differs from the configured one; a redirect,
error status or oversized response from the provider. The client secret is
never logged or returned (`Debug` output is redacted). Only a confidential
client (`client_secret_basic`) is supported.

`allow_insecure_loopback_for_tests()` accepts `http://` for loopback addresses
only. It exists for local test providers: it also drops `Secure` and the
`__Host-` cookie prefix. Never enable it in a deployment.

| Setting | Default |
| --- | --- |
| `with_login_ttl` | 5 minutes (at most 1 hour) |
| `with_session_ttl` | 8 hours, absolute (at most 400 days) |
| `with_max_pending_logins` | 10 000; more logins get 503 |
| `with_http_timeout` | 5 seconds per provider request |
| `with_scopes` | only `openid` |

## Login flow

1. `login` validates `return_to` (a local path: one leading `/`, no `//` or `\`),
   generates a PKCE S256 verifier, a random `state` and a random `nonce`, and
   stores a pending attempt keyed by `state`. The browser receives an
   `HttpOnly` binding cookie (`__Host-lithair-oidc-login` over HTTPS) whose
   SHA-256 the attempt keeps.
2. `callback` **removes** the attempt for `state` before anything else. A
   replayed or concurrent callback finds nothing, so one attempt yields at most
   one session. It then requires the attempt to be unexpired and the binding
   cookie to match (a login started in another browser is refused). A provider
   `error` is refused.
3. The code is exchanged with the PKCE verifier. The ID token must be signed by
   a provider key with an allowed asymmetric algorithm (RS/PS 256–512,
   ES256/384, EdDSA; never `none` or HMAC), with the exact issuer, the client ID
   as audience (any additional audience is refused), an unexpired `exp`, and
   the attempt's nonce.
4. A new 256-bit random session id is stored with the issuer and subject. An
   earlier OIDC session of the same browser is deleted (no fixation).

The binding cookie is cleared on every callback outcome. No failed callback
creates a session. Status codes: 400 for state, binding, `return_to` or
input errors; 401 for a provider denial or an invalid ID token; 502 for a token
endpoint failure or a missing ID token; 503 when the store or pending-login
bound is exhausted.

### Key rotation

When an ID token names a key id missing from the cached key set, the relying
party rediscovers the provider (metadata and JWKS) and verifies once more. It
refreshes at most once per minute; the first refresh after startup is
immediate. A token signed with an unknown key between two refreshes is refused
(401), and the user logs in again.

## Sessions and requests

The session cookie (`__Host-lithair-oidc` over HTTPS) is `HttpOnly`,
`SameSite=Lax`, `Path=/`, host-only and `Secure`. It is separate from the RBAC
password cookie. The session id is opaque; provider tokens are never stored or
sent to the browser.

- **Absolute expiry.** The session ends `session_ttl` after login; activity
  never extends it.
- **Revocation.** Logout deletes the session from the store; the old cookie no
  longer works even if a client keeps it.
- **Store loss.** With `MemorySessionStore`, a restart logs everyone out.
  Sessions are local to one process: cross-node session replication is not
  provided (#248).
- **Pending logins** also live in the process; a restart or a second instance
  behind a load balancer invalidates them (the user starts the login again).

`identity` reads only the dedicated cookie. `Authorization: Bearer`, forwarded
identity headers and request bodies are ignored, so a request cannot change its
subject. Two session cookies are refused as ambiguous. A session without the
OIDC keys (for example an RBAC password session) is not an OIDC identity.

**CSRF.** For `POST`, `PUT`, `PATCH` and `DELETE`, the `Origin` header must equal
the configured public origin; without `Origin`, only
`Sec-Fetch-Site: same-origin` is accepted. Anything else is 403, logout
included. Safe methods are never checked: keep them free of side effects.

## Provider assumptions and limits

- Standard discovery at `{issuer}/.well-known/openid-configuration`, a
  `token_endpoint`, a `jwks_uri` and asymmetric ID token signatures.
- No refresh tokens, userinfo calls, front- or back-channel provider logout, or
  dynamic registration. Logout ends the Lithair session only; the provider
  session may still let the user sign in again without a prompt.
- One relying party (provider) per `Oidc` value; register routes once.
- The `rsa` crate pulled by `openidconnect` carries RUSTSEC-2023-0071 (Marvin).
  It affects private-key operations; the relying party only verifies signatures
  with public keys. The advisory is ignored in `.cargo/audit.toml` with that
  justification.

## Qualification

`lithair-core/tests/oidc_test.rs` and the `features/core/oidc.feature` Gherkin
scenario run a signed local provider and a real session store:

- login, who-am-I, subject unchanged by body, headers or Bearer, logout revocation
- missing, unknown, replayed and expired state; missing or foreign binding cookie
- eight concurrent callbacks for one attempt yield exactly one session
- foreign signature, wrong issuer, audience or nonce, expired token, token
  endpoint failure, missing ID token, refused code, provider denial
- provider key rotation
- discovery issuer mismatch, redirect, 2 MiB document, error status, and invalid
  configuration (insecure URLs, missing secret or client ID, origin with a path)
- forged, non-OIDC and ambiguous cookies; cross-site and Origin-less writes;
  cross-site logout
- absolute session expiry and a restart with a new store

Each protection was checked by removing it and confirming a test fails.
Qualification against real deployment providers remains an integration step.
