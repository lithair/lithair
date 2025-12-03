# Security Overview

Security features included by default in Lithair.

- RBAC (agnostic `Permission` trait) and middleware
- Sessions/JWT, persistent session store (event sourced)
- IP-based firewall + rate limiting for admin paths
- HTTP hardening (gzip, headers)

## Key concepts

- Permission model: application-defined permissions implementing `Permission`
- Session management: cookie-based, persistent store
- Admin protection: automatic endpoints + firewall + custom handlers

## Related docs

- RBAC guide: `../../guides/rbac.md`
- Admin protection: `../../guides/admin-protection.md`
- HTTP hardening: `../../guides/http_hardening_gzip_firewall.md`
