# Hybrid storage: declarative native + Turso models

`LiveTask` uses native memory-first/event-sourced storage. `Archive` adds
`#[storage(turso, collection = "archives_v1", filters("category"))]` to the usual
`DeclarativeModel`. Both are registered with `with_model`; the framework opens
storage and provides their HTTP routes automatically.

The example seeds a one-hour in-memory session from `ARCHIVE_TOKEN`. Tasks are
public; a declarative route guard protects archives. A production application can
use the normal RBAC login/session configuration instead of this demo credential.

```bash
ARCHIVE_TOKEN=local-example-token cargo run -p hybrid-storage
```

`PORT` defaults to 8080, `DATA_DIR` to `./data/hybrid`. The server binds loopback.
Native tasks write under `tasks/`; archives use `archives/model.db`.

```bash
curl -X POST http://127.0.0.1:8080/api/tasks \
  -H 'Content-Type: application/json' \
  -d '{"id":"live-1","title":"in memory"}'

curl -X POST http://127.0.0.1:8080/api/archives \
  -H 'Authorization: Bearer local-example-token' \
  -H 'Content-Type: application/json' \
  -d '{"id":"archive-1","title":"stored in SQL","category":"work"}'

curl 'http://127.0.0.1:8080/api/archives?category=work&limit=25&offset=0' \
  -H 'Authorization: Bearer local-example-token'

curl -X PATCH http://127.0.0.1:8080/api/archives/archive-1 \
  -H 'Authorization: Bearer local-example-token' \
  -H 'Content-Type: application/json' -d '{"title":"updated"}'

curl -X DELETE http://127.0.0.1:8080/api/archives/archive-1 \
  -H 'Authorization: Bearer local-example-token'
```

Restart with the same data directory and token to read committed documents.
Archive validation, model permissions, pagination and storage errors are enforced
by the generated adapter. No manual `SqlModel` implementation or custom CRUD
handlers are present in the application.

Turso support is an experimental crate (`lithair-turso` 0.2, with Lithair 1.12). It supports one
SQL authority per model; native retention, history, replication, SSE and backups
do not apply. Unsupported native annotations fail to compile. See the
[adapter contract](../../../lithair-turso/README.md) and [RFC](../../../docs/rfcs/235-hybrid-storage.md).
For a separate repository, use the
[standalone application guide](../../../docs/guides/turso-in-an-application.md).

`cidx run test` includes the mixed HTTP/restart Gherkin scenario;
`cidx run ci` validates the full workspace.
