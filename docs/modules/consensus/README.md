# Module Consensus Raft

Le module de consensus Raft de Lithair assure la cohérence des données dans un environnement distribué, avec une intégration transparente dans le système d'event sourcing.

## 🎯 Vue d'Ensemble

Le consensus Raft dans Lithair permet de maintenir un état cohérent entre plusieurs nœuds, garantissant que toutes les modifications de données sont appliquées dans le même ordre sur tous les nœuds du cluster.

```mermaid
flowchart TD
    subgraph "Cluster Raft (3 nœuds)"
        L[Leader Node]
        F1[Follower 1] 
        F2[Follower 2]
    end
    
    Client --> L
    L --> F1
    L --> F2
    
    L -->|Heartbeats| F1
    L -->|Heartbeats| F2
    F1 -->|Ack| L
    F2 -->|Ack| L
    
    subgraph "Event Log Replication"
        E1[Event 1: ProductCreated]
        E2[Event 2: PriceUpdated] 
        E3[Event 3: StockChanged]
    end
    
    L --> E1
    E1 --> E2
    E2 --> E3
```

## ⚙️ Architecture du Consensus

### États des Nœuds

```mermaid
stateDiagram-v2
    [*] --> Follower
    
    Follower --> Candidate : Election timeout
    Candidate --> Leader : Majority votes
    Candidate --> Follower : Higher term discovered
    
    Leader --> Follower : Higher term discovered
    Follower --> Follower : Receive valid heartbeat
    
    note right of Leader
        - Gère les requêtes clients
        - Envoie les heartbeats  
        - Réplique les logs
    end note
    
    note right of Follower
        - Répond aux heartbeats
        - Vote aux élections
        - Applique les logs
    end note
    
    note right of Candidate
        - Démarre élection
        - Demande votes
        - Timeout → nouvelle élection
    end note
```

### Flux de Réplication

```mermaid
sequenceDiagram
    participant C as Client
    participant L as Leader
    participant F1 as Follower 1
    participant F2 as Follower 2

    C->>L: POST /api/products (Create Product)
    L->>L: Create Event + Log Entry
    
    par Parallel Replication
        L->>F1: AppendEntries(event)
        L->>F2: AppendEntries(event)
    end
    
    F1-->>L: AppendEntries OK
    F2-->>L: AppendEntries OK
    
    Note over L: Majority achieved (2/3)
    
    L->>L: Commit Event
    L->>L: Apply to State Machine
    
    par Notify Commit
        L->>F1: Commit Index Updated
        L->>F2: Commit Index Updated
    end
    
    F1->>F1: Apply Event
    F2->>F2: Apply Event
    
    L-->>C: 201 Created (Success)
```

## 🔧 Configuration

### Configuration Basique

```rust
use lithair_core::consensus::RaftConfig;

let raft_config = RaftConfig {
    node_id: 1,
    cluster_nodes: vec![
        "127.0.0.1:8080".to_string(),
        "127.0.0.1:8081".to_string(), 
        "127.0.0.1:8082".to_string(),
    ],
    data_dir: "./data/node1".into(),
    
    // Timings
    election_timeout_ms: 300,
    heartbeat_interval_ms: 50,
    
    // Performance
    max_payload_entries: 100,
    snapshot_threshold: 10000,
    
    ..Default::default()
};
```

### Intégration avec DeclarativeModel

```rust
#[derive(DeclarativeModel)]
#[consensus(
    enabled = true,
    node_id = 1,
    cluster_size = 3,
    data_dir = "./consensus_data"
)]
pub struct DistributedProduct {
    #[db(primary_key, indexed)]
    #[http(expose)]
    #[persistence(replicate, consistent_read)]
    pub id: Uuid,
    
    #[http(expose, validate = "non_empty")]
    #[persistence(replicate, track_history)]
    #[lifecycle(audited)]
    pub name: String,
    
    #[http(expose, validate = "min_value(0.01)")]
    #[persistence(replicate, consistent_read)]
    pub price: f64,
}
```

## 📊 Algorithme Raft Détaillé

### 1. Élection du Leader

```mermaid
flowchart TD
    A[Nœud Follower] --> B{Election Timeout?}
    B -->|Yes| C[Become Candidate]
    B -->|No| A
    
    C --> D[Increment Term]
    D --> E[Vote for Self]
    E --> F[Send RequestVote RPCs]
    
    F --> G{Majority Votes?}
    G -->|Yes| H[Become Leader]
    G -->|No| I{Higher Term Seen?}
    
    I -->|Yes| A
    I -->|No| J[Start New Election]
    J --> C
    
    H --> K[Send Heartbeats]
    K --> L[Handle Client Requests]
```

### 2. Cohérence des Logs

```mermaid
gantt
    title Réplication d'Event sur 3 Nœuds
    dateFormat X
    axisFormat %s
    
    section Leader
    Receive Event    :0, 1
    Create Log Entry :1, 2
    Send to Followers:2, 3
    Wait Majority    :3, 5
    Commit Event     :5, 6
    
    section Follower 1
    Receive Entry    :3, 4
    Persist to Log   :4, 5
    Send ACK         :5, 5.5
    Apply Event      :6, 7
    
    section Follower 2  
    Receive Entry    :3, 4
    Persist to Log   :4, 5
    Send ACK         :5, 5.5
    Apply Event      :6, 7
```

## 🔄 Event Sourcing Distribué

### Structure des Events

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RaftEvent {
    pub event_id: Uuid,
    pub aggregate_id: Uuid,
    pub event_type: String,
    pub event_data: serde_json::Value,
    pub sequence_number: u64,
    pub term: u64,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub node_id: u32,
}
```

### Log Compaction via Snapshots

```mermaid
timeline
    title Evolution du Log Raft
    
    Events 1-1000 : Log complet
                   : Taille: 100MB
                   
    Snapshot @1000 : État consolidé  
                    : Taille: 10MB
                    : Events 1-1000 → Snapshot
                    
    Events 1001-2000 : Nouveaux events
                      : + Snapshot précédent
                      : Taille totale: 20MB
                      
    Snapshot @2000 : Nouvel état
                    : Taille: 12MB  
                    : Suppression anciens events
```

## ⚡ Performance et Optimisations

### Métriques de Performance

| Métrique | Valeur Typique | Configuration |
|----------|---------------|---------------|
| **Latence consensus** | 5-15ms | 3 nœuds, LAN |
| **Throughput** | 5,000 ops/s | Batch size 100 |
| **Recovery time** | <2s | Snapshot récent |
| **Network usage** | 50KB/s/nœud | État stable |
| **Election time** | 300-600ms | Election timeout |

### Optimisations Configurables

```rust
#[derive(DeclarativeModel)]
#[consensus(
    // Performance optimizations
    batch_mode = true,              // Batch multiple events
    pipeline_replication = true,    // Pipeline AppendEntries
    compress_entries = true,        // Compress large payloads
    
    // Consistency trade-offs  
    read_consistency = "eventually", // "strong" | "eventually"
    async_apply = true,             // Non-blocking state machine
    
    // Network optimizations
    max_inflight_requests = 10,     // Concurrent RPCs
    heartbeat_batch_size = 50,      // Batch heartbeats
)]
pub struct OptimizedModel {
    // Model fields...
}
```

## 🛡️ Gestion des Pannes

### Scénarios de Panne

```mermaid
flowchart TD
    subgraph "Scénarios de Panne"
        A[Leader Crash]
        B[Follower Crash]
        C[Network Partition]
        D[Majority Loss]
    end
    
    A --> E[Election Automatique]
    E --> F[Nouveau Leader Élu]
    F --> G[Service Restored]
    
    B --> H[Follower Rejoin]
    H --> I[Log Catchup]
    I --> J[Sync Completed]
    
    C --> K[Split Brain Prevention]
    K --> L[Minority Stops]
    L --> M[Wait for Reunion]
    
    D --> N[Service Unavailable]
    N --> O[Manual Intervention]
    O --> P[Cluster Rebuild]
```

### Recovery Automatique

```rust
// Configuration de tolérance aux pannes
#[derive(DeclarativeModel)]
#[consensus(
    fault_tolerance = "byzantine",   // "crash" | "byzantine"  
    auto_recovery = true,           // Automatic node recovery
    backup_strategy = "continuous", // Continuous backups
    
    // Recovery timeouts
    leader_election_timeout_ms = 300,
    node_reconnect_timeout_ms = 10000,
    snapshot_recovery_timeout_ms = 30000,
)]
pub struct FaultTolerantModel {
    // Model with automatic fault tolerance
}
```

## 📈 Monitoring du Consensus

### Métriques Raft

```rust
// Métriques automatiques exposées
pub struct RaftMetrics {
    // État du cluster
    pub current_term: u64,
    pub current_leader: Option<u32>, 
    pub cluster_size: u32,
    pub healthy_nodes: u32,
    
    // Performance
    pub commit_latency_ms: f64,
    pub replication_lag_ms: f64,
    pub throughput_ops_per_sec: f64,
    
    // Élections
    pub election_count: u64,
    pub last_election_duration_ms: u64,
    
    // Logs
    pub log_size_entries: u64,
    pub last_applied_index: u64,
    pub commit_index: u64,
}
```

### Dashboards de Monitoring

```mermaid
flowchart LR
    subgraph "Raft Monitoring Dashboard"
        A[Cluster Health]
        B[Leader Election]  
        C[Log Replication]
        D[Performance Metrics]
    end
    
    A --> A1[Node Status: 3/3 UP]
    A --> A2[Leader: Node 1]
    A --> A3[Last Election: 2h ago]
    
    B --> B1[Election Count: 12]
    B --> B2[Avg Duration: 450ms]
    B --> B3[Success Rate: 100%]
    
    C --> C1[Commit Latency: 8ms]
    C --> C2[Replication Lag: 2ms]
    C --> C3[Log Size: 15.2MB]
    
    D --> D1[Throughput: 1,250 ops/s]
    D --> D2[CPU Usage: 15%]
    D --> D3[Memory: 245MB]
```

## 🧪 Testing et Validation

### Tests de Consensus

```rust
#[tokio::test]
async fn test_leader_election() {
    let cluster = TestCluster::new(3).await;
    
    // Arrêter le leader actuel
    cluster.stop_leader().await;
    
    // Vérifier qu'une nouvelle élection a lieu
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(cluster.has_leader().await);
    
    // Vérifier que le service continue
    let response = cluster
        .post("/api/products")
        .json(&test_product())
        .send()
        .await?;
    
    assert_eq!(response.status(), 201);
}

#[tokio::test]
async fn test_split_brain_prevention() {
    let cluster = TestCluster::new(5).await;
    
    // Créer partition réseau (2 vs 3)
    cluster.partition_network(&[0, 1], &[2, 3, 4]).await;
    
    // Vérifier qu'une seule partition reste active
    tokio::time::sleep(Duration::from_secs(1)).await;
    
    assert_eq!(cluster.active_partitions(), 1);
    assert!(cluster.majority_partition().is_serving());
    assert!(!cluster.minority_partition().is_serving());
}
```

### Chaos Engineering

```bash
# Tests de résilience automatisés
chaos-raft \
    --scenario leader_crash \
    --duration 300s \
    --cluster localhost:8080,localhost:8081,localhost:8082 \
    --load-test-concurrent 100

# Résultats attendus:
# - Availability: >99.9%
# - Max interruption: <500ms  
# - Data consistency: 100%
```

## 🚀 Exemples d'Usage

### Cluster 3 Nœuds Local

```bash
# Terminal 1 - Node 1 (Leader)
cargo run --bin raft_node -- \
    --node-id 1 \
    --port 8080 \
    --peers 127.0.0.1:8081,127.0.0.1:8082 \
    --data-dir ./data/node1

# Terminal 2 - Node 2 (Follower)  
cargo run --bin raft_node -- \
    --node-id 2 \
    --port 8081 \
    --peers 127.0.0.1:8080,127.0.0.1:8082 \
    --data-dir ./data/node2
    
# Terminal 3 - Node 3 (Follower)
cargo run --bin raft_node -- \
    --node-id 3 \
    --port 8082 \
    --peers 127.0.0.1:8080,127.0.0.1:8081 \
    --data-dir ./data/node3
```

### Test du Consensus

```bash
# Créer des données sur le leader
curl -X POST http://127.0.0.1:8080/api/products \
    -H "Content-Type: application/json" \
    -d '{"name": "Test Product", "price": 19.99}'

# Vérifier cohérence sur les followers  
curl http://127.0.0.1:8081/api/products | jq '.[]'
curl http://127.0.0.1:8082/api/products | jq '.[]'

# Les 3 nœuds doivent avoir les mêmes données
```

## 🗺️ Roadmap

### v1.1 (Prochain)
- ✅ Byzantine Fault Tolerance
- ✅ Multi-Raft (sharding)
- ✅ Witness nodes (non-voting)
- ✅ Learner mode pour scaling lecture

### v1.2 (Futur)  
- 🔄 Cross-datacenter replication
- 🔄 Consensus sur événements cryptographiquement signés
- 🔄 Auto-scaling cluster
- 🔄 Consensus-as-a-Service

---

**💡 Note :** Le consensus Raft dans Lithair est optimisé pour la cohérence forte tout en maintenant des performances élevées pour les applications temps réel.