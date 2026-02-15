# Advanced - DataTable

Multi-table relational demo with DeclarativeModel, event sourcing, and frontend.

## Run

```bash
cargo run -p datatable -- --port 8080
```

## Models

- **Product** — catalog items with name, price, stock, category
- **Consumer** — customer accounts with email
- **Order** — relations between consumers and products (with quantity and status)

## What it demonstrates

- Multiple DeclarativeModel structs in one server
- SCC2 frontend engine serving an interactive UI
- Event-sourced persistence for all models
- Admin panel with data visualization
- Seed data generation for testing

## Purpose

Sandbox for experimenting with Lithair's multi-model capabilities.
Good starting point for building data-heavy applications.
