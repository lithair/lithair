# Diagrammes Lithair

Collection complète de tous les diagrammes Mermaid utilisés dans la documentation Lithair, organisés par catégorie et cas d'usage.

## 🏗️ Architecture Générale

### Vue d'Ensemble du Système

```mermaid
flowchart TD
    A[Modèle Déclaratif<br/>Une seule source de vérité] --> B[Macro System]
    
    B --> C[API Layer]
    B --> D[Business Layer]
    B --> E[Persistence Layer]
    B --> F[Security Layer]
    B --> G[Distribution Layer]
    
    subgraph "Généré Automatiquement"
        C
        D
        E
        F
        G
    end
    
    H[Résultat: 99% moins de code] --> C
    H --> D
    H --> E
    H --> F
    H --> G
```

### Architecture en Couches

```mermaid
flowchart TB
    subgraph "Client Layer"
        CL[HTTP Clients<br/>Web, Mobile, API]
    end
    
    subgraph "Gateway Layer"
        FW[HTTP Firewall<br/>IP Filter, Rate Limit]
        LB[Load Balancer<br/>Request Distribution]
    end
    
    subgraph "Application Layer"
        HS[HTTP Server<br/>Hyper-based]
        RT[Router<br/>Auto-generated Routes]
        VL[Validation<br/>Declarative Rules]
        AU[Authorization<br/>RBAC System]
    end
    
    subgraph "Business Layer"
        HN[Handlers<br/>CRUD Operations]
        BL[Business Logic<br/>Generated from Models]
        EV[Event Processing<br/>Command/Query]
    end
    
    subgraph "Persistence Layer"
        ES[Event Store<br/>Event Sourcing]
        SS[Snapshots<br/>State Reconstruction]
        IX[Indexes<br/>Query Optimization]
    end
    
    subgraph "Distribution Layer"
        RF[Raft Consensus<br/>Leader Election]
        RP[Replication<br/>Multi-node Sync]
        ST[Storage<br/>Persistent Files]
    end
    
    CL --> FW
    FW --> LB
    LB --> HS
    HS --> RT
    RT --> VL
    VL --> AU
    AU --> HN
    HN --> BL
    BL --> EV
    EV --> ES
    ES --> SS
    ES --> IX
    ES --> RF
    RF --> RP
    RP --> ST
```

## 🔄 Flux de Données

### Cycle de Vie d'une Requête HTTP

```mermaid
sequenceDiagram
    participant C as Client
    participant FW as Firewall
    participant HS as HTTP Server
    participant RT as Router
    participant AU as Auth
    participant HL as Handler
    participant ES as Event Store
    participant RF as Raft
    participant ST as Storage

    C->>FW: HTTP Request
    FW->>FW: Check IP + Rate Limits
    FW->>HS: Forward if Allowed
    HS->>RT: Route Request
    RT->>RT: Match Declarative Routes
    RT->>AU: Check Permissions
    AU->>AU: RBAC Validation
    AU->>HL: Execute Handler
    HL->>HL: Business Logic
    HL->>ES: Create Event
    ES->>RF: Consensus Request
    RF->>RF: Raft Protocol
    RF->>ST: Commit to Storage
    ST-->>ES: Storage Confirmed
    ES-->>HL: Event Applied
    HL-->>C: JSON Response
```

### Flux Event Sourcing

```mermaid
flowchart TD
    A[Application Events] --> B[Event Store]
    B --> C[Event Log]
    B --> D[Snapshots]
    B --> E[Indexes]
    
    C --> F[Reconstruction d'État]
    D --> F
    E --> G[Requêtes Optimisées]
    
    F --> H[Current State]
    G --> H
    
    subgraph "Persistance"
        C
        D  
        E
    end
    
    subgraph "Performance"
        I[Memory Cache]
        J[Bloom Filters]
        K[Compression]
    end
    
    B --> I
    E --> J
    C --> K
```

## 🛡️ Module HTTP Firewall

### Flux de Protection

```mermaid
flowchart TD
    A[Requête HTTP] --> B{Firewall Activé?}
    B -->|Non| H[Traitement Normal]
    B -->|Oui| C{IP Autorisée?}
    C -->|Non| D[403 Forbidden]
    C -->|Oui| E{Rate Limit Global?}
    E -->|Dépassé| F[429 Too Many Requests]
    E -->|OK| G{Rate Limit IP?}
    G -->|Dépassé| I[429 IP Rate Limited]
    G -->|OK| H[Traitement Normal]
```

## ⚖️ Consensus Raft

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

### Cluster Multi-Nœuds

```mermaid
flowchart TD
    subgraph "Region 1"
        subgraph "Datacenter A"
            A1[Node 1<br/>Leader]
            A2[Node 2<br/>Follower]
        end
        subgraph "Datacenter B"
            B1[Node 3<br/>Follower]
        end
    end
    
    subgraph "Region 2"
        subgraph "Datacenter C"
            C1[Node 4<br/>Learner]
            C2[Node 5<br/>Learner]
        end
    end
    
    A1 -.->|Heartbeat| A2
    A1 -.->|Heartbeat| B1
    A1 -.->|Replication| C1
    A1 -.->|Replication| C2
    
    A2 -->|Failover| A1
    B1 -->|Failover| A1
```

## 📊 Stockage et Persistance

### Architecture Event Store

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

### Cycle de Vie des Données

```mermaid
flowchart LR
    subgraph "Storage Layers"
        A[Memory Cache]
        B[SSD Event Log] 
        C[Compressed Archives]
        D[Cold Storage]
    end
    
    subgraph "Data Lifecycle"
        E[Hot Data<br/>0-7 days]
        F[Warm Data<br/>7-90 days]
        G[Cold Data<br/>>90 days]
    end
    
    E --> A
    E --> B
    F --> B
    F --> C
    G --> C
    G --> D
    
    A -->|Cache Miss| B
    B -->|Archival| C
    C -->|Deep Archive| D
```

### Patterns CQRS

```mermaid
flowchart TD
    A[Product Events] --> B[Event Processor]
    
    B --> C[Product Read Model]
    B --> D[Sales Analytics]
    B --> E[Inventory Summary]
    B --> F[Audit Trail]
    
    subgraph "Projections Temps Réel"
        C --> G[Product Catalog API]
        D --> H[Dashboard Analytics]
        E --> I[Stock Alerts]
        F --> J[Compliance Reports]
    end
    
    subgraph "Event Types"
        K[ProductCreated]
        L[PriceUpdated] 
        M[StockChanged]
        N[ProductDeleted]
    end
    
    K --> B
    L --> B
    M --> B
    N --> B
```

## 🧠 Modèles Déclaratifs

### Transformation Data-First

```mermaid
mindmap
  root((Lithair<br/>Data-First))
    (Une Struct)
      [Attributs Déclaratifs]
        #[db(...)]
        #[http(...)]
        #[permission(...)]
        #[lifecycle(...)]
        #[persistence(...)]
    (Génération Automatique)
      [API REST]
        GET/POST/PUT/DELETE
        Validation automatique
        Sérialisation JSON
      [Base de Données]
        Schémas automatiques
        Migrations
        Indexes optimisés
      [Sécurité]
        RBAC granulaire
        Firewall IP
        Rate limiting
      [Distribution]
        Event Sourcing
        Consensus Raft
        Réplication multi-nœuds
```

## 🔐 Architecture Sécurisée

### Defense in Depth

```mermaid
flowchart TD
    A[Internet] --> B[DDoS Protection]
    B --> C[HTTP Firewall<br/>IP Filter + Rate Limit]
    C --> D[TLS Termination]
    D --> E[Authentication<br/>JWT + API Keys]
    E --> F[Authorization<br/>RBAC + Field-Level]
    F --> G[Input Validation<br/>Declarative Rules]
    G --> H[Business Logic<br/>Secure by Design]
    H --> I[Data Encryption<br/>At Rest + In Transit]
    I --> J[Audit Logging<br/>Full Traceability]
    J --> K[Storage<br/>Encrypted + Replicated]
```

## ⚡ Optimisations Performance

### Stack d'Optimisations

```mermaid
flowchart LR
    subgraph "Memory Optimizations"
        A[Zero-Copy Serialization]
        B[Memory Pool Allocation]
        C[Lazy Loading]
    end
    
    subgraph "I/O Optimizations"
        D[Async I/O (Tokio)]
        E[Batch Operations]
        F[Connection Pooling]
    end
    
    subgraph "Storage Optimizations"
        G[Event Compaction]
        H[Compression (ZSTD)]
        I[Bloom Filters]
    end
    
    subgraph "Network Optimizations"
        J[Pipeline Replication]
        K[Request Batching]
        L[Keep-Alive Connections]
    end
    
    A --> D
    B --> E
    C --> F
    D --> G
    E --> H
    F --> I
    G --> J
    H --> K
    I --> L
```

### Métriques de Latence

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

## 📈 Scalabilité

### Scaling Horizontal

```mermaid
flowchart TB
    subgraph "Load Balancer Tier"
        LB[HAProxy/Nginx]
    end
    
    subgraph "Application Tier (Stateless)"
        A1[Lithair Node 1]
        A2[Lithair Node 2]
        A3[Lithair Node N]
    end
    
    subgraph "Consensus Tier (Stateful)"
        R1[Raft Leader]
        R2[Raft Follower]
        R3[Raft Follower]
    end
    
    subgraph "Storage Tier"
        S1[Persistent Store 1]
        S2[Persistent Store 2]
        S3[Persistent Store 3]
    end
    
    LB --> A1
    LB --> A2
    LB --> A3
    
    A1 --> R1
    A2 --> R1
    A3 --> R1
    
    R1 --> R2
    R1 --> R3
    
    R1 --> S1
    R2 --> S2
    R3 --> S3
```

## 🧪 Architecture de Test

### Stratégie de Tests

```mermaid
flowchart LR
    subgraph "Unit Tests"
        A[Model Tests<br/>Auto-generated]
        B[Validation Tests<br/>Declarative Rules]
        C[Permission Tests<br/>RBAC Logic]
    end
    
    subgraph "Integration Tests"
        D[API Tests<br/>Full HTTP Stack]
        E[Database Tests<br/>Event Store]
        F[Consensus Tests<br/>Multi-node]
    end
    
    subgraph "End-to-End Tests"
        G[User Journeys<br/>Complete Workflows]
        H[Performance Tests<br/>Load Testing]
        I[Chaos Tests<br/>Fault Tolerance]
    end
    
    A --> D
    B --> E
    C --> F
    D --> G
    E --> H
    F --> I
```

## 🗺️ Évolution Temporelle

### Roadmap Architecture

```mermaid
gantt
    title Lithair Architecture Roadmap
    dateFormat  YYYY-MM-DD
    section v1.0 Foundation
    Core Event Store        :done, foundation, 2024-01-01, 2024-06-30
    Raft Consensus         :done, consensus, 2024-03-01, 2024-08-31
    HTTP + Firewall        :done, http, 2024-06-01, 2024-09-30
    
    section v1.1 Enhancement
    Multi-Raft Sharding    :enhancement, 2024-10-01, 2025-01-31
    Byzantine Fault Tolerance :bft, 2024-11-01, 2025-02-28
    Cross-Region Replication :cross-region, 2024-12-01, 2025-03-31
    
    section v1.2 Advanced
    Auto-Scaling           :auto-scale, 2025-02-01, 2025-05-31
    ML-Driven Optimization :ml-opt, 2025-03-01, 2025-06-30
    Edge Computing         :edge, 2025-04-01, 2025-07-31
```

### Evolution d'un Product

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

## 📊 Métriques et Monitoring

### Distribution du Stockage

```mermaid
pie title Storage Distribution
    "Event Data" : 60
    "Snapshots" : 25
    "Indexes" : 10
    "Metadata" : 5
```

### Analyse Temporelle des Events

```mermaid
gantt
    title Analyse Temporelle des Events
    dateFormat YYYY-MM-DD
    axisFormat %m/%d
    
    section Product Lifecycle
    Created        :milestone, created, 2024-01-01, 0d
    Price Updates  :price-updates, 2024-01-01, 2024-12-31
    Stock Changes  :stock-changes, 2024-01-01, 2024-12-31
    Discontinued   :milestone, discontinued, 2024-10-15, 0d
    
    section Analytics Windows
    Daily Metrics  :daily, 2024-01-01, 2024-12-31
    Weekly Reports :weekly, 2024-01-01, 2024-12-31
    Monthly Summary:monthly, 2024-01-01, 2024-12-31
```

## 🎯 Trade-offs Architecturaux

### Équilibres de Conception

```mermaid
graph LR
    A[Consistency] <--> B[Availability]
    B <--> C[Partition Tolerance]
    C <--> A
    
    D[Development Speed] <--> E[Runtime Performance]
    E <--> F[Resource Usage]
    F <--> D
    
    G[Type Safety] <--> H[Flexibility]
    H <--> I[Learning Curve]
    I <--> G
    
    subgraph "Lithair Choices"
        J[Strong Consistency ✓]
        K[High Availability ✓]
        L[Fast Development ✓]
        M[Type Safety ✓]
    end
```

---

**💡 Usage :** Ces diagrammes peuvent être copiés directement dans des documents Markdown avec support Mermaid ou utilisés dans des outils comme GitHub, GitLab, ou Notion.