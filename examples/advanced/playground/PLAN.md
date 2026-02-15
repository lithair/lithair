# Lithair Playground - Plan de Conception

> **Objectif**: Demonstrer TOUTES les capacites de Lithair dans une demo interactive de reference.
> Cette demo servira de vitrine technique et de base pour les developpements futurs.

## Vue d'ensemble

Le Lithair Playground est une application web interactive permettant de:
- Visualiser en temps reel la replication Raft
- Executer des benchmarks integres
- Controler le cluster (kill/restart nodes, forcer election)
- Tester les fonctionnalites de securite (rate limiting, firewall)
- Explorer les donnees avec CRUD en direct

---

## Architecture

```
advanced/playground/
├── Cargo.toml
├── src/
│   ├── main.rs                 # Point d'entree, setup cluster
│   ├── models.rs               # Modeles DeclarativeModel
│   ├── playground_api.rs       # Endpoints /_playground/*
│   ├── benchmark.rs            # Moteur de benchmark
│   ├── node_controller.rs      # Controle des noeuds (kill/restart)
│   └── sse_events.rs           # Server-Sent Events pour live updates
├── frontend/
│   ├── index.html              # SPA principale
│   ├── style.css               # Styles (dark theme)
│   └── app.js                  # Logique frontend
└── run_playground.sh           # Script de lancement cluster
```

---

## Fonctionnalites Lithair a Demontrer

### 1. Raft Consensus
| Feature | Demo |
|---------|------|
| Leader Election | Bouton "Force Election", visualisation du leader actuel |
| Log Replication | Compteur en temps reel (commit index, term) |
| Automatic Failover | Kill leader → observer election automatique |
| WAL (Write-Ahead Log) | Stats de persistance, taille WAL |
| Snapshots | Declenchement manuel, stats snapshot |

### 2. SCC2 Engine
| Feature | Demo |
|---------|------|
| Lock-free Operations | Benchmark concurrent (1000+ ops/sec) |
| Versioned Entries (OCC) | Affichage versions dans data explorer |
| Secondary Indexes | Recherche par index dans explorer |

### 3. Cluster Health
| Feature | Demo |
|---------|------|
| Node Status | Health check par noeud (healthy/unhealthy/desynced) |
| Replication Lag | Graphe en temps reel du lag par follower |
| Follower Sync | Progress bars de synchronisation |

### 4. Security
| Feature | Demo |
|---------|------|
| Rate Limiting | Bouton "Test Rate Limit" → voir rejection |
| Firewall (IP filter) | Configuration live des regles |
| Anti-DDoS | Stats requests blocked/allowed |
| Circuit Breaker | Visualisation etat circuit |

### 5. RBAC & Sessions
| Feature | Demo |
|---------|------|
| Role-based Access | Login avec differents roles |
| Permission Checker | Actions interdites selon role |
| Persistent Sessions | Sessions survivent aux restarts |

### 6. DeclarativeModel
| Feature | Demo |
|---------|------|
| Auto CRUD | Formulaires Create/Update/Delete |
| Validation | Erreurs de validation en temps reel |
| Replication Tracking | Badge "replicated" sur chaque entite |

### 7. Performance Metrics
| Feature | Demo |
|---------|------|
| Ops/sec | Graphe temps reel |
| Latency | Histogramme P50/P95/P99 |
| Throughput | MB/s en lecture/ecriture |

---

## API Endpoints

### Cluster Control
```
GET  /_playground/cluster/status          # Etat complet du cluster
POST /_playground/cluster/kill/:node_id   # Tuer un noeud
POST /_playground/cluster/restart/:node_id # Redemrrer un noeud
POST /_playground/cluster/force-election  # Forcer election leader
GET  /_playground/cluster/wal-stats       # Stats WAL
POST /_playground/cluster/snapshot        # Declencher snapshot
```

### Benchmark
```
POST /_playground/benchmark/start         # Demarrer benchmark
GET  /_playground/benchmark/status        # Progression/resultats
POST /_playground/benchmark/stop          # Arreter benchmark

Body start:
{
  "type": "write|read|mixed",
  "concurrency": 100,
  "duration_secs": 30,
  "payload_size": 1024
}
```

### Live Events (SSE)
```
GET  /_playground/events/replication      # Stream replication events
GET  /_playground/events/cluster          # Stream cluster state changes
GET  /_playground/events/benchmark        # Stream benchmark progress
```

### Security Testing
```
POST /_playground/security/test-rate-limit
POST /_playground/security/test-firewall
GET  /_playground/security/stats
```

### Data Operations (via DeclarativeModel)
```
GET    /api/items                         # List
POST   /api/items                         # Create
GET    /api/items/:id                     # Read
PUT    /api/items/:id                     # Update
DELETE /api/items/:id                     # Delete
```

---

## Frontend UI

### Layout Principal
```
+------------------------------------------------------------------+
|  LITHAIR PLAYGROUND                          [Node: 0] [Role: Leader]
+------------------------------------------------------------------+
|                                                                    |
|  +-- CLUSTER STATUS ----------------------------------------+     |
|  |                                                           |     |
|  |  [Node 0 - LEADER]   [Node 1 - Follower]   [Node 2 - Follower] |
|  |   ● Healthy           ● Healthy             ● Healthy     |     |
|  |   Commit: 1234        Commit: 1234          Commit: 1233  |     |
|  |                                                           |     |
|  |  [Kill Node 0] [Kill Node 1] [Kill Node 2] [Force Election]    |
|  +-----------------------------------------------------------+     |
|                                                                    |
|  +-- REPLICATION MONITOR -----------+  +-- BENCHMARK ---------+   |
|  |                                   |  |                      |   |
|  |  Ops/sec: ████████████ 15,234    |  | Type: [Write v]      |   |
|  |  Latency: ████░░░░░░░░ 2.3ms     |  | Concurrency: [100]   |   |
|  |                                   |  | Duration: [30s]      |   |
|  |  Term: 5   Commit Index: 1234    |  |                      |   |
|  |  WAL Size: 12.4 MB               |  | [START BENCHMARK]    |   |
|  |                                   |  |                      |   |
|  |  [Replication Graph - Live]      |  | Results:             |   |
|  |  ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~  |  | - Ops: 456,789      |   |
|  |  ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~  |  | - Avg: 2.1ms        |   |
|  |  ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~  |  | - P99: 8.4ms        |   |
|  +-----------------------------------+  +----------------------+   |
|                                                                    |
|  +-- DATA EXPLORER ------------------+  +-- SECURITY ----------+   |
|  |                                   |  |                      |   |
|  |  Items (42 total)                 |  | Rate Limit: 100/s   |   |
|  |  +---------------------------+    |  | [Test Rate Limit]   |   |
|  |  | ID    | Name   | Status  |    |  |                      |   |
|  |  |-------|--------|---------|    |  | Firewall: ON        |   |
|  |  | abc.. | Item 1 | active  |    |  | Blocked IPs: 3      |   |
|  |  | def.. | Item 2 | draft   |    |  | [View Rules]        |   |
|  |  +---------------------------+    |  |                      |   |
|  |                                   |  | DDoS Protection: ON |   |
|  |  [+ New Item] [Refresh]          |  | Circuit: CLOSED     |   |
|  +-----------------------------------+  +----------------------+   |
|                                                                    |
+------------------------------------------------------------------+
```

### Interactions Cles

1. **Kill Node** → API call → SSE event → UI update → Observer failover
2. **Force Election** → API call → New leader elected → All UIs update
3. **Start Benchmark** → Progress stream → Live graph update → Final results
4. **Create Item** → Replication event stream → Badge "synced" appears
5. **Test Rate Limit** → Rapid calls → See rejection counter increase

---

## Modeles de Donnees

```rust
/// Item simple pour demo CRUD + replication
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
pub struct PlaygroundItem {
    #[db(primary_key, indexed)]
    #[http(expose)]
    #[persistence(replicate, track_history)]
    #[serde(default = "uuid::Uuid::new_v4")]
    pub id: Uuid,

    #[db(indexed)]
    #[http(expose, validate = "non_empty")]
    #[persistence(replicate)]
    pub name: String,

    #[http(expose)]
    #[persistence(replicate)]
    pub description: String,

    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default)]
    pub status: ItemStatus,

    #[http(expose)]
    #[persistence(replicate)]
    pub metadata: serde_json::Value,

    #[lifecycle(immutable)]
    #[http(expose)]
    #[serde(default = "chrono::Utc::now")]
    pub created_at: DateTime<Utc>,

    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default = "chrono::Utc::now")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ItemStatus {
    #[default]
    Draft,
    Active,
    Archived,
}
```

---

## Implementation par Phases

### Phase 1: Structure de Base
- [ ] Creer structure projet (Cargo.toml, src/, frontend/)
- [ ] Modele PlaygroundItem avec DeclarativeModel
- [ ] Setup cluster 3 noeuds avec ClusterArgs
- [ ] Endpoints basiques /_raft/health existants

### Phase 2: Playground API
- [ ] /_playground/cluster/status (aggrege health de tous les noeuds)
- [ ] /_playground/cluster/kill/:node_id (signal SIGTERM au process)
- [ ] /_playground/cluster/restart/:node_id (relance le process)
- [ ] /_playground/cluster/force-election

### Phase 3: Live Events (SSE)
- [ ] /_playground/events/replication (broadcast chaque op repliquee)
- [ ] /_playground/events/cluster (changements d'etat cluster)
- [ ] Hook dans ReplicationBatcher pour emettre events

### Phase 4: Benchmark Engine
- [ ] POST /_playground/benchmark/start avec config
- [ ] Worker async qui execute le benchmark
- [ ] Metriques: ops/sec, latency histogram, throughput
- [ ] Stream progression via SSE

### Phase 5: Frontend UI
- [ ] Layout HTML avec sections (cluster, replication, benchmark, data, security)
- [ ] JavaScript pour SSE listeners
- [ ] Graphes temps reel (simple canvas ou SVG)
- [ ] Formulaires CRUD
- [ ] Boutons de controle cluster

### Phase 6: Security Demo
- [ ] Configuration firewall dans le playground
- [ ] Endpoint test rate limiting
- [ ] Affichage stats anti-DDoS
- [ ] Circuit breaker visualization

### Phase 7: Polish & Documentation
- [ ] Script run_playground.sh pour lancer 3 noeuds
- [ ] README avec instructions
- [ ] Screenshots/GIFs demo
- [ ] Integration dans examples/README.md

---

## Scripts de Lancement

### run_playground.sh
```bash
#!/bin/bash
set -e

ACTION=${1:-start}
DATA_DIR="./data"

case $ACTION in
  start)
    echo "Starting Lithair Playground Cluster..."
    mkdir -p $DATA_DIR

    # Node 0 (initial leader)
    cargo run --release --bin playground_node -- \
      --node-id 0 --port 8080 --peers 8081,8082 &

    # Node 1
    cargo run --release --bin playground_node -- \
      --node-id 1 --port 8081 --peers 8080,8082 &

    # Node 2
    cargo run --release --bin playground_node -- \
      --node-id 2 --port 8082 --peers 8080,8081 &

    echo "Cluster started!"
    echo "  - Node 0: http://localhost:8080"
    echo "  - Node 1: http://localhost:8081"
    echo "  - Node 2: http://localhost:8082"
    echo ""
    echo "Open http://localhost:8080 for the Playground UI"
    ;;

  stop)
    echo "Stopping Lithair Playground..."
    pkill -f "playground_node" || true
    ;;

  clean)
    echo "Cleaning data..."
    rm -rf $DATA_DIR
    ;;

  *)
    echo "Usage: $0 {start|stop|clean}"
    ;;
esac
```

---

## Metriques de Succes

La demo sera consideree complete quand:

1. **Fonctionnel**
   - [ ] Cluster 3 noeuds demarre en <5 secondes
   - [ ] Kill leader → nouveau leader en <3 secondes
   - [ ] CRUD operations repliquees visibles sur tous les noeuds

2. **Performance**
   - [ ] Benchmark atteint >10,000 ops/sec en write
   - [ ] Latency P99 <10ms en conditions normales
   - [ ] UI reste reactive pendant benchmarks

3. **UX**
   - [ ] Interface intuitive, pas besoin de doc pour comprendre
   - [ ] Feedback visuel immediat pour toutes les actions
   - [ ] Dark mode professionnel

4. **Reference**
   - [ ] Code bien documente, reutilisable
   - [ ] Patterns extraits pour autres projets
   - [ ] Tests de non-regression

---

## Questions Ouvertes

1. **Node Controller**: Comment kill/restart des processes depuis l'API?
   - Option A: Spawner les noeuds comme child processes du main
   - Option B: Utiliser signaux (SIGTERM/SIGKILL) + script externe
   - **Recommandation**: Option A pour demo self-contained

2. **Frontend**: Framework JS ou vanilla?
   - **Recommandation**: Vanilla JS pour zero-dependency, comme admin UI existant

3. **Graphes**: Quelle lib?
   - **Recommandation**: Canvas natif ou uPlot (leger, performant)

---

## Estimation Effort

| Phase | Effort | Priorite |
|-------|--------|----------|
| Phase 1: Structure | 2h | P0 |
| Phase 2: Playground API | 4h | P0 |
| Phase 3: Live Events | 3h | P0 |
| Phase 4: Benchmark | 4h | P1 |
| Phase 5: Frontend | 6h | P0 |
| Phase 6: Security Demo | 3h | P2 |
| Phase 7: Polish | 2h | P1 |
| **Total** | **~24h** | |

---

*Ce plan servira de reference pour l'implementation du Lithair Playground.*
