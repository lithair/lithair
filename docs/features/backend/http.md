# HTTP Server and Handlers

HTTP conventions and handler patterns used across Lithair.

## Response types

- Use response aliases for clarity and consistency:
  - `type RespBody = BoxBody<Bytes, Infallible>`
  - `type Resp = Response<RespBody>`
- Helper for bodies:
  - `body_from<T: Into<Bytes>>(data: T) -> RespBody`

## Handler signatures

- Handlers follow consistent patterns for method, path params and request types.
- Prefer explicit parameter extraction and return `Resp`.

## Routing patterns

- Path parameters: use `strip_prefix(':')` style parsing for clarity.
- Keep handler and router signatures consistent using type aliases.

## JSON helpers

- Use JSON helpers for building responses (`.json()`, `.text()`, `.html()` patterns where provided).

## See also

- Server module: `../../modules/http-server/README.md`
- Guides: `../../guides/http_hardening_gzip_firewall.md`, `../../guides/http_performance_endpoints.md`
