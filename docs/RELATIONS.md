# Declarative Relations & Auto-Joiner

Lithair introduces a **Declarative Relation System** that allows you to define relationships between different aggregates (tables) and automatically resolve them at read-time without writing manual join logic.

## Overview

Instead of performing expensive SQL `JOIN` operations or manual data fetching in your application layer, Lithair allows you to:

1. **Declare** relations in your `ModelSpec` (using macros).
2. **Register** data sources in a `RelationRegistry`.
3. **Expand** results automatically using the `AutoJoiner`.

This preserves the performance of the "Memory Image" pattern while providing the convenience of a relational database.

## 1. Define the Relation (Declarative)

Simply annotate your struct fields with `#[db(fk = "...")]` or `#[relation(foreign_key = "...")]`.

```rust
#[derive(DeclarativeModel, Clone, Serialize, Deserialize)]
pub struct Product {
    #[db(primary_key)]
    pub id: String,

    #[http(expose)]
    pub name: String,

    // Define a Foreign Key to the "categories" collection
    #[db(fk = "categories")]
    pub category_id: String,
}
```

The `DeclarativeModel` macro automatically generates the `ModelSpec` implementation that the Auto-Joiner needs.

## 2. Register Data Sources

At application startup, register your engines (or any struct implementing `DataSource`) into the `RelationRegistry`.

```rust
// Create the registry
let mut relation_registry = RelationRegistry::new();

// Register your engines (e.g., products and categories)
// Scc2Engine automatically implements the DataSource trait
relation_registry.register("products", product_engine.clone());
relation_registry.register("categories", category_engine.clone());
```

## 3. Use the Auto-Joiner

When serving data (e.g., in your HTTP handlers), use the `AutoJoiner` to expand the JSON response.

### Naming Convention

The Auto-Joiner uses a smart naming convention:

- If the field ends in `_id` (e.g., `category_id`), the suffix is removed (e.g., `category`).
- Otherwise, `_data` is appended (e.g., `parent` -> `parent_data`).

### Example Usage

```rust
// Retrieve a product (which has a category_id)
let product = product_engine.read("prod_1", |p| p.clone()).unwrap();

// Expand relations
// If product has { "category_id": "cat_A" }
// The result will be { "category_id": "cat_A", "category": { "id": "cat_A", "name": "Electronics" } }
let json_response = AutoJoiner::expand(
    &product,
    // Use the auto-generated spec from the struct
    Product::get_declarative_spec(),
    &relation_registry
).unwrap();

return Ok(Json(json_response));
```

## Performance Considerations

- **O(1) Lookups**: Since Lithair engines are in-memory (using SCC2 HashMap), resolving a foreign key is an extremely fast hash map lookup (nanoseconds).
- **No N+1 Problem**: While naive implementations might suffer from N+1 issues, the in-memory nature of Lithair makes these lookups negligible compared to network latency.
- **Read-Only**: The Auto-Joiner is designed for read models (projections/queries).
