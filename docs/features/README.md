---
title: "Features"
description: "Comprehensive overview of Lithair features and capabilities"
category: "features"
level: "intermediate"
---

# Lithair Features

Lithair provides a comprehensive set of features for building modern, scalable web applications with Rust.

## Core Features

### Declarative Models

Define your data models once with powerful attributes that automatically generate:

- Database schemas and migrations
- REST API endpoints with CRUD operations
- Business logic and validation
- Security and permission controls
- Event sourcing and audit trails

### HTTP Server

High-performance HTTP server built on Hyper with:

- Automatic API generation from models
- Frontend serving (development and production modes)
- Route guards and middleware
- Session management
- CORS and security headers

### RBAC (Role-Based Access Control)

Comprehensive authentication and authorization system:

- Role definitions with hierarchical permissions
- Session management with secure cookies
- MFA/TOTP support (Google Authenticator, Authy, etc.)
- Declarative route protection
- User management and authentication endpoints

### Storage & Persistence

Flexible storage system supporting:

- File-based storage with event sourcing
- Replication with Raft consensus
- In-memory caching
- Transaction support
- Audit logging

### Frontend Integration

Seamless frontend integration with:

- Multiple frontend builds (public, admin)
- Memory-first asset serving
- Hot reload in development mode
- Production optimization
- Static site generation support

### Performance & Security

Production-ready features:

- HTTP hardening and rate limiting
- IP-based firewall
- Gzip compression
- Performance benchmarking endpoints
- Load testing capabilities

## Feature Matrix

| Feature          | Status    | Documentation                                                                                                                                                                                                                             |
| ---------------- | --------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Frontend**     | ✅ Stable | [Overview](./frontend/overview.md) · [Modes](./frontend/modes.md)                                                                                                                                                                         |
| **Backend**      | ✅ Stable | [Overview](./backend/overview.md) · [HTTP](./backend/http.md)                                                                                                                                                                             |
| **Security**     | ✅ Stable | [Overview](./security/overview.md) · [RBAC](./security/rbac.md) · [Firewall](./security/firewall.md) · [Sessions](./security/sessions.md)                                                                                                 |
| **Persistence**  | ✅ Stable | [Overview](./persistence/overview.md) · [Event Store](./persistence/event-store.md) · [Snapshots](./persistence/snapshots.md) · [Deserializers](./persistence/deserializers.md) · [Optimized Storage](./persistence/optimized-storage.md) |
| **State Engine** | ✅ Stable | [Overview](./state-engine/overview.md) · [SCC2](./state-engine/scc2.md) · [Lock-free](./state-engine/lockfree.md)                                                                                                                         |
| **Declarative**  | ✅ Stable | [Overview](./declarative/overview.md) · [Schema Evolution](./declarative/schema-evolution.md)                                                                                                                                             |
| **Clustering**   | ✅ Stable | [Overview](./clustering/overview.md) · [OpenRaft](./clustering/openraft.md)                                                                                                                                                               |

## Quick Links

- [Main Documentation](../README.md)
- [Getting Started](../guides/getting-started.md)
- [Examples](../examples/README.md)
- [API Reference](../reference/api-reference.md)

## Author

Yoan Roblet [Arcker](https://github.com/arcker)
