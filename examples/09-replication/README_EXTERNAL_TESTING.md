# 🔥 Lithair External CURL Testing

## 🎯 **What This Demonstrates**

This external test proves Lithair's robustness under real-world conditions:

- **Independent HTTP nodes** functioning as separate servers
- **Parallel CURL requests** from outside the process
- **Real distributed consensus** using standard tools (curl, bash, jq)

## 🚀 **Quick Start**

### 1. Build the External Binary

```bash
cargo build --release --bin external_cluster_node
```

### 2. Start the Cluster (Terminal 1)

```bash
./start_cluster.sh
```

This launches 3 independent nodes:

- **Leader**: Port 8081 (Node 1)
- **Follower1**: Port 8082 (Node 2)
- **Follower2**: Port 8083 (Node 3)

### 3. Run the External Benchmark (Terminal 2)

```bash
./external_curl_benchmark.sh
```

### 4. Verify Consistency (Terminal 3)

```bash
./verify_cluster.sh
```

## 🌐 **External Test Architecture**

### Lithair Cluster

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   LEADER 8081   │    │ FOLLOWER1 8082  │    │ FOLLOWER2 8083  │
│                 │    │                 │    │                 │
│ DeclarativeModel│◄──►│ DeclarativeModel│◄──►│ DeclarativeModel│
│ + EventStore    │    │ + EventStore    │    │ + EventStore    │
│ + HTTP Server   │    │ + HTTP Server   │    │ + HTTP Server   │
└─────────────────┘    └─────────────────┘    └─────────────────┘
         ▲                       ▲                       ▲
         │                       │                       │
         └───────────────────────┼───────────────────────┘
                                 │
                    ┌─────────────────────────┐
                    │  EXTERNAL CURL BENCHMARK │
                    │                         │
                    │  - 600 random CRUD ops │
                    │  - 10 concurrent jobs   │
                    │  - Real HTTP requests   │
                    └─────────────────────────┘
```

### Test Flow

1. **start_cluster.sh** -- Launches 3 independent processes on different ports
2. **external_curl_benchmark.sh** -- Sends 600 parallel CURL requests
3. **verify_cluster.sh** -- Verifies data consistency

## 🔧 **Auto-Generated API Endpoints**

Each node automatically exposes these REST endpoints:

### Products (Full CRUD)

- `GET /api/consensus_products` - List all products
- `POST /api/consensus_products` - Create a product
- `GET /api/consensus_products/{id}` - Get a product by ID
- `PUT /api/consensus_products/{id}` - Update a product
- `DELETE /api/consensus_products/{id}` - Delete a product

### Administration

- `GET /status` - Node status (ID, role, product count)
- `POST /api/consensus_products/_replicate` - Internal replication

## 📊 **Manual CURL Tests**

### Create a Product

```bash
curl -X POST http://127.0.0.1:8081/api/consensus_products \
     -H 'Content-Type: application/json' \
     -d '{"name":"External Test Product","price":199.99,"category":"External"}'
```

### List All Products

```bash
curl http://127.0.0.1:8081/api/consensus_products | jq
```

### Check Cluster Status

```bash
curl http://127.0.0.1:8081/status | jq
curl http://127.0.0.1:8082/status | jq
curl http://127.0.0.1:8083/status | jq
```

### Get a Specific Product

```bash
# Use an ID from the previous listing
curl http://127.0.0.1:8081/api/consensus_products/{product-uuid} | jq
```

## 🎯 **What the Benchmark Proves**

### ✅ **Real Distributed Consensus**

- Each CURL request targets a different node
- Data replicates automatically between nodes
- Eventual consistency guaranteed across all nodes

### ✅ **Working DeclarativeModel**

- A single `ConsensusProduct` struct generates the entire REST API
- Automatic data validation via `#[http(validate)]`
- Automatic RBAC via `#[permission()]`
- Automatic EventStore via `#[persistence()]`

### ✅ **Real-World Performance**

- 600+ simultaneous external HTTP requests
- 10 concurrent CURL jobs per node
- Real-time replication between nodes

### ✅ **Operational Robustness**

- Completely independent nodes (separate processes)
- Network fault resilience
- Per-node logging and monitoring

## 📁 **File Structure**

```
examples/09-replication/
├── external_cluster_node.rs      # HTTP server with DeclarativeModel
├── start_cluster.sh              # Launches 3 independent nodes
├── external_curl_benchmark.sh    # External CURL benchmark
├── verify_cluster.sh             # Consistency verification
├── data/
│   ├── external_node_1/
│   │   ├── node.log              # Leader logs
│   │   ├── node.pid              # Process PID
│   │   └── consensus_products.events/
│   │       └── events.raftlog    # EventStore persistence
│   ├── external_node_2/          # Follower 1 data
│   └── external_node_3/          # Follower 2 data
```

## 🏆 **Expected Results**

### Benchmark Performance

```
🔥 BENCHMARK RESULTS
=================================
✅ Total operations: 600
⏱️  Total time: 2.40s
📊 Throughput: 250.00 ops/sec
```

### Consistency Verification

```
🔍 VERIFICATION: IDENTICAL DATA on ALL NODES
==============================================
👑 Leader (port 8081):    347 products
📡 Follower1 (port 8082): 347 products
📡 Follower2 (port 8083): 347 products

🎉 SUCCESS: Perfect data consistency!
   All 347 products identical across all nodes
   TRUE distributed consensus achieved! 🚀
```

## 🛠️ **Troubleshooting**

### Problem: Nodes Won't Start

```bash
# Check if ports are in use
lsof -i :8081
lsof -i :8082
lsof -i :8083

# Manually clean up
killall external_cluster_node
```

### Problem: Data Inconsistency

```bash
# Check the logs
tail -f data/external_node_1/node.log
tail -f data/external_node_2/node.log
tail -f data/external_node_3/node.log

# Restart the cluster
./start_cluster.sh
```

### Problem: CURL Benchmark Fails

```bash
# Check connectivity
curl -v http://127.0.0.1:8081/status
curl -v http://127.0.0.1:8082/status
curl -v http://127.0.0.1:8083/status

# Install dependencies if needed
sudo apt install curl jq bc  # Ubuntu/Debian
```

## 🎭 **Differences from the Internal Test**

| Aspect        | Historical internal test             | External test (CURL)                |
| ------------- | ------------------------------------ | ----------------------------------- |
| **Process**   | Single binary with 3 simulated nodes | 3 separate processes + CURL script  |
| **Network**   | In-memory HTTP simulation            | Real HTTP TCP calls                 |
| **Isolation** | Shared threads                       | Completely isolated processes       |
| **Realism**   | High-performance simulation          | Real production conditions          |
| **Debugging** | Unified logs                         | Separate logs per node              |
| **Tools**     | Pure Rust code                       | Standard tools (curl, bash, jq)     |

## 🎯 **Conclusion**

This external test **conclusively proves** that Lithair works under real-world conditions:

- ✅ Completely independent nodes with distributed consensus
- ✅ Auto-generated REST API via DeclarativeModel accessible through CURL
- ✅ Performance and consistency maintained with external requests
- ✅ Operational robustness with standard monitoring tools

**Lithair passes the real-world test!** 🚀
