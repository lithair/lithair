# Flux de Données dans Lithair

Ce document détaille comment les données circulent dans l'architecture Lithair, de la réception d'une requête HTTP jusqu'à la persistance distribuée.

## 🌊 Vue d'Ensemble du Flux

```mermaid
flowchart TD
    A[Client HTTP] --> B[Serveur HTTP Hyper]
    B --> C[Middleware Firewall]
    C --> D[Router Déclaratif]
    D --> E[Validation des Données]
    E --> F[Vérification Permissions]
    F --> G[Handler CRUD]
    G --> H[Event Sourcing]
    H --> I[Consensus Raft]
    I --> J[Stockage Local]
    J --> K[Réplication Cluster]
    
    subgraph "Couche HTTP"
        B
        C
        D
    end
    
    subgraph "Couche Application" 
        E
        F
        G
    end
    
    subgraph "Couche Persistance"
        H
        I
        J
        K
    end
```

## 🔄 Flux Détaillé par Opération

### 1. Requête GET (Lecture)

```mermaid
sequenceDiagram
    participant C as Client
    participant HS as HTTP Server
    participant R as Router
    participant P as Permissions
    participant H as Handler
    participant ES as Event Store
    participant S as Storage

    C->>HS: GET /api/products/123
    HS->>R: Route Request
    R->>P: Check Read Permission
    P->>H: Forward if Authorized
    H->>ES: Query Current State
    ES->>S: Read from Storage
    S-->>ES: Return Data
    ES-->>H: Product State
    H-->>C: JSON Response
```

**Flux de données :**
1. **Requête HTTP** : Client → Serveur Hyper
2. **Routage** : Identification du handler via attributs `#[http(expose)]`
3. **Permissions** : Vérification RBAC via `#[permission(read = "...")]`
4. **Lecture état** : Reconstruction depuis Event Store
5. **Réponse JSON** : Sérialisation automatique

### 2. Requête POST (Création)

```mermaid
sequenceDiagram
    participant C as Client
    participant HS as HTTP Server
    participant V as Validator
    participant P as Permissions
    participant H as Handler
    participant ES as Event Store
    participant Raft as Raft Consensus
    participant S as Storage

    C->>HS: POST /api/products + JSON
    HS->>V: Validate Request Data
    V->>P: Check Write Permission
    P->>H: Forward if Authorized
    H->>ES: Create Event
    ES->>Raft: Propose Change
    Raft->>Raft: Achieve Consensus
    Raft->>S: Commit to Storage
    S-->>H: Confirmation
    H-->>C: 201 Created + JSON
```

**Flux de données :**
1. **Désérialisation** : JSON → Struct Rust via Serde
2. **Validation** : Vérification via attributs `#[http(validate = "...")]`
3. **Permissions** : Check write via `#[permission(write = "...")]`
4. **Event Creation** : Génération événement avec ID unique
5. **Consensus** : Synchronisation via Raft entre nœuds
6. **Persistance** : Écriture atomique dans Event Store

### 3. Requête PUT (Modification)

```mermaid
sequenceDiagram
    participant C as Client
    participant HS as HTTP Server
    participant V as Validator
    participant P as Permissions
    participant H as Handler
    participant ES as Event Store
    participant A as Audit
    participant Raft as Raft Consensus

    C->>HS: PUT /api/products/123 + JSON
    HS->>V: Validate Changes
    V->>P: Check Write Permission
    P->>H: Process Update
    H->>ES: Query Current State
    ES-->>H: Current Product
    H->>H: Apply Changes
    H->>A: Log Audit Trail
    H->>ES: Create Update Event
    ES->>Raft: Replicate Change
    Raft-->>H: Consensus Achieved
    H-->>C: 200 OK + Updated JSON
```

**Flux de données :**
1. **Delta detection** : Comparaison état actuel vs modifications
2. **Audit automatique** : Si `#[lifecycle(audited)]` présent
3. **Versioning** : Gestion versions si `#[lifecycle(versioned = N)]`
4. **Event replay** : Reconstruction état depuis événements
5. **Réplication** : Propagation changement vers autres nœuds

## 🗄️ Architecture de Stockage

### Event Store Structure

```mermaid
erDiagram
    Event {
        uuid event_id
        uuid aggregate_id
        string event_type
        json event_data
        timestamp created_at
        int sequence_number
        string node_id
    }
    
    Snapshot {
        uuid aggregate_id
        json state_data
        int last_event_sequence
        timestamp created_at
    }
    
    AuditLog {
        uuid event_id
        string user_id
        string action
        json old_values
        json new_values
        timestamp timestamp
    }
    
    Event ||--o{ AuditLog : "generates"
    Event ||--o{ Snapshot : "compacts_to"
```

### Flux de Persistance

```mermaid
flowchart LR
    A[Nouvelle Donnée] --> B{Event Store}
    B --> C[Append Event]
    C --> D[Update Sequence]
    D --> E{Snapshot Needed?}
    E -->|Yes| F[Create Snapshot]
    E -->|No| G[Continue]
    F --> G
    G --> H[Replicate to Peers]
    
    subgraph "Local Storage"
        I[Events Log]
        J[Snapshots]
        K[Indexes]
    end
    
    B --> I
    F --> J
    D --> K
```

## 🔄 Patterns de Données

### 1. CQRS (Command Query Responsibility Segregation)

```rust
// Commands (Write Side)
#[derive(DeclarativeModel)]
pub struct ProductCommand {
    #[http(method = "POST", path = "/api/products")]
    #[persistence(event_sourced)]
    pub create_product: CreateProduct,
    
    #[http(method = "PUT", path = "/api/products/{id}")]
    #[lifecycle(audited)]
    pub update_product: UpdateProduct,
}

// Queries (Read Side) 
#[derive(DeclarativeModel)]
pub struct ProductQuery {
    #[http(method = "GET", path = "/api/products")]
    #[db(indexed, optimized_for_read)]
    pub list_products: ProductList,
    
    #[http(method = "GET", path = "/api/products/{id}")]
    #[caching(ttl = 300)]
    pub get_product: Product,
}
```

### 2. Event Sourcing avec Snapshots

```mermaid
timeline
    title Évolution d'un Product (ID: 123)
    
    Event 1 : ProductCreated
             : name="iPhone 15"
             : price=999.99
             
    Event 2 : PriceUpdated  
             : old_price=999.99
             : new_price=899.99
             
    Event 3 : StockUpdated
             : stock=100
             
    Snapshot : Consolidated State
             : name="iPhone 15"
             : price=899.99
             : stock=100
             
    Event 4 : ProductDeleted
             : deleted_at=2024-09-13
```

### 3. Aggregation et Projections

```rust
#[derive(DeclarativeModel)]
#[projection(from = "OrderEvent", update_on = ["OrderCreated", "OrderCancelled"])]
pub struct SalesMetrics {
    #[db(primary_key)]
    pub date: Date,
    
    #[aggregate(sum, source = "Order.total")]
    pub daily_revenue: f64,
    
    #[aggregate(count, source = "Order.id")]
    pub orders_count: i64,
    
    #[aggregate(avg, source = "Order.total")]
    pub avg_order_value: f64,
}
```

## ⚡ Optimisations de Performance

### 1. Lecture Optimisée

```mermaid
flowchart TD
    A[GET Request] --> B{Cache Hit?}
    B -->|Yes| C[Return Cached]
    B -->|No| D{Recent Snapshot?}
    D -->|Yes| E[Load Snapshot]
    D -->|No| F[Replay Events]
    
    E --> G[Apply Recent Events]
    F --> H[Full Reconstruction]
    G --> I[Cache Result]
    H --> I
    I --> J[Return to Client]
    
    subgraph "Performance Layers"
        K[Memory Cache]
        L[Snapshot Store]
        M[Event Store]
    end
```

### 2. Écriture Optimisée

```rust
// Optimisations déclaratives
#[derive(DeclarativeModel)]
#[performance(
    batch_size = 100,           // Batch events
    async_replication = true,   // Async to followers  
    snapshot_frequency = 1000   // Snapshot every 1000 events
)]
pub struct HighThroughputModel {
    #[db(indexed, bloom_filter)]  // Fast lookups
    #[caching(write_behind)]      // Async writes
    pub high_frequency_field: String,
}
```

## 📊 Métriques de Flux

### Latences par Étape

```mermaid
gantt
    title Latence Typique d'une Requête POST
    dateFormat X
    axisFormat %L ms
    
    section HTTP Layer
    Request Parsing    :0, 0.1
    Firewall Check     :0.1, 0.2
    Routing           :0.2, 0.3
    
    section Application
    Validation        :0.3, 0.5
    Permissions      :0.5, 0.7
    Handler Logic    :0.7, 1.0
    
    section Persistence  
    Event Creation   :1.0, 1.2
    Raft Consensus   :1.2, 2.8
    Local Storage    :2.8, 3.0
    
    section Response
    Serialization    :3.0, 3.2
    HTTP Response    :3.2, 3.5
```

### Throughput par Composant

| Composant | Throughput (ops/s) | Goulot d'étranglement |
|-----------|-------------------|----------------------|
| HTTP Server | 50,000 | - |
| Firewall | 45,000 | IP lookup |
| Validation | 40,000 | Complex rules |
| Permissions | 35,000 | RBAC queries |
| Event Store | 15,000 | Disk I/O |
| Raft Consensus | 5,000 | Network + Consensus |

## 🔍 Debugging du Flux

### Tracing Distribué

```rust
// Automatic tracing with OpenTelemetry
#[derive(DeclarativeModel)]
#[tracing(
    enabled = true,
    sample_rate = 0.1,      // 10% of requests
    include_body = false     // Security
)]
pub struct TracedProduct {
    // Model fields...
}
```

### Logs Structurés

```json
{
  "timestamp": "2024-09-13T10:30:00Z",
  "level": "INFO", 
  "trace_id": "abc123",
  "span_id": "def456",
  "message": "Processing product creation",
  "context": {
    "model": "Product",
    "operation": "create",
    "user_id": "user_789",
    "request_size": 1024,
    "validation_time_ms": 2.3
  }
}
```

## 🛠️ Configuration du Flux

### Tuning Performance

```rust
// Configuration globale du flux
let config = LithairConfig {
    http_server: HttpConfig {
        worker_threads: num_cpus::get(),
        connection_pool: 1000,
        request_timeout: Duration::from_secs(30),
    },
    
    event_store: EventStoreConfig {
        batch_size: 100,
        sync_mode: SyncMode::Periodic(Duration::from_millis(10)),
        snapshot_threshold: 1000,
    },
    
    raft: RaftConfig {
        election_timeout: Duration::from_millis(300),
        heartbeat_interval: Duration::from_millis(50),
        max_payload_entries: 100,
    },
};
```

---

**💡 Résumé :** Le flux de données Lithair est conçu pour être **prévisible**, **traceable** et **optimisé** tout en maintenant la **cohérence distribuée** et la **sécurité** à chaque étape.