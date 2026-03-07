# Lithair vs SQL Databases: Technical Comparison

## Executive Summary

This document provides a technical comparison between Lithair's
event-sourcing architecture and traditional SQL databases
(PostgreSQL, MariaDB, SQLite). The performance ranges below are best
read as architectural expectations to validate with reproducible
benchmarks on your own workload.

## Historical Context: SQL Architecture Origins

### SQL Database Design Era (1970-1985)

- **Hardware Context**: 64KB-1MB RAM, 5-50MB disk storage, single-core CPUs
- **Design Priorities**: Minimize memory usage, optimize disk I/O, maximize
  data compaction
- **Architecture**: Page-based storage (8KB-16KB pages), B-tree indexes,
  complex buffer management

### Modern Hardware Reality (2025)

- **RAM**: 16-128GB standard (1000x increase)
- **Storage**: 1-10TB NVMe SSDs (200,000x increase)
- **CPU**: 8-64 cores, 3-5GHz (10,000x increase)
- **Network**: 1-100Gbps (100,000,000x increase)

## Architectural Comparison

### SQL Database Write Operation (PostgreSQL Example)

```text
1. Parse SQL statement (1-5ms)
2. Query planning and optimization (1-10ms)
3. Acquire locks on affected rows/pages (0.1-5ms)
4. Read target page(s) from disk to buffer pool (1-10ms)
5. Modify page in memory (0.1-1ms)
6. Write to WAL (Write-Ahead Log) (1-5ms)
7. Update all affected indexes (1-20ms)
8. Write modified page back to disk (1-10ms)
9. Release locks (0.1-1ms)
10. Commit transaction (1-5ms)

Total: 7-72ms per write operation
```

### Lithair Write Operation

```text
1. Serialize event to JSON (0.01-0.1ms)
2. Append to events.raftlog file (0.1-1ms)
3. Apply event to in-memory state (0.01-0.1ms)

Total: 0.12-1.2ms per write operation
```

**Illustrative ratio**: in this simplified comparison, Lithair can be
substantially faster for write operations when the workload matches this
append-only, in-memory model.

### SQL Database Read Operation (PostgreSQL Example)

```text
1. Parse SQL statement (1-5ms)
2. Query planning (1-10ms)
3. Index lookup via B-tree traversal (1-5ms)
4. Page retrieval from buffer pool or disk (0.1-10ms)
5. Row filtering and projection (0.1-2ms)
6. Result serialization (0.1-2ms)

Total: 3-34ms per read operation
```

### Lithair Read Operation

```text
1. Direct memory access to Rust struct (0.001-0.01ms)
2. Data serialization if needed (0.01-0.1ms)

Total: 0.011-0.11ms per read operation
```

**Illustrative ratio**: Lithair can be dramatically faster for read
operations when the requested state is already resident in memory.

## Storage Architecture Comparison

### SQL Database Storage (PostgreSQL)

- **Page Size**: Fixed 8KB pages
- **Write Pattern**: Random writes (pages scattered across disk)
- **Fragmentation**: Inevitable over time, requires VACUUM
- **Index Overhead**: Multiple B-tree indexes per table
- **Storage Efficiency**: High (binary format, normalized data)

### Lithair Storage

- **Write Pattern**: Sequential append-only writes
- **Fragmentation**: None (append-only by design)
- **Index Overhead**: None (in-memory indexes rebuilt on startup)
- **Storage Efficiency**: Lower (JSON format, denormalized events)
- **Corruption Resistance**: High (immutable append-only log)

## Complexity Analysis

### PostgreSQL Operational Complexity

- **Configuration Parameters**: 300+ tunable parameters
- **Required Expertise**: Database Administrator (DBA) role
- **Monitoring Points**: 50+ critical metrics (connections, locks, buffer hit
  ratio, etc.)
- **Backup Strategy**: Complex (pg_dump, pg_basebackup, PITR)
- **High Availability**: Replication and failover procedures
- **Maintenance**: Regular VACUUM, ANALYZE, index rebuilding

### Lithair Operational Complexity

- **Configuration Parameters**: <10 parameters
- **Required Expertise**: Often manageable by a standard backend developer
- **Monitoring Points**: 5-10 basic metrics
- **Backup Strategy**: File-level backup of the event log and snapshots
- **High Availability**: Built-in Raft consensus
- **Maintenance**: Automatic compaction with a lighter manual ops surface

## Why SQL Architectures Remain Dominant

### Technical Reasons

1. **Backward Compatibility**: 40+ years of SQL standard compliance
2. **Ecosystem Lock-in**: Thousands of tools, ORMs, and integrations
3. **Workload Diversity**: Must support OLTP, OLAP, and mixed workloads
4. **Standards Compliance**: ACID properties, SQL standard adherence

### Business Reasons

1. **Risk Aversion**: Enterprise customers prefer proven technology
2. **Investment Protection**: Existing DBA skills and tooling
3. **Commercial Ecosystems**: Tooling, support, and training are built around
   established database models
4. **Market Inertia**: "Nobody gets fired for choosing PostgreSQL"

### Innovation Attempts

Several databases have attempted modernization:

- **CockroachDB**: Distributed SQL with modern storage
- **FoundationDB**: Key-value store with ACID properties
- **Apache Cassandra**: Wide-column store with eventual consistency
- **MongoDB**: Document store with flexible schema

However, they reflect different trade-offs around query models,
consistency semantics, and operational complexity.

## Lithair Design Decisions

### Trade-offs Accepted

1. **Storage Size**: Often larger files than compact SQL storage engines,
   especially with JSON events
2. **Query Flexibility**: Event sourcing vs arbitrary SQL queries
3. **Ecosystem**: New tooling vs mature SQL ecosystem
4. **Learning Curve**: New concepts vs familiar SQL

### Benefits Gained

1. **Performance**: Strong performance for in-memory, event-sourced workloads
2. **Simplicity**: Smaller operational surface in common deployments
3. **Reliability**: Append-only logs can reduce some corruption scenarios
4. **Auditability**: Complete event history by design
5. **Development Speed**: Less plumbing for common application patterns

## Benchmark Methodology

### Test Environment

- **Hardware**: Modern server (32GB RAM, NVMe SSD, 8-core CPU)
- **PostgreSQL**: Version 15+ with standard configuration
- **Lithair**: Current implementation
- **Workload**: E-commerce operations (product CRUD, user management)

### Measurable Metrics

1. **Latency**: P50, P95, P99 response times
2. **Throughput**: Operations per second
3. **Resource Usage**: CPU, memory, disk I/O
4. **Operational Complexity**: Configuration parameters, monitoring points
5. **Development Time**: Time to implement features

## Addressing Common Objections

### "SQL is battle-tested"

**Response**: SQL remains battle-tested across a very broad range of
workloads. Lithair is a better fit when you want a memory-first,
event-sourced model and accept its trade-offs.

### "Storage efficiency matters"

**Response**: Storage efficiency can still matter. Lithair simply makes a
different trade-off by spending more memory and storage to reduce some
runtime and operational complexity.

### "What about complex queries?"

**Response**: Lithair is strongest when your query patterns are known in
advance or can be served by projections. More exploratory or relational
query workloads may still favor SQL.

### "Ecosystem maturity"

**Response**: Lithair prioritizes a smaller and more focused surface area.
That can help some teams move faster, while others will still prefer the
broader SQL ecosystem.

## Conclusion

Lithair represents a different architectural choice optimized for:

- Modern hardware capabilities (abundant RAM, fast SSDs)
- Developer productivity over storage efficiency in the targeted model
- Operational simplicity over feature completeness
- Audit trail and replayability over broad query flexibility

For teams whose workload aligns with those priorities, it can feel like a
substantial shift in how backend applications are assembled and operated.

---

**Note**: The performance claims in this document should be validated
through reproducible benchmarks that match your own deployment profile.
