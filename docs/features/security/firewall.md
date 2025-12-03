# Firewall

IP-based firewall and rate limiting for protected admin paths.

- Allowlist/denylist with CIDR support
- Protected/exempt path prefixes
- Optional global and per-IP QPS limits
- Detailed logging for allow/deny decisions

## Key points

- Designed to protect `admin` and internal endpoints
- Denylist takes precedence over allowlist
- Works behind proxies with proper forwarded headers

## See also

- Overview: `./overview.md`
- Admin protection guide: `../../guides/admin-protection.md`
