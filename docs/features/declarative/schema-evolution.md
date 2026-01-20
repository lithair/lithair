# Schema Evolution

Strategies for safely evolving DeclarativeModel schemas over time.

## Best Practices

### Prefer Additive Changes

- Add new nullable fields (`Option<T>`) - always safe
- Add new indexes - improves query performance
- Add new computed fields - no data migration needed

### Avoid Breaking Changes

- Removing fields requires data migration
- Changing field types needs conversion logic
- Renaming fields breaks backward compatibility

### Migration Strategies

| Strategy | When to Use | Consensus |
|----------|-------------|-----------|
| Additive | New nullable field | No |
| Versioned | Type change with converter | Partial |
| Breaking | Remove field, add NOT NULL | Yes |

## Techniques

- **Versioned snapshots**: Different rkyv schemas per version
- **Default values**: Provide defaults for new required fields
- **Upcasters**: Convert old payloads to new format on read
- **Soft deletes**: Mark deprecated, remove on compaction

## Process

1. Increment schema version: `#[schema(version = 2)]`
2. Detect changes at startup
3. For additive: auto-migrate
4. For breaking: coordinate cluster consensus
5. Maintain rollback capability

## See Also

- [Migration System Overview](../migration/overview.md) - Complete architecture
- [Schema Detection](../migration/schema-detection.md) - How changes are detected
- [Execution Flow](../migration/execution-flow.md) - Migration lifecycle
- [Roadmap](../migration/roadmap.md) - Current gaps and implementation plan
- Attributes reference: `../../reference/declarative-attributes.md`
