# Advanced - Consistency Test

Automated Raft consistency testing under heavy concurrent load.

## Run

```bash
cargo run -p consistency-test -- --ops 100 --concurrency 10
```

## What it does

1. Launches a 3-node cluster automatically
2. Runs concurrent CRUD operations across all nodes
3. Verifies data consistency after operations complete
4. Reports detailed results (success/failure rates, latency)

## Options

```
--ops <N>           Operations per table (default: 100)
--concurrency <N>   Concurrent workers (default: 10)
--with-updates      Include UPDATE operations (default: true)
--with-deletes      Include DELETE operations (default: true)
```

## Purpose

This is a testing tool, not a learning example.
Use it to validate that Raft replication maintains consistency under load.
