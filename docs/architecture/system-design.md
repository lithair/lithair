# Lithair System Overview - Complete Guide with Diagrams

_Created by Yoan Roblet - Disruptive application framework with AI assistance_

## 🎯 The Lithair Philosophy

Lithair emerged from a simple frustration: **why create a complex 3-tier architecture for just 3 simple tables?**

The core insight is that most applications don't need massive databases - they need **intelligent data lifecycle management**. Instead of forcing developers to build their own historization systems, Lithair provides an integrated approach where you declare how each piece of data should behave throughout its lifecycle.

### The Declarative Data Lifecycle Concept

```rust
// Traditional approach: Manual historization everywhere
// - Product creation date: Do we need to historize? NO (never changes)
// - Product price: Do we need full history? YES (business critical)
// - Product name: Do we need 20 versions? NO (2-3 copies sufficient)

// Lithair approach: Declare the lifecycle upfront
#[derive(Event)]
struct ProductCreated {
    id: ProductId,
    name: String,        // @historize(versions=3)
    creation_date: Date, // @historize(never) - immutable
    price: Money,        // @historize(full) - business critical
}
```

**Key Philosophy**: Think about each data point's lifecycle from day one. You can change it later, but this integrated vision eliminates the need for developers to add their own historization systems afterward.

## 🏗️ System Architecture

Lithair **embeds everything into a single binary**, eliminating traditional 3-tier complexity:

### Traditional vs Lithair Approach

```mermaid
graph TB
    subgraph "Traditional 3-Tier (Complex)"
        WEB[Web Server]
        APP[Application Server]
        DB[(Database)]
        CACHE[(Cache)]
        WEB --> APP
        APP --> DB
        APP --> CACHE
        APP -.-> HIST[Manual Historization]
    end

    subgraph "Lithair (Simple)"
        SINGLE["Single Binary\n• HTTP Server\n• Business Logic\n• Event Store\n• Declarative Lifecycle"]
    end
```

### Core Architecture Components

```mermaid
graph TB
    subgraph "Lithair Application (Single Binary)"
        subgraph "Declarative Layer"
            MODEL[Data Models]
            LIFECYCLE[Lifecycle Declarations]
            HIST[Historization Rules]
        end

        subgraph "Runtime Layer"
            HTTP[HTTP Server]
            ROUTES[Route Handler]
            EVENTS[Event Handler]
            STATE[In-Memory State]
        end

        subgraph "Storage Layer"
            LOG[(Event Log)]
            SNAP[(Smart Snapshots)]
            META[(Lifecycle Metadata)]
        end

        MODEL --> LIFECYCLE
        LIFECYCLE --> HIST
        HTTP --> ROUTES
        ROUTES --> EVENTS
        EVENTS --> STATE
        EVENTS --> LOG
        HIST --> SNAP
        LIFECYCLE --> META
    end
```

## 🔄 Event Flow (Event Sourcing)

### Event Lifecycle

```mermaid
sequenceDiagram
    participant Client
    participant HTTP as HTTP Server
    participant Events as Event Handler
    participant Lifecycle as Lifecycle Engine
    participant State as In-Memory State
    participant Store as Smart Storage

    Client->>HTTP: Product Update Request
    HTTP->>Events: ProductUpdated Event

    Events->>Lifecycle: Analyze Field Changes

    Note over Lifecycle: Price changed: full_history
    Note over Lifecycle: Name changed: versions=3
    Note over Lifecycle: Created_at: immutable (ignore)

    Lifecycle->>Store: Store with Rules
    Store->>Store: Price: Add to full history
    Store->>Store: Name: Keep last 3 versions
    Store->>Store: Created_at: Skip (immutable)

    Events->>State: Update Current State
    State-->>HTTP: Updated State
    HTTP-->>Client: Success Response
```

### Event Structure

```mermaid
graph LR
    subgraph "Business Event"
        EVENT[ProductCreated]
        PAYLOAD["{name: 'iPhone', price: 999}"]
    end

    subgraph "Lithair Envelope"
        ENVELOPE[Event Envelope]
        TYPE[event_type: 'product_app::ProductEvent']
        ID[event_id: 'product:123:create']
        TIMESTAMP[timestamp: 1723806050]
        JSON[payload: JSON string]
    end

    subgraph "Persistence"
        EVENTLOG[events.raftlog]
        DEDUP_INDEX[Deduplication Index]
        HASH[Deduplication Hash]
    end

    EVENT --> ENVELOPE
    PAYLOAD --> JSON
    ENVELOPE --> EVENTLOG
    ID --> DEDUP_INDEX
    ID --> HASH
```

## 🚀 Startup and Recovery

### Startup Process

```mermaid
flowchart TD
    START([Application Startup]) --> LOAD_META{Metadata exists?}

    LOAD_META -->|Yes| LOAD_SNAP{Snapshot exists?}
    LOAD_META -->|No| INIT_EMPTY[Initialize empty state]

    LOAD_SNAP -->|Yes| RESTORE_SNAP[Load snapshot]
    LOAD_SNAP -->|No| INIT_EMPTY

    RESTORE_SNAP --> REPLAY_EVENTS[Replay events since snapshot]
    INIT_EMPTY --> REPLAY_ALL[Replay all events]

    REPLAY_EVENTS --> BUILD_DEDUP[Rebuild deduplication index]
    REPLAY_ALL --> BUILD_DEDUP

    BUILD_DEDUP --> INIT_HTTP[Initialize HTTP server]
    INIT_HTTP --> READY[🚀 Application Ready]

    style START fill:#e8f5e8
    style READY fill:#c8e6c9
    style RESTORE_SNAP fill:#fff3e0
    style REPLAY_EVENTS fill:#f3e5f5
```

### State Recovery

```mermaid
graph TB
    subgraph "Persistence Files"
        SNAP[(state.raftsnap<br/>Complete State)]
        LOG[(events.raftlog<br/>Events since snapshot)]
        DEDUP[(dedup.raftids<br/>IDs for deduplication)]
    end

    subgraph "Recovery Process"
        LOAD[Load Snapshot]
        REPLAY[Replay Events]
        REBUILD[Rebuild Indexes]
    end

    subgraph "Final State"
        MEMORY[In-Memory State]
        INDEXES[Pre-calculated Indexes]
        ANALYTICS[Real-time Analytics]
    end

    SNAP --> LOAD
    LOG --> REPLAY
    DEDUP --> REBUILD

    LOAD --> MEMORY
    REPLAY --> MEMORY
    REBUILD --> INDEXES
    MEMORY --> ANALYTICS

    style SNAP fill:#e3f2fd
    style LOG fill:#f1f8e9
    style DEDUP fill:#fce4ec
    style MEMORY fill:#fff3e0
```

## 📊 Architecture de Performance

### Accès aux Données

```mermaid
graph LR
    subgraph "Approche Traditionnelle"
        CLIENT1[Client] --> API1[API Server]
        API1 --> DB1[(Database)]
        DB1 --> NETWORK1[Réseau 1-10ms]
        NETWORK1 --> QUERY1[Parsing SQL]
        QUERY1 --> DISK1[Accès Disque]
        DISK1 --> SERIALIZE1[Sérialisation]
    end

    subgraph "Approche Lithair"
        CLIENT2[Client] --> API2[Lithair]
        API2 --> MEMORY2[HashMap en Mémoire]
        MEMORY2 --> DIRECT2[Accès Direct 5ns]
    end

    style NETWORK1 fill:#ffcdd2
    style DISK1 fill:#ffcdd2
    style SERIALIZE1 fill:#ffcdd2
    style MEMORY2 fill:#c8e6c9
    style DIRECT2 fill:#c8e6c9
```

### Indexes Pré-calculés

```mermaid
graph TB
    subgraph "Événement OrderCreated"
        ORDER_EVENT[OrderCreated Event]
    end

    subgraph "Mises à Jour Atomiques"
        PRIMARY[orders: HashMap]
        BY_USER[orders_by_user: HashMap]
        BY_STATUS[orders_by_status: HashMap]
        BY_DATE[orders_by_date: HashMap]
        ANALYTICS[user_analytics: HashMap]
    end

    subgraph "Requêtes O(1)"
        QUERY1[Commandes d'un utilisateur]
        QUERY2[Commandes par statut]
        QUERY3[Commandes par date]
        QUERY4[Analytics utilisateur]
    end

    ORDER_EVENT --> PRIMARY
    ORDER_EVENT --> BY_USER
    ORDER_EVENT --> BY_STATUS
    ORDER_EVENT --> BY_DATE
    ORDER_EVENT --> ANALYTICS

    BY_USER --> QUERY1
    BY_STATUS --> QUERY2
    BY_DATE --> QUERY3
    ANALYTICS --> QUERY4

    style ORDER_EVENT fill:#e1f5fe
    style PRIMARY fill:#f3e5f5
    style QUERY1 fill:#e8f5e8
```

## 🔧 Optimisations de Performance

### Logging et Snapshots

```mermaid
graph TD
    subgraph "Configuration Standard"
        LOG_ON[log_verbose: true]
        SNAP_10[snapshot_every: 10]
        PERF_SLOW[Performance: ~500 events/sec]
    end

    subgraph "Configuration Optimisée"
        LOG_OFF[log_verbose: false]
        SNAP_1000[snapshot_every: 1000]
        PERF_FAST[Performance: ~2000 events/sec]
    end

    subgraph "Impact des Optimisations"
        LESS_IO[Moins d'I/O disque]
        LESS_LOGS[Moins de logs console]
        FASTER[4x plus rapide]
    end

    LOG_ON --> LOG_OFF
    SNAP_10 --> SNAP_1000
    LOG_OFF --> LESS_LOGS
    SNAP_1000 --> LESS_IO
    LESS_LOGS --> FASTER
    LESS_IO --> FASTER

    style LOG_OFF fill:#c8e6c9
    style SNAP_1000 fill:#c8e6c9
    style FASTER fill:#4caf50
```

### Benchmark Asynchrone

```mermaid
sequenceDiagram
    participant Bench as Benchmark
    participant Dispatch as Event Dispatcher
    participant Queue as Queue Async
    participant Worker as Worker Thread
    participant Store as Event Store

    Bench->>Dispatch: Envoyer 10K événements
    Note over Dispatch: Dispatch ultra-rapide (7ms)

    loop 10,000 événements
        Dispatch->>Queue: Ajouter à la queue
    end

    Bench->>Bench: Attendre traitement (5s)

    par Traitement Asynchrone
        Worker->>Queue: Récupérer événement
        Worker->>Store: Persister événement
        Store->>Store: Écrire sur disque
    end

    Bench->>Store: Compter événements persistés
    Store-->>Bench: 10,000 événements confirmés

    Note over Bench: Total: 5053ms (4x plus rapide)
```

## 🏗️ Architecture des Exemples

### Product App (E-commerce)

```mermaid
graph TB
    subgraph "Product App"
        subgraph "Routes HTTP"
            AUTH_ROUTE[/auth/login]
            PRODUCTS_ROUTE[/api/products]
            BENCHMARK_ROUTE[/api/admin/benchmark-engine]
        end

        subgraph "Événements Métier"
            PRODUCT_CREATED[ProductCreated]
            PRODUCT_UPDATED[ProductUpdated]
            USER_REGISTERED[UserRegistered]
        end

        subgraph "État Application"
            PRODUCTS_STATE[products: HashMap]
            USERS_STATE[users: HashMap]
            SECURITY_STATE[security: RBAC]
        end

        subgraph "Persistance"
            EVENTS_LOG[events.raftlog]
            STATE_SNAP[state.raftsnap]
        end
    end

    AUTH_ROUTE --> USER_REGISTERED
    PRODUCTS_ROUTE --> PRODUCT_CREATED
    PRODUCTS_ROUTE --> PRODUCT_UPDATED
    BENCHMARK_ROUTE --> PRODUCT_CREATED

    PRODUCT_CREATED --> PRODUCTS_STATE
    USER_REGISTERED --> USERS_STATE

    PRODUCTS_STATE --> EVENTS_LOG
    USERS_STATE --> STATE_SNAP
```

### IoT Timeseries

```mermaid
graph TB
    subgraph "IoT Timeseries"
        subgraph "Routes HTTP"
            STATS_ROUTE[/api/stats]
            GENERATE_ROUTE[/api/generate-fresh]
            DUPLICATES_ROUTE[/api/test-duplicates]
        end

        subgraph "Événements IoT"
            BATCH_READINGS[BatchReadingsAdded]
            SENSOR_READING[SensorReading]
        end

        subgraph "État IoT"
            SENSORS_STATE[sensors: HashMap]
            READINGS_STATE[recent_readings: Vec]
            LOCATION_INDEX[location_index: HashMap]
        end

        subgraph "Mode Adaptatif"
            EAGER_LOADING[EagerLoading Mode]
            MEMORY_USAGE[Memory Usage Tracking]
        end
    end

    GENERATE_ROUTE --> BATCH_READINGS
    BATCH_READINGS --> SENSOR_READING
    SENSOR_READING --> SENSORS_STATE
    SENSOR_READING --> READINGS_STATE
    SENSORS_STATE --> LOCATION_INDEX

    READINGS_STATE --> EAGER_LOADING
    EAGER_LOADING --> MEMORY_USAGE
    STATS_ROUTE --> MEMORY_USAGE
```

## 🎯 Flux de Données Complet

### Injection de Données Massive

```mermaid
flowchart LR
    subgraph "Injection Massive"
        SCRIPT[Script d'injection]
        BATCHES[Batches de données]
        TEMP_FILES[Fichiers temporaires]
    end

    subgraph "Serveur Lithair"
        HTTP_SERVER[Serveur HTTP]
        EVENT_QUEUE[Queue d'événements]
        WORKER[Worker Thread]
    end

    subgraph "Persistance"
        EVENT_LOG[Event Log]
        SNAPSHOTS[Snapshots]
        DEDUP_INDEX[Index Dédup]
    end

    SCRIPT --> BATCHES
    BATCHES --> TEMP_FILES
    TEMP_FILES --> HTTP_SERVER
    HTTP_SERVER --> EVENT_QUEUE
    EVENT_QUEUE --> WORKER
    WORKER --> EVENT_LOG
    WORKER --> SNAPSHOTS
    WORKER --> DEDUP_INDEX

    style SCRIPT fill:#e3f2fd
    style WORKER fill:#f1f8e9
    style EVENT_LOG fill:#fff3e0
```

## 📈 Métriques de Performance

### Comparaison Avant/Après Optimisations

```mermaid
xychart-beta
    title "Performance Benchmark (événements/sec)"
    x-axis [Avant, Après]
    y-axis "Événements par seconde" 0 --> 2500
    bar [500, 2000]
```

### Utilisation Mémoire par Taille de Dataset

```mermaid
xychart-beta
    title "Utilisation Mémoire"
    x-axis ["10MB", "100MB", "500MB", "1GB"]
    y-axis "Mémoire (MB)" 0 --> 1200
    line [12, 120, 600, 1200]
```

## 🔍 Points Clés à Retenir

### Avantages Architecturaux

1. **Un seul binaire** - Pas de base de données externe
2. **Accès mémoire direct** - 1,000,000x plus rapide que SQL
3. **Event sourcing natif** - Audit trail complet
4. **Indexes pré-calculés** - Requêtes O(1)
5. **Déduplication automatique** - Idempotence garantie

### Optimisations Appliquées

1. **Logging désactivé** - 4x amélioration des performances
2. **Snapshots moins fréquents** - Réduction I/O disque
3. **Timeout adaptatif** - Attente optimisée pour l'asynchrone
4. **Persistance binaire** - Option pour performances extrêmes

### Cas d'Usage Optimaux

- **Applications web** avec données < 500MB
- **SaaS multi-tenant** avec isolation
- **Dashboards temps réel** nécessitant latence ultra-faible
- **Prototypage rapide** sans configuration base de données

Cette architecture disruptif permet à Lithair de livrer des performances exceptionnelles tout en maintenant la simplicité d'un déploiement mono-binaire mais qui est aussi compatible avec des applications plus complexes.

Ce binaire n'est pas pour autant "dans son coin", puisque que, comme son nom l'indique, il peut créer plusieurs instances afin d'avoir un cluster géré grâce au protocole Raft.

## 🎯 Key Benefits of the Lithair Approach

### Eliminates Architecture Complexity

- **No 3-tier setup** - Single binary for simple apps
- **No external dependencies** - Everything embedded
- **No manual historization** - Declared upfront
- **No custom backup systems** - Built-in lifecycle management

### Intelligent Data Management

- **Think lifecycle first** - Design data behavior from day one
- **Automatic optimization** - Different strategies per field type
- **Smart storage** - Only store what you need, how you need it
- **Built-in compliance** - Audit trails where required

### Developer Experience

- **Declarative approach** - Describe what you want, not how
- **Integrated tooling** - Backup, recovery, replication included
- **Performance by design** - Lifecycle rules optimize automatically
- **Migration friendly** - Change lifecycle rules as needs evolve

### Perfect For Medium-Scale Applications

- **Not replacing massive databases** - Targeting medium-scale apps
- **Eliminating over-engineering** - Right-sized for most use cases
- **Faster development** - Focus on business logic, not infrastructure
- **Easier maintenance** - Single binary, declarative rules
- **Dashboards temps réel** nécessitant latence ultra-faible
- **Prototypage rapide** sans configuration base de données

Cette architecture disruptif permet à Lithair de livrer des performances exceptionnelles tout en maintenant la simplicité d'un déploiement mono-binaire.
