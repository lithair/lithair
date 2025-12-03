# Sessions

Session management with persistent store and JWT support.

- Persistent session store (event-sourced) for durability and audit
- Cookie-based sessions with configurable max age and cookie name
- Optional JWT flows for stateless auth

## Key points

- Session store path per environment (e.g., `./data/sessions`)
- Rebuilds session state on startup by replaying events
- Integrates with RBAC middleware for authorization

## See also

- Overview: `./overview.md`
- Admin protection: `../../guides/admin-protection.md`
- Getting started: `../../guides/getting-started.md`
