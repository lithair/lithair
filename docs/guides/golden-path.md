# The Lithair golden path

This is the shortest supported journey from an empty directory to a durable,
operable Lithair application. It deliberately starts with a single node and a
single model. Authentication, retention, clustering, and other capabilities
come after this path works.

Lithair's core promise is simple: **a Rust model becomes a stateful, durable,
operable application**.

## Check the fit

Lithair is a strong fit when the active dataset fits comfortably in memory,
fast reads matter, an event history is valuable, and reducing infrastructure
is more useful than retaining a SQL ecosystem. Start with another architecture
when ad-hoc relational analytics, write-heavy workloads, or datasets that
cannot be bounded in memory are the primary requirement.

Read [capacity planning](../operations/capacity-planning.md) before committing
a non-trivial production dataset.

## 1. Create the application

Install the CLI and generate a project:

```bash
cargo install lithair-cli
lithair new my-app
cd my-app
```

The generated application contains one `Item` model, its REST API, a custom
health route, an optional static frontend, metrics, and a local data directory.

## 2. Reach the first success

Run it:

```bash
cargo run
```

In another terminal, create and read an item:

```bash
curl -X POST http://127.0.0.1:3000/api/items \
  -H 'content-type: application/json' \
  -d '{"id":"first","name":"My first item","description":"Created with Lithair"}'

curl http://127.0.0.1:3000/api/items/first
```

This is the minimal Lithair model: writes append durable events and reads use
the reconstructed in-memory state.

## 3. Prove durability, do not merely assume it

Stop the server with `Ctrl-C`, start it again with `cargo run`, and repeat the
GET request. The item must still be present after replay.

Stop the server again and verify the store offline:

```bash
lithair verify ./data/items
```

The command exits with status 0 when the event-store hash chain is valid. This
same verification belongs in a restore procedure before a restored server is
started.

## 4. Make the model yours

Change `src/models/item.rs`, then add tests at the lowest useful level:

- unit tests for local invariants;
- Rust integration tests for HTTP and storage contracts;
- Gherkin scenarios for user-visible promises spanning several components;
- compile-fail tests when extending Lithair's declarative macro language.

The project's own rules and commands are documented in
[Testing](../TESTING.md). Gherkin is a contract tool, not a requirement for
every implementation detail.

## 5. Add capabilities by intent

Only introduce the next concept when the application needs it:

| Need | Next capability |
|---|---|
| Protect users and endpoints | [Sessions](../features/sessions.md) and [RBAC](rbac.md) |
| Bound the in-memory working set | [Retention](../features/retention.md) |
| Stream live changes | [SSE](../features/sse.md) |
| Prepare production operations | [Observability](../operations/observability.md) and [backup/restore](../operations/backup-restore.md) |
| Run several nodes | [Cluster operations](../operations/cluster.md) |

Clustering is intentionally last. A single-node application exercises the
same model and storage fundamentals with fewer operational variables.

## 6. Ship deliberately

Before the first production deployment:

1. estimate memory and disk growth with the capacity guide;
2. configure health, readiness, metrics, and graceful shutdown;
3. automate backups and perform a real restore drill;
4. run `lithair verify` against the restored store;
5. rehearse the documented [upgrade procedure](../operations/upgrade.md).

The golden path is complete when the application can be created, exercised,
restarted, verified, tested, backed up, restored, and upgraded without relying
on undocumented maintainer knowledge.
