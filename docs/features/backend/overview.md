# Backend Overview

Core backend building blocks and patterns in Lithair.

- Declarative models drive CRUD endpoints and validation
- HTTP server (Hyper) with consistent handler signatures
- Route composition (static + generated) and middleware
- Built-in responses helpers (`.text()`, `.json()`, `.html()`)
- Type aliases for response ergonomics (see modules/http-server)

## Key concepts

- Handlers: consistent signatures using `Request`, `PathParams`, and response helpers
- Router patterns: parameter parsing, guards, error handling
- Generated CRUD: derive from models with minimal boilerplate

## See also

- Server module: `../../modules/http-server/README.md`
- Declarative models: `../../modules/declarative-models/README.md`
- Examples: `../../examples/README.md`
