---
title: "Reference"
description: "Complete API reference and technical documentation for Lithair"
category: "reference"
level: "advanced"
---

# Lithair Reference Documentation

Complete technical reference for all Lithair features, APIs, and configuration options.

## API Reference

### Core References
- [API Reference](./api-reference.md) - Complete API documentation
- [Declarative Attributes](./declarative-attributes.md) - Full reference of all declarative attributes
- [Configuration Reference](../configuration-reference.md) - Environment variables and configuration
- [Configuration Matrix](../configuration-matrix.md) - Quick reference matrix for all config options

### Comparisons
- [SQL vs Lithair](./sql-vs-lithair.md) - Detailed comparison with traditional SQL approaches

## Declarative Attributes Quick Reference

### Database Attributes
```rust
#[db(primary_key)]              // Mark as primary key
#[db(unique)]                   // Enforce uniqueness
#[db(indexed)]                  // Create index
#[db(required)]                 // NOT NULL constraint
```

### Lifecycle Attributes
```rust
#[lifecycle(immutable)]         // Cannot be modified after creation
#[lifecycle(audited)]           // Automatic audit trail
#[lifecycle(versioned)]         // Enable versioning
#[lifecycle(soft_delete)]       // Soft delete support
```

### HTTP Attributes
```rust
#[http(expose)]                 // Expose in API
#[http(readonly)]               // Read-only in API
#[http(validate = "email")]     // Automatic validation
#[http(hidden)]                 // Hide from API responses
```

### Permission Attributes
```rust
#[permission(read = "UserRead")]    // Read permission required
#[permission(write = "Admin")]      // Write permission required
#[permission(delete = "Admin")]     // Delete permission required
```

### Persistence Attributes
```rust
#[persistence(replicate)]       // Enable replication
#[persistence(cached)]          // Enable caching
#[persistence(event_sourced)]   // Event sourcing
```

## Configuration Variables

### Server Configuration
- `RS_PORT` - Server port (default: 3000)
- `RS_HOST` - Server host (default: 127.0.0.1)
- `RS_ADMIN_PATH` - Admin panel path (default: /admin)
- `RS_DOCS_PATH` - Documentation path (default: ../Lithair/docs)

### Frontend Configuration
- `RS_PUBLIC_DIR` - Public frontend directory
- `RS_ADMIN_DIR` - Admin frontend directory

### Development Configuration
- `RS_DEV_RELOAD_TOKEN` - Development token (bypasses TOTP/MFA + enables hot reload) ⚠️ **DEV ONLY**

### Security Configuration
- `RS_SESSION_SECRET` - Session encryption secret
- `RS_MFA_ISSUER` - MFA/TOTP issuer name
- `RS_CORS_ORIGINS` - Allowed CORS origins

See [Configuration Reference](../configuration-reference.md) for complete list.

## Module References

Each module has detailed documentation:
- [HTTP Server Module](../modules/http-server/README.md)
- [Storage Module](../modules/storage/README.md)
- [Consensus Module](../modules/consensus/README.md)
- [Firewall Module](../modules/firewall/README.md)
- [Declarative Models Module](../modules/declarative-models/README.md)

## Quick Links

- [Main Documentation](../README.md)
- [Getting Started](../guides/getting-started.md)
- [Examples](../examples/README.md)
- [Architecture](../architecture/README.md)
