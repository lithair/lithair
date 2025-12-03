# Schema Evolution

Strategies for safely evolving models over time.

## Techniques

- Versioned events and snapshots
- Optional fields with sensible defaults
- Upcasters/adapters for older payloads

## Process

- Additive changes preferred; avoid breaking removals
- Migrate readers first (tolerant), then writers
- Maintain test corpus of historical payloads

## See also

- Attributes reference: `../../reference/declarative-attributes.md`
- Deserializers: `../persistence/deserializers.md`
