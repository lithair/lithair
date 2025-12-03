# Frontend Overview

Lithair supports multiple frontend serving strategies with a focus on performance and DX.

- Memory-first serving using SCC2 for production performance
- Development mode for instant asset updates from disk
- Hybrid mode: production-level performance with API-triggered reload
- Multiple frontends (e.g., public + admin) via virtual hosts

## Key concepts

- Virtual hosts: map multiple frontend roots to paths
- Asset discovery: `public/`, `frontend/public/`, `static/`, `assets/`
- MIME detection for correct Content-Type headers
- Atomic reload (hybrid): load new assets, swap, continue serving

## Related guides

- Serving modes: `../../guides/serving-modes.md`
- Environment variables: `../../reference/env-vars.md`
