# lithair-cli

Command-line tool for scaffolding [Lithair](https://github.com/lithair/lithair) projects.

## Installation

```bash
cargo install lithair-cli
```

This installs a `lithair` binary on your `$PATH`.

## Experimental offline cluster tools

From a checkout containing the new consensus foundation:

```bash
cargo install --path lithair-cli --features cluster-ops
lithair cluster check --config /etc/lithair/node.toml
lithair cluster provision --config /etc/lithair/node.toml
lithair cluster inspect --config /etc/lithair/node.toml
```

These commands validate local configuration/TLS, explicitly provision an empty
identity-bound consensus store, and inspect it offline without repairing it.
They print JSON on success and exit 2 on error. They do not start the application
or initialize a cluster. See the [configuration and operations contract](../docs/internal/specs/OPENRAFT_OPERATOR.md)
for the three-peer TOML format, prerequisites, limits and bootstrap preflight.

## Usage

### Create a new project

```bash
lithair new my-app
```

This generates a ready-to-run project with the standard Lithair structure:

```
my-app/
├── Cargo.toml              # lithair-core + lithair-macros dependencies
├── .env                    # LT_PORT, LT_HOST, LT_LOG_LEVEL, LT_DATA_DIR
├── .env.example            # Same, with comments
├── .gitignore              # target/, data/, .env
├── README.md               # Getting started guide
├── src/
│   ├── main.rs             # LithairServer entry point
│   ├── models/
│   │   ├── mod.rs          # Module declarations
│   │   └── item.rs         # Example model
│   ├── routes/
│   │   ├── mod.rs          # Module declarations
│   │   └── health.rs       # GET /health handler
│   └── middleware/
│       └── mod.rs          # Ready for custom middleware
├── frontend/               # Static assets
│   ├── index.html
│   ├── css/styles.css
│   └── js/app.js
└── data/
    └── .gitkeep            # Runtime event store directory
```

### API-only project (no frontend)

```bash
lithair new my-api --no-frontend
```

Skips the `frontend/` directory for backend-only services.

### Run the generated project

```bash
cd my-app
cargo run
```

The server starts at `http://127.0.0.1:3000` with an admin panel and metrics enabled.

## Project name rules

The project name is used as both the directory name and the Cargo package name. It must:

- Contain only ASCII alphanumeric characters, hyphens (`-`), or underscores (`_`)
- Not start with `.` or `-`
- Not contain path separators (`/`, `\\`, `..`)

## License

Licensed under either of [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
or [MIT license](http://opensource.org/licenses/MIT) at your option.
