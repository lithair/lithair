# lithair-cli

Command-line tool for scaffolding [Lithair](https://github.com/lithair/lithair) projects.

## Installation

```bash
cargo install lithair-cli
```

This installs a `lithair` binary on your `$PATH`.

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
