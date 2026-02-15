# 05 - E-Commerce

Multi-model e-commerce app with relations, foreign keys, and auto-joins.

## Run

```bash
cargo run -p ecommerce
# Open http://localhost:8080
```

## What you learn

- `ModelSpec` trait for field policies (unique, indexed, foreign keys)
- `RelationRegistry` for cross-model relationships
- `AutoJoiner` for automatic join queries
- `RaftstoneApplication` with multiple data sources
- Custom HTTP routes alongside auto-generated CRUD

## Models

| Model | Fields | Relations |
|-------|--------|-----------|
| Category | id, name | has many Products |
| Product | id, name, price, stock, category_id | belongs to Category |
| Customer | id, name, email | has many Orders |
| Order | id, customer_id, product_id, quantity | belongs to Customer + Product |

## API

```bash
# Categories
curl http://localhost:8080/api/categories
curl -X POST http://localhost:8080/api/categories \
  -H "Content-Type: application/json" \
  -d '{"name": "Electronics"}'

# Products (with foreign key to category)
curl http://localhost:8080/api/products
curl -X POST http://localhost:8080/api/products \
  -H "Content-Type: application/json" \
  -d '{"name": "Laptop", "price": 999.99, "stock": 10, "category_id": "<cat_id>"}'
```

## Next

Add authentication → [06-auth-sessions](../06-auth-sessions/)
