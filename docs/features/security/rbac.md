# RBAC

Role-Based Access Control in Lithair is agnostic and application-defined.

- Define your own permission enum or type implementing `Permission`
- Use `RBACMiddleware<P>` and `SecurityState<P>` with your `P`
- Works with sessions or JWT for authentication

## Key points

- Generic `Permission` trait decouples framework from app business rules
- Authorization checks happen consistently in middleware/handlers
- Full audit trail via event-sourced security events

## See also

- Overview: `./overview.md`
- Guide (admin protection): `../../guides/admin-protection.md`
- RBAC demo/example: `../../examples/README.md`
