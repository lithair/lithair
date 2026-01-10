# Lithair Architecture Overview

Lithair revolutionizes backend development with a **Data-First** approach that unifies all infrastructure layers around a single data definition.

## Vision

### Problem: Traditional 3-Tier Architecture

```mermaid
flowchart TD
    subgraph "Traditional Approach (3-Tier)"
        A[Presentation Layer<br/>Controllers, Routes, Validation]
        B[Business Layer<br/>Services, Domain Logic, Rules]
        C[Data Layer<br/>Database, ORM, Queries]
    end

    A --> B
    B --> C

    D[Problem: Duplicated Code] --> A
    D --> B
    D --> C

    E[Maintenance: 3x the work] --> A
    E --> B
    E --> C
```

### Solution: Lithair Data-First Architecture

```mermaid
flowchart TD
    A[Declarative Model<br/>Single source of truth] --> B[Macro System]

    B --> C[API Layer]
    B --> D[Business Layer]
    B --> E[Persistence Layer]
    B --> F[Security Layer]
    B --> G[Distribution Layer]

    subgraph "Auto-Generated"
        C
        D
        E
        F
        G
    end

    H[Result: 99% less code] --> C
    H --> D
    H --> E
    H --> F
    H --> G
```

## Layered Architecture

### Component Overview

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

## Complete Data Flow

### Request Lifecycle

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

## Mental Model: Data-First

### Conceptual Transformation

```mermaid
mindmap
  root((Lithair<br/>Data-First))
    (One Struct)
      [Declarative Attributes]
        #[db(...)]
        #[http(...)]
        #[permission(...)]
        #[lifecycle(...)]
        #[persistence(...)]
    (Auto Generation)
      [REST API]
        GET/POST/PUT/DELETE
        Automatic validation
        JSON serialization
      [Database]
        Automatic schemas
        Migrations
        Optimized indexes
      [Security]
        Granular RBAC
        IP Firewall
        Rate limiting
      [Distribution]
        Event Sourcing
        Raft Consensus
        Multi-node replication
```

## Technical Architecture

### Core Components

```mermaid
classDiagram
    class DeclarativeModel {
        +derive(DeclarativeModel)
        +generate_api()
        +generate_database_schema()
        +generate_validation()
        +generate_permissions()
    }

    class DeclarativeServer {
        +new(bind_addr)
        +with_firewall()
        +with_cors()
        +run()
    }

    class EventStore {
        +append_event()
        +load_aggregate()
        +create_snapshot()
        +query_events()
    }

    class RaftConsensus {
        +propose_change()
        +handle_vote()
        +replicate_log()
        +elect_leader()
    }

    class HttpFirewall {
        +check_ip()
        +rate_limit()
        +apply_rules()
    }

    DeclarativeModel --> DeclarativeServer
    DeclarativeServer --> EventStore
    DeclarativeServer --> HttpFirewall
    EventStore --> RaftConsensus
```

### Module Dependencies

```mermaid
graph TD
    subgraph "lithair-macros"
        A[DeclarativeModel Derive]
        B[Attribute Parsing]
        C[Code Generation]
    end

    subgraph "lithair-core"
        D[HTTP Server]
        E[Event Store]
        F[Raft Consensus]
        G[Firewall]
        H[Validation]
    end

    A --> D
    B --> E
    C --> F
    C --> G
    C --> H

    D --> I[Hyper]
    E --> J[Serde]
    F --> K[OpenRaft]
    G --> L[DashMap]
    H --> M[Regex]
```

## Performance Architecture

### Optimization Stack

```mermaid
flowchart LR
    subgraph "Memory Optimizations"
        A[Zero-Copy Serialization]
        B[Memory Pool Allocation]
        C[Lazy Loading]
    end

    subgraph "I/O Optimizations"
        D[Async I/O - Tokio]
        E[Batch Operations]
        F[Connection Pooling]
    end

    subgraph "Storage Optimizations"
        G[Event Compaction]
        H[Compression - ZSTD]
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

### Performance Metrics

| Component | P50 Latency | P99 Latency | Throughput | CPU Usage |
|-----------|-------------|-------------|------------|-----------|
| **HTTP Server** | 0.3ms | 1.2ms | 50K req/s | 15% |
| **Firewall** | 0.1ms | 0.4ms | 100K req/s | 5% |
| **Event Store** | 0.8ms | 3.2ms | 25K ops/s | 25% |
| **Raft Consensus** | 5.2ms | 15.8ms | 5K ops/s | 20% |
| **Total Stack** | 2.1ms | 8.5ms | 15K req/s | 35% |

## Security Architecture

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

## Distribution Architecture

### Multi-Node Cluster

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

### Consistency Guarantees

```mermaid
timeline
    title Consistency Levels

    Strong Consistency : Raft Consensus
                       : All nodes agree before commit
                       : Linearizable reads/writes

    Sequential Consistency : Event Ordering
                            : Same order on all nodes
                            : Causal relationships preserved

    Eventual Consistency : Asynchronous Learners
                          : Eventually all nodes converge
                          : Higher availability, lower latency
```

## Design Principles

### Core Principles

1. **Single Source of Truth**: Declarative models define everything
2. **Generated, Not Written**: Infrastructure code is generated
3. **Security by Design**: Security is built-in, not bolted on
4. **Performance by Default**: Automatic optimizations
5. **Consistency First**: Strong consistency across the stack
6. **Developer Experience**: Simplicity without sacrificing power

### Architectural Trade-offs

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
        J[Strong Consistency ]
        K[High Availability ]
        L[Fast Development ]
        M[Type Safety ]
    end
```

---

**Vision:** Lithair combines **declarative simplicity** and **distributed performance** in a unified architecture where **thinking about data is enough** to get a complete, scalable backend.
