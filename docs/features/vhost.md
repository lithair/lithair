# Host-header routing (vhosts)

A single Lithair binary can serve multiple hostnames, each with its own
frontend bundle served from memory. Incoming requests are routed by the `Host`
header to the matching vhost. Models and custom routes remain host-agnostic
in this first iteration — vhosts segment the static frontend, not the API
surface.

This is intended for the common shape where one box serves several brands or
subdomains and you'd rather skip a reverse proxy. The
[design rationale](https://arcker.org/blog/2026-04-24-lithair-vhost-routing/)
covers why this layer ended up in-process.

## Declaring vhosts

```rust
use lithair_core::app::LithairServer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    LithairServer::new()
        .with_vhost("arcker.org",  |v| v.with_frontend_at("/", "sites/arcker.org"))
        .with_vhost("lithair.net", |v| v.with_frontend_at("/", "sites/lithair.net"))
        .with_default_vhost(|v| v.with_frontend_at("/", "sites/lithair.net"))
        .serve()
        .await
}
```

`with_default_vhost(...)` is the fallthrough for requests whose `Host` header
matches none of the declared vhosts (including IP-only access and health
probes from load balancers that don't set a hostname).

## Strict host routing (421)

By default, requests whose `Host` header matches no declared vhost (and no
default vhost) fall through to the host-agnostic pipeline and end up as
`404 Not Found`. Opt into `421 Misdirected Request` (RFC 9110 §15.5.20)
instead with `.strict_host_routing()`:

```rust
LithairServer::new()
    .with_vhost("arcker.org", |v| v.with_frontend_at("/", "sites/arcker.org"))
    .strict_host_routing() // unknown Host -> 421 instead of 404 fallthrough
    .serve()
    .await
```

Useful to surface CDN/DNS host misconfigurations instead of silently serving
fallback content. Notes: it never fires when a default vhost is set (the
default always matches), and it applies to the whole pipeline including
`/health` — register your probe host (or keep a default vhost) if load
balancers reach the server by IP.

## Host-to-host redirects

For canonical-URL enforcement (e.g. forcing `www.` to the bare domain), use
`.with_redirect("from-host", "to-host")`. This emits a declarative 301 at the
server level, no separate reverse proxy needed.

```rust
LithairServer::new()
    .with_redirect("www.arcker.org",  "arcker.org")
    .with_redirect("www.lithair.net", "lithair.net")
    .with_vhost("arcker.org",  |v| v.with_frontend_at("/", "sites/arcker.org"))
    .with_vhost("lithair.net", |v| v.with_frontend_at("/", "sites/lithair.net"))
    .serve()
    .await
```

## See also

- [The Layer I Stopped Choosing](https://arcker.org/blog/2026-04-24-lithair-vhost-routing/) — design rationale and trade-offs.
