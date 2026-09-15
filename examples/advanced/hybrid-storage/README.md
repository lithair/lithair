# Hybrid storage: native tasks and SQL archives

This prototype serves two independent models in one Lithair process:

| Endpoint | Authority | Access |
|---|---|---|
| `/api/tasks` | Native memory-first state and event log | Public demo model |
| `/api/archives` | Embedded Turso SQL database | Explicit bearer token |

The archive routes use a typed `lithair-turso` repository with existing model
validation and permission hooks. The example intentionally uses custom routes;
there is no `with_sql_model` macro/builder API yet. See the
[RFC](../../../docs/rfcs/235-hybrid-storage.md) and
[adapter contract](../../../lithair-turso/README.md).

## Run

Set `ARCHIVE_TOKEN` to a non-empty secret, then run:

```bash
cargo run -p hybrid-storage
```

`PORT` defaults to 8080, `DATA_DIR` to `./data/hybrid`. The example binds loopback.
`Ctrl-C` requests graceful shutdown. Reusing `DATA_DIR` preserves both stores.
Archive credentials are configured by the application, not stored in either model.
A production app should map its authenticated principal to repository permissions.
The native tasks API is deliberately public; do not use it for private data.

## Try the two stores

With the same `ARCHIVE_TOKEN` set in your client shell:

```bash
curl -X POST http://127.0.0.1:8080/api/tasks \
  -H 'Content-Type: application/json' \
  -d '{"id":"live-1","title":"Process incoming mail"}'

curl -X POST http://127.0.0.1:8080/api/archives \
  -H "Authorization: Bearer $ARCHIVE_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"id":"saved-1","title":"Completed import","category":"work"}'

curl 'http://127.0.0.1:8080/api/archives?category=work&limit=10&offset=0' \
  -H "Authorization: Bearer $ARCHIVE_TOKEN"

curl 'http://127.0.0.1:8080/api/archives?id=saved-1' \
  -H "Authorization: Bearer $ARCHIVE_TOKEN"

curl -X PUT 'http://127.0.0.1:8080/api/archives?id=saved-1' \
  -H "Authorization: Bearer $ARCHIVE_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"id":"saved-1","title":"Reviewed import","category":"work"}'

curl -X DELETE 'http://127.0.0.1:8080/api/archives?id=saved-1' \
  -H "Authorization: Bearer $ARCHIVE_TOKEN"
```

Archive GET returns `{"data": ...}`; list returns an array without an unfiltered
total. Empty titles fail validation (400), duplicate IDs fail (409), missing IDs
return 404 and absent/incorrect archive credentials return 401. The SQL repository
also enforces the model's `ArchiveRead` / `ArchiveWrite` checks. Requests are limited
to 1 MiB of JSON. No HTTP endpoint exposes SQL or lets a client choose a namespace.

Operations spanning tasks and archives are not atomic. Native history/replication
and retention do not automatically apply to archives. Avoid unsupported annotations
on SQL models; this prototype does not implement a general ORM.

The executable Gherkin scenario in `features/persistence/turso.feature` covers both
HTTP models, authorization, validation, updates and restart. Run `cidx run test`.
