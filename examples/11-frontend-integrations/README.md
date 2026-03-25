# 11 - Frontend Framework Integrations

One Rust backend serving the same Notes CRUD API, paired with five different frontend frameworks.
Pick the framework you know (or want to learn) and follow the same pattern.

## Architecture

```
cargo run -p frontend-integrations -- --frontend react
                     |
          LithairServer (port 8080)
          /api/notes  -> SCC2 in-memory CRUD
          /*          -> static files from <framework>/dist/
```

## Frameworks

| Framework | Directory | Build command | Dev command | Dist path |
|-----------|-----------|--------------|-------------|-----------|
| React 19 | `react/` | `npm run build` | `npm run dev` (Vite proxy) | `react/dist` |
| Angular 19 | `angular/` | `npm run build` | `npm start` (ng proxy) | `angular/dist/angular/browser` |
| Vue 3.5 | `vue/` | `npm run build` | `npm run dev` (Vite proxy) | `vue/dist` |
| Svelte 5 | `svelte/` | `npm run build` | `npm run dev` (Vite proxy) | `svelte/dist` |
| Astro 5 | `astro/` | `npm run build` | `npm run dev` | `astro/dist` |

## Quick Start

```bash
# 1. Build a frontend (e.g., Vue)
cd examples/11-frontend-integrations/vue
npm install
npm run build

# 2. Start the server (from repo root)
cargo run -p frontend-integrations -- --frontend vue

# 3. Open http://127.0.0.1:8080
```

## Dev Mode (hot reload)

Each frontend has a dev server that proxies `/api` requests to the Rust backend on port 8080.

```bash
# Terminal 1: Rust backend
cargo run -p frontend-integrations -- --frontend react

# Terminal 2: Frontend dev server (e.g., React on port 5173)
cd examples/11-frontend-integrations/react
npm install
npm run dev
```

Open the dev server URL (usually `http://localhost:5173`) for hot reload.

## CLI Options

```
--frontend <name>   Framework to serve: react, angular, vue, svelte, astro (default: react)
--port <number>     Port to listen on (default: 8080)
```
