# Lithair System Overview - Complete Guide with Diagrams

_Created by Yoan Roblet - Disruptive application framework with AI assistance_

## The Lithair Philosophy

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

## System Architecture

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
        SINGLE["Single Binary\n- HTTP Server\n- Business Logic\n- Event Store\n- Declarative Lifecycle"]
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

## Event Flow (Event Sourcing)

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

## Startup and Recovery

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
    INIT_HTTP --> READY[Application Ready]

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

## Performance Architecture

### Data Access

```mermaid
graph LR
    subgraph "Traditional Approach"
        CLIENT1[Client] --> API1[API Server]
        API1 --> DB1[(Database)]
        DB1 --> NETWORK1[Network 1-10ms]
        NETWORK1 --> QUERY1[SQL Parsing]
        QUERY1 --> DISK1[Disk Access]
        DISK1 --> SERIALIZE1[Serialization]
    end

    subgraph "Lithair Approach"
        CLIENT2[Client] --> API2[Lithair]
        API2 --> MEMORY2[In-Memory HashMap]
        MEMORY2 --> DIRECT2[Direct Access 5ns]
    end

    style NETWORK1 fill:#ffcdd2
    style DISK1 fill:#ffcdd2
    style SERIALIZE1 fill:#ffcdd2
    style MEMORY2 fill:#c8e6c9
    style DIRECT2 fill:#c8e6c9
```

### Pre-calculated Indexes

```mermaid
graph TB
    subgraph "OrderCreated Event"
        ORDER_EVENT[OrderCreated Event]
    end

    subgraph "Atomic Updates"
        PRIMARY[orders: HashMap]
        BY_USER[orders_by_user: HashMap]
        BY_STATUS[orders_by_status: HashMap]
        BY_DATE[orders_by_date: HashMap]
        ANALYTICS[user_analytics: HashMap]
    end

    subgraph "O(1) Queries"
        QUERY1[Orders by user]
        QUERY2[Orders by status]
        QUERY3[Orders by date]
        QUERY4[User analytics]
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

## Performance Optimizations

### Logging and Snapshots

```mermaid
graph TD
    subgraph "Standard Configuration"
        LOG_ON[log_verbose: true]
        SNAP_10[snapshot_every: 10]
        PERF_SLOW[Performance: ~500 events/sec]
    end

    subgraph "Optimized Configuration"
        LOG_OFF[log_verbose: false]
        SNAP_1000[snapshot_every: 1000]
        PERF_FAST[Performance: ~2000 events/sec]
    end

    subgraph "Optimization Impact"
        LESS_IO[Less disk I/O]
        LESS_LOGS[Fewer console logs]
        FASTER[4x faster]
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

### Async Benchmark

```mermaid
sequenceDiagram
    participant Bench as Benchmark
    participant Dispatch as Event Dispatcher
    participant Queue as Async Queue
    participant Worker as Worker Thread
    participant Store as Event Store

    Bench->>Dispatch: Send 10K events
    Note over Dispatch: Ultra-fast dispatch (7ms)

    loop 10,000 events
        Dispatch->>Queue: Add to queue
    end

    Bench->>Bench: Wait for processing (5s)

    par Async Processing
        Worker->>Queue: Fetch event
        Worker->>Store: Persist event
        Store->>Store: Write to disk
    end

    Bench->>Store: Count persisted events
    Store-->>Bench: 10,000 events confirmed

    Note over Bench: Total: 5053ms (4x faster)
```

## Example Architectures

### Product App (E-commerce)

```mermaid
graph TB
    subgraph "Product App"
        subgraph "HTTP Routes"
            AUTH_ROUTE[/auth/login]
            PRODUCTS_ROUTE[/api/products]
            BENCHMARK_ROUTE[/api/admin/benchmark-engine]
        end

        subgraph "Business Events"
            PRODUCT_CREATED[ProductCreated]
            PRODUCT_UPDATED[ProductUpdated]
            USER_REGISTERED[UserRegistered]
        end

        subgraph "Application State"
            PRODUCTS_STATE[products: HashMap]
            USELT_STATE[users: HashMap]
            SECURITY_STATE[security: RBAC]
        end

        subgraph "Persistence"
            EVENTS_LOG[events.raftlog]
            STATE_SNAP[state.raftsnap]
        end
    end

    AUTH_ROUTE --> USER_REGISTERED
    PRODUCTS_ROUTE --> PRODUCT_CREATED
    PRODUCTS_ROUTE --> PRODUCT_UPDATED
    BENCHMARK_ROUTE --> PRODUCT_CREATED

    PRODUCT_CREATED --> PRODUCTS_STATE
    USER_REGISTERED --> USELT_STATE

    PRODUCTS_STATE --> EVENTS_LOG
    USELT_STATE --> STATE_SNAP
```

### IoT Timeseries

```mermaid
graph TB
    subgraph "IoT Timeseries"
        subgraph "HTTP Routes"
            STATS_ROUTE[/api/stats]
            GENERATE_ROUTE[/api/generate-fresh]
            DUPLICATES_ROUTE[/api/test-duplicates]
        end

        subgraph "IoT Events"
            BATCH_READINGS[BatchReadingsAdded]
            SENSOR_READING[SensorReading]
        end

        subgraph "IoT State"
            SENSOLT_STATE[sensors: HashMap]
            READINGS_STATE[recent_readings: Vec]
            LOCATION_INDEX[location_index: HashMap]
        end

        subgraph "Adaptive Mode"
            EAGER_LOADING[EagerLoading Mode]
            MEMORY_USAGE[Memory Usage Tracking]
        end
    end

    GENERATE_ROUTE --> BATCH_READINGS
    BATCH_READINGS --> SENSOR_READING
    SENSOR_READING --> SENSOLT_STATE
    SENSOR_READING --> READINGS_STATE
    SENSOLT_STATE --> LOCATION_INDEX

    READINGS_STATE --> EAGER_LOADING
    EAGER_LOADING --> MEMORY_USAGE
    STATS_ROUTE --> MEMORY_USAGE
```

## Complete Data Flow

### Massive Data Injection

```mermaid
flowchart LR
    subgraph "Massive Injection"
        SCRIPT[Injection Script]
        BATCHES[Data Batches]
        TEMP_FILES[Temporary Files]
    end

    subgraph "Lithair Server"
        HTTP_SERVER[HTTP Server]
        EVENT_QUEUE[Event Queue]
        WORKER[Worker Thread]
    end

    subgraph "Persistence"
        EVENT_LOG[Event Log]
        SNAPSHOTS[Snapshots]
        DEDUP_INDEX[Dedup Index]
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

## Performance Metrics

### Before/After Optimization Comparison

```mermaid
xychart-beta
    title "Performance Benchmark (events/sec)"
    x-axis [Before, After]
    y-axis "Events per second" 0 --> 2500
    bar [500, 2000]
```

### Memory Usage by Dataset Size

```mermaid
xychart-beta
    title "Memory Usage"
    x-axis ["10MB", "100MB", "500MB", "1GB"]
    y-axis "Memory (MB)" 0 --> 1200
    line [12, 120, 600, 1200]
```

## Key Takeaways

### Architectural Advantages

1. **Single binary** - No external database
2. **Direct memory access** - 1,000,000x faster than SQL
3. **Native event sourcing** - Complete audit trail
4. **Pre-calculated indexes** - O(1) queries
5. **Automatic deduplication** - Guaranteed idempotence

### Applied Optimizations

1. **Logging disabled** - 4x performance improvement
2. **Less frequent snapshots** - Reduced disk I/O
3. **Adaptive timeout** - Optimized async waiting
4. **Binary persistence** - Option for extreme performance

### Optimal Use Cases

- **Web applications** with data < 500MB
- **Multi-tenant SaaS** with isolation
- **Real-time dashboards** requiring ultra-low latency
- **Rapid prototyping** without database configuration

## Key Benefits of the Lithair Approach

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

This disruptive architecture allows Lithair to deliver exceptional performance while maintaining the simplicity of a single-binary deployment.

The binary is not isolated - as the name suggests, it can create multiple instances to form a cluster managed via the Raft consensus protocol.
