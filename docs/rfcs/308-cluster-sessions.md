# RFC 308: sessions and authentication in a native cluster

Status: **implemented** (step 1, store A in the native consensus group, store B in PostgreSQL). Decisions: [Decisions](#decisions).
Discussion and tracker: [#308](https://github.com/lithair/lithair/issues/308). Part of #248 (step 5, "Sessions and admission"), building on [RFC 248](248-three-node-cluster.md) and [RFC 296](296-postgres-storage-tiers.md).

## Problem

A server with a native consensus cluster refuses every session store, RBAC configuration and route guard (`build()`: "native consensus does not yet support local sessions/RBAC stores"). Sessions are local to one process, so a login on node A is unknown on node B, and a logout or revocation on A doesn't reach B. As a result, a clustered application today cannot authenticate users at all: no RBAC login, no OIDC (#288), no `with_models_require_session`. PostgreSQL models next to a native cluster (RFC 296 Q5) are limited to anonymous permission hooks.

RFC 248 sets the requirements: replicate creation, refresh, logout, revocation and role changes; **absolute expiry selected by the authority**; and authorization decided on **current state**, so that a minority never authorizes a revoked session.

## Two obstacles found in the code

1. **Core only recognizes its built-in stores.** The builder keeps the session store as `Arc<dyn Any>`, and the gate and `session_for_request` downcast it to the four built-in shapes (`RecognizedSessionStore`). Any other store is invisible: `with_models_require_session` refuses to start with it. A clustered store, whatever its backend, needs core to accept **any `SessionStore`**.
2. **Sliding expiration writes on every request.** `SessionMiddleware::extract_session` calls `touch()` then `set()` on each request. On a replicated or shared store that's one write per authenticated request. RFC 248 already asks for absolute expiry; OIDC sessions are absolute.

## Proposal

**Step 1: core accepts any session store, and declares which are cluster-safe.**
- The builder keeps `Arc<dyn SessionStore>` (next to the existing `Any` for compatibility). The gate, route guards, `session_for_request`, the RBAC login/logout and OIDC use it, so the built-in shapes stop being special.
- New `SessionStore::shared_authority(&self) -> bool` (default `false`): the store's sessions are visible and current on every node.
- With a native cluster, `build()` accepts sessions, RBAC, route guards and `with_models_require_session` **only with a shared-authority store**. Local stores stay refused, as today.
- Shared stores have **absolute expiry**: `SessionMiddleware` skips the touch write for them.

**Step 2: two shared-authority stores.**
- **A. Sessions in the native consensus group.** The sessions live in the cluster's own replicated, checkpointed state, next to the native models, in an internal collection. Creation, deletion and revocation are consensus writes. Reads take the same read barrier as native model reads, so only the ready leader authorizes, and a follower or minority answers 503 with `Retry-After`, exactly like native models. Expiry is absolute, chosen by the leader at login. Expiry checks use the deciding node's clock, under a documented skew bound (NTP required). No external dependency.
- **B. Sessions in PostgreSQL** (`lithair_postgres::SessionStore`). Rows live in the `lithair` schema. Reads are `SERIALIZABLE`, so a revocation is visible to every node immediately, and **any node** can authorize, not just the leader. Expiry is checked against **the database clock** (`expires_at > now()`), so there is no skew between application nodes. Expired rows are cleaned up by any node.

The two fit different deployments: A for a cluster with no external database, B for deployments that already have a highly available PostgreSQL (the RFC 296 Q5 topology), where any node serves authenticated traffic.

## Qualification
- Login on node A, authenticated request on B (B: any node; A: through the ready leader), logout on A, then rejection on B, with no stale window beyond the documented one.
- Revocation and role change take effect on every node; a minority or isolated leader refuses authorization (A).
- Restart and leader failover keep sessions; expiry is absolute; no write per request.
- OIDC login and `oidc.require` work in a native cluster with A and with B.
- Unrecognized or local stores are still refused with a native cluster.

## Decisions

| | Question | Decision |
|---|---|---|
| Q1 | Stores and order | **Step 1** (core accepts any store, declares shared authority), then **B** (PostgreSQL), then **A** (native consensus group). |
| Q2 | Sliding expiration | **Absolute expiry only** for shared stores; no write per request. |
| Q3 | RBAC accounts | **Configuration-declared users are enough**; only sessions are shared. Database-backed accounts are out of scope. |
