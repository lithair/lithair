# 03 - REST API

Define a struct, get a full CRUD API. No auth, no persistence config — just pure API.

## Run

```bash
cargo run -p rest-api
```

## Test

```bash
# Create a todo
curl -X POST http://localhost:8080/api/todos \
  -H "Content-Type: application/json" \
  -d '{"title": "Learn Lithair", "done": false}'

# List all
curl http://localhost:8080/api/todos

# Get one
curl http://localhost:8080/api/todos/<id>

# Update
curl -X PUT http://localhost:8080/api/todos/<id> \
  -H "Content-Type: application/json" \
  -d '{"title": "Learn Lithair", "done": true}'

# Delete
curl -X DELETE http://localhost:8080/api/todos/<id>
```

## What you learn

- `#[derive(DeclarativeModel)]` generates REST endpoints
- `#[http(expose)]` controls which fields appear in the API
- `.with_model::<Todo>(data_path, base_path)` registers the model
- Automatic GET, POST, PUT, DELETE

## Code highlights

```rust
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Todo {
    #[http(expose)]
    id: String,
    #[http(expose)]
    title: String,
    #[http(expose)]
    done: bool,
}

LithairServer::new()
    .with_model::<Todo>("./data/todos", "/api/todos")
    .serve()
    .await?;
```

One struct → five endpoints. That's it.

## Next

Build a full blog with auth → [04-blog](../04-blog/)
