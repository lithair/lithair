# Live updates over SSE

Every model registered with `with_model::<T>(...)` automatically exposes a
`GET /api/{model}/stream` endpoint that streams creates, updates, and deletes
to connected clients as Server-Sent Events. Writes made through the REST API,
through the programmatic handler (`with_model_ref`), or replicated from a
peer node all broadcast on the same channel — subscribers don't need to know
which path produced the change.

SSE is opt-in: enable it on the builder with `.with_sse(true)`. Without that
call, the `/stream` endpoint is not registered and no broadcaster is allocated.

## Server-side

```rust
use lithair_core::app::LithairServer;
use lithair_core::DeclarativeModel;
use serde::{Serialize, Deserialize};

#[derive(DeclarativeModel, Serialize, Deserialize, Clone, Debug)]
struct Article {
    #[http(expose)]
    id: String,
    #[http(expose)]
    title: String,
    #[http(expose)]
    body: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    LithairServer::new()
        .with_sse(true)
        .with_model::<Article>("./data/articles", "/api/articles")
        .serve()
        .await
}
```

## Client-side

```js
const es = new EventSource('/api/articles/stream');
es.onmessage = (event) => {
    const change = JSON.parse(event.data);
    // { kind: "create" | "update" | "delete", item: { ... } }
    console.log(change);
};
```

## Programmatic broadcasts

When you use `with_model_ref::<T>(...)` instead of `with_model::<T>(...)`, you
get back a handler that shares the same SSE broadcaster as the auto-generated
REST routes. Calls to `apply_replicated_item`, `apply_replicated_update`, and
`apply_replicated_delete` on that handler reach SSE subscribers exactly as if
they had come in over HTTP. This is how replication, background workers, and
custom routes all stay coherent with REST writes from a single client's point
of view.

## See also

- [`examples/04-blog/`](https://github.com/lithair/lithair/tree/main/examples/04-blog) — blog example exercising the SSE stream end-to-end.
- CHANGELOG entries for v0.7.0 (initial SSE), v0.9.0 (broadcaster wired through `with_handler`), v0.11.0 (incremental streaming fix) for the evolution arc.
