---
title: "Architecture"
description: "System architecture and design principles of Lithair"
category: "architecture"
level: "advanced"
---

# Lithair Architecture

Understanding the architectural foundations and design principles of the Lithair framework.

## Architecture Documentation

### System Design
- [Overview](./overview.md) - General architecture overview with diagrams
- [System Design](./system-design.md) - Design principles and architectural patterns
- [Data Flow](./data-flow.md) - How data flows through the system

## Architectural Principles

### Data-First Philosophy
Lithair is built on the principle that data models should be the single source of truth. All other concerns (API, storage, security, validation) are derived from the data model definition.

### Declarative Over Imperative
Instead of writing boilerplate code for common patterns, Lithair uses declarative attributes to express intent. The framework handles the implementation details.

### Performance by Design
Architecture optimized for performance with:
- Memory-first caching strategies
- Zero-copy operations where possible
- Async/await throughout
- Minimal allocations in hot paths

### Security by Default
Security considerations built into the core:
- RBAC integrated at the framework level
- Session management with secure defaults
- Input validation and sanitization
- Audit logging for sensitive operations

### Modular Architecture
Clean separation of concerns with independent modules:
- HTTP server module
- Storage and persistence module
- Consensus and replication module
- Security and firewall module
- Frontend serving module

## System Layers

```
┌─────────────────────────────────────────┐
│        Application Layer                 │
│  (Your Models & Business Logic)         │
├─────────────────────────────────────────┤
│         Framework Layer                  │
│  (Lithair Core - Auto-generated)      │
├─────────────────────────────────────────┤
│          Module Layer                    │
│  (HTTP, Storage, RBAC, Consensus)       │
├─────────────────────────────────────────┤
│       Infrastructure Layer               │
│  (Hyper, Tokio, Filesystem)             │
└─────────────────────────────────────────┘
```

## Key Components

### LithairServer
Central server orchestrator managing:
- HTTP routing and middleware
- Frontend serving
- Model registration
- RBAC integration
- Session management

### DeclarativeModel System
Attribute-driven model system providing:
- Automatic CRUD generation
- Validation and business rules
- Permission checking
- Event sourcing
- Replication

### Storage Engine
Flexible persistence layer with:
- Event sourcing patterns
- Raft consensus for replication
- In-memory caching
- Transaction support

## Quick Links

- [Main Documentation](../README.md)
- [Modules Documentation](../modules/README.md)
- [Developer Guide](../guides/developer-guide.md)
- [Performance Guide](../guides/performance.md)
