#!/bin/bash

# Lithair High-Performance CRUD Operations Parallel Benchmark
# Tests distributed replication with high concurrency across 3 nodes

set -e

# ========== CONFIGURATION ==========
# You can modify these values for different test scenarios

# Total operations to perform PER NODE (configurable)
PER_NODE_OPS=${1:-10000}  # Default 10k per node, override with: ./script.sh 50000

# Node ports
LEADER_PORT=8080
FOLLOWER1_PORT=8081
FOLLOWER2_PORT=8082
NODES=3
SINGLE_NODE=${SINGLE_NODE:-0}
if [ "$SINGLE_NODE" = "1" ]; then
  NODES=1
fi
TOTAL_OPS=$((PER_NODE_OPS * NODES))

# High-performance tuning (NO DELAYS)
CONCURRENT_BATCH_SIZE=1000000   # No batching delays
OPERATION_DELAY=0               # No delays between operations
REPLICATION_WAIT=10             # Wait for replication stabilization
MAX_HTTP_TIMEOUT=10             # Allow redirects/processing under load

# High-performance mode: Random CRUD with no delays
CREATE_PERCENTAGE=${CREATE_PERCENTAGE:-80}          # 80% CREATE operations 
READ_PERCENTAGE=${READ_PERCENTAGE:-15}              # 15% READ operations  
UPDATE_PERCENTAGE=${UPDATE_PERCENTAGE:-5}           # 5% UPDATE operations
DELETE_PERCENTAGE=${DELETE_PERCENTAGE:-0}           # 0% DELETE operations (default)

echo "=================================================="
echo "Total Operations: $TOTAL_OPS ($PER_NODE_OPS per node × $NODES nodes)"
if [ "$SINGLE_NODE" = "1" ]; then
  echo "Nodes: $NODES (Single node on port $LEADER_PORT)"
else
  echo "Nodes: $NODES (Leader: $LEADER_PORT, Followers: $FOLLOWER1_PORT, $FOLLOWER2_PORT)"
fi
echo "Performance: $CONCURRENT_BATCH_SIZE ops/batch, ${OPERATION_DELAY}s delay"
PRINT_FULL_LISTS=true
if [ "$TOTAL_OPS" -gt 2000 ]; then
  PRINT_FULL_LISTS=false
fi
echo "Distribution: ${CREATE_PERCENTAGE}% CREATE, ${READ_PERCENTAGE}% READ, ${UPDATE_PERCENTAGE}% UPDATE, ${DELETE_PERCENTAGE}% DELETE"
echo ""

# High-throughput EventStore tuning via environment
export LT_EVENT_MAX_BATCH=${LT_EVENT_MAX_BATCH:-65536}
export LT_FLUSH_INTERVAL_MS=${LT_FLUSH_INTERVAL_MS:-5}
export LT_FSYNC_ON_APPEND=${LT_FSYNC_ON_APPEND:-0}
export RUST_LOG=${RUST_LOG:-warn}

# Storage profiles: high_throughput (default), balanced, durable_security
STORAGE_PROFILE=${STORAGE_PROFILE:-high_throughput}
case "$STORAGE_PROFILE" in
  high_throughput)
    export LT_OPT_PERSIST=1
    export LT_ENABLE_BINARY=${LT_ENABLE_BINARY:-1}
    export LT_DISABLE_INDEX=${LT_DISABLE_INDEX:-1}
    export LT_DEDUP_PERSIST=${LT_DEDUP_PERSIST:-0}
    export LT_BUFFER_SIZE=${LT_BUFFER_SIZE:-8388608}
    export LT_MAX_EVENTS_BUFFER=${LT_MAX_EVENTS_BUFFER:-10000}
    export LT_FLUSH_INTERVAL_MS=${LT_FLUSH_INTERVAL_MS:-2}
    export LT_EVENT_MAX_BATCH=${LT_EVENT_MAX_BATCH:-10000}
    export LT_FSYNC_ON_APPEND=${LT_FSYNC_ON_APPEND:-0}
    export LT_SNAPSHOT_EVERY=${LT_SNAPSHOT_EVERY:-10000000}
    ;;
  balanced)
    export LT_OPT_PERSIST=1
    export LT_ENABLE_BINARY=${LT_ENABLE_BINARY:-1}
    export LT_DISABLE_INDEX=${LT_DISABLE_INDEX:-0}
    export LT_DEDUP_PERSIST=${LT_DEDUP_PERSIST:-1}
    export LT_BUFFER_SIZE=${LT_BUFFER_SIZE:-2097152}
    export LT_MAX_EVENTS_BUFFER=${LT_MAX_EVENTS_BUFFER:-2000}
    export LT_FLUSH_INTERVAL_MS=${LT_FLUSH_INTERVAL_MS:-5}
    export LT_EVENT_MAX_BATCH=${LT_EVENT_MAX_BATCH:-2000}
    export LT_FSYNC_ON_APPEND=${LT_FSYNC_ON_APPEND:-0}
    export LT_SNAPSHOT_EVERY=${LT_SNAPSHOT_EVERY:-500000}
    ;;
  durable_security)
    export LT_OPT_PERSIST=1
    export LT_ENABLE_BINARY=${LT_ENABLE_BINARY:-0}
    export LT_DISABLE_INDEX=${LT_DISABLE_INDEX:-0}
    export LT_DEDUP_PERSIST=${LT_DEDUP_PERSIST:-1}
    export LT_BUFFER_SIZE=${LT_BUFFER_SIZE:-1048576}
    export LT_MAX_EVENTS_BUFFER=${LT_MAX_EVENTS_BUFFER:-1000}
    export LT_FLUSH_INTERVAL_MS=${LT_FLUSH_INTERVAL_MS:-10}
    export LT_EVENT_MAX_BATCH=${LT_EVENT_MAX_BATCH:-1000}
    export LT_FSYNC_ON_APPEND=${LT_FSYNC_ON_APPEND:-1}
    export LT_SNAPSHOT_EVERY=${LT_SNAPSHOT_EVERY:-100000}
    ;;
  *)
    echo "Unknown STORAGE_PROFILE='$STORAGE_PROFILE'. Using defaults."
    ;;
esac

echo "Storage profile: $STORAGE_PROFILE"
echo "EventStore tuning: LT_EVENT_MAX_BATCH=$LT_EVENT_MAX_BATCH, LT_FLUSH_INTERVAL_MS=${LT_FLUSH_INTERVAL_MS}ms, LT_FSYNC_ON_APPEND=$LT_FSYNC_ON_APPEND"
echo "Optimized persistence: LT_OPT_PERSIST=${LT_OPT_PERSIST:-unset}, LT_ENABLE_BINARY=${LT_ENABLE_BINARY:-unset}, LT_DISABLE_INDEX=${LT_DISABLE_INDEX:-unset}, LT_DEDUP_PERSIST=${LT_DEDUP_PERSIST:-unset}"
echo "Buffers: LT_BUFFER_SIZE=${LT_BUFFER_SIZE:-unset}, LT_MAX_EVENTS_BUFFER=${LT_MAX_EVENTS_BUFFER:-unset}, LT_SNAPSHOT_EVERY=${LT_SNAPSHOT_EVERY:-unset}"

# 0. Kill any existing benchmark/node processes
echo "🧹 Step 0: Killing existing Lithair nodes..."
pkill -f "target/release/replication-declarative-node" 2>/dev/null || true
pkill -f "replication-declarative-node" 2>/dev/null || true

# Free the expected ports if still occupied
for port in $LEADER_PORT $FOLLOWER1_PORT $FOLLOWER2_PORT; do
    if lsof -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1; then
        echo "   Killing process on port $port"
        lsof -tiTCP:"$port" -sTCP:LISTEN | xargs -r kill -9 || true
    fi
done
sleep 2
echo "✅ Existing processes terminated (if any)"
echo ""

# 1. Clean data directory (use EXPERIMENT_DATA_BASE if set or default)
EXPERIMENT_DATA_BASE=${EXPERIMENT_DATA_BASE:-examples/replication/data}
export EXPERIMENT_DATA_BASE
echo "🗑️ Step 1: Cleaning data directory at $EXPERIMENT_DATA_BASE ..."
if [ -d "$EXPERIMENT_DATA_BASE" ]; then
    rm -rf "$EXPERIMENT_DATA_BASE"
    echo "✅ Data directory cleaned"
else
    echo "✅ Data directory already clean"
fi
# Also clean legacy root data/ used by DeclarativeCluster
if [ -d "data" ]; then
  echo "🗑️ Cleaning legacy root data/ directory to avoid stale logs"
  rm -rf data
fi
# Also clean legacy root raft/ directory
if [ -d "raft" ]; then
  echo "🗑️ Cleaning legacy root raft/ directory"
  rm -rf raft
fi
mkdir -p "$EXPERIMENT_DATA_BASE"
echo ""

# 2. Build project
echo "🔨 Step 2: Building Lithair..."
cargo build --release --bin replication-declarative-node
cargo build --release -p replication --bin replication-loadgen
echo "✅ Build completed"
echo ""

# 3. Start 3 nodes
echo "🏗️ Step 3: Starting Lithair node(s)..."
echo "Starting Node 1 (Leader) on port $LEADER_PORT..."
if [ "$SINGLE_NODE" = "1" ]; then
  cargo run --release --bin replication-declarative-node -- --node-id 1 --port $LEADER_PORT > node1_bench.log 2>&1 &
  NODE1_PID=$!
else
  cargo run --release --bin replication-declarative-node -- --node-id 1 --port $LEADER_PORT --peers "$FOLLOWER1_PORT,$FOLLOWER2_PORT" > node1_bench.log 2>&1 &
  NODE1_PID=$!

  sleep 3

  echo "Starting Node 2 (Follower) on port $FOLLOWER1_PORT..."
  cargo run --release --bin replication-declarative-node -- --node-id 2 --port $FOLLOWER1_PORT --peers "$LEADER_PORT,$FOLLOWER2_PORT" > node2_bench.log 2>&1 &
  NODE2_PID=$!

  sleep 3

  echo "Starting Node 3 (Follower) on port $FOLLOWER2_PORT..."
  cargo run --release --bin replication-declarative-node -- --node-id 3 --port $FOLLOWER2_PORT --peers "$LEADER_PORT,$FOLLOWER1_PORT" > node3_bench.log 2>&1 &
  NODE3_PID=$!
fi

# Wait for nodes to initialize
echo "⏳ Waiting for nodes to initialize and elect leader..."

# Robust per-port readiness check (up to 30s each)
echo "🔍 Testing node connectivity..."
PORTS="$LEADER_PORT"
if [ "$SINGLE_NODE" != "1" ]; then
  PORTS="$LEADER_PORT $FOLLOWER1_PORT $FOLLOWER2_PORT"
fi
for port in $PORTS; do
  echo -n "  Waiting for port $port to be ready"
  READY=0
  for i in $(seq 1 30); do
    if curl -s -f http://127.0.0.1:$port/status > /dev/null; then
      READY=1
      break
    fi
    echo -n "."
    sleep 1
  done
  echo ""
  if [ "$READY" = "1" ]; then
    echo "✅ Node on port $port is ready"
  else
    echo "❌ Node on port $port did not become ready in time"
    exit 1
  fi
done
echo ""

# 4. Launch benchmark
echo "🚀 Step 4: Launching random CRUD operations via replication-loadgen (80% CREATE, 15% READ, 5% UPDATE)..."
START_TIME=$(date +%s)

# Use the Rust HTTP load generator exclusively
LOADGEN_CONCURRENCY=${LOADGEN_CONCURRENCY:-256}
LOADGEN_BULK_SIZE=${LOADGEN_BULK_SIZE:-100}
LOADGEN_MODE=${LOADGEN_MODE:-random}
LOADGEN_TIMEOUT_S=${LOADGEN_TIMEOUT_S:-10}
LEADER_URL=${LEADER_URL:-http://127.0.0.1:$LEADER_PORT}
READ_TARGETS=${READ_TARGETS:-http://127.0.0.1:$LEADER_PORT,http://127.0.0.1:$FOLLOWER1_PORT,http://127.0.0.1:$FOLLOWER2_PORT}
if [ "$SINGLE_NODE" = "1" ]; then
  READ_TARGETS="http://127.0.0.1:$LEADER_PORT"
fi
READ_PATH=${READ_PATH:-/api/products}
LIGHT_READS=${LIGHT_READS:-0}
case "$LIGHT_READS" in
  1|true|status)
    READ_PATH="/status"
    ;;
  count)
    READ_PATH="/api/products/count"
    ;;
esac

echo "Using Rust HTTP load generator (mode=$LOADGEN_MODE)"
echo "Loadgen params per node: total=$PER_NODE_OPS, concurrency=$LOADGEN_CONCURRENCY, bulk_size=$LOADGEN_BULK_SIZE, timeout_s=$LOADGEN_TIMEOUT_S"
echo "Read targets: $READ_TARGETS"
echo "Read path: $READ_PATH (LIGHT_READS=$LIGHT_READS)"

# Optional pre-seed phase to populate IDs before main workload
PRESEED_PER_NODE=${PRESEED_PER_NODE:-0}
if [ "$PRESEED_PER_NODE" -gt 0 ]; then
  echo "🌱 Pre-seeding dataset: $PRESEED_PER_NODE items per node (total $((PRESEED_PER_NODE * NODES)))"
  SEED_TOTAL=$((PRESEED_PER_NODE * NODES))
  target/release/replication-loadgen \
    --leader "$LEADER_URL" \
    --total "$SEED_TOTAL" \
    --concurrency "$LOADGEN_CONCURRENCY" \
    --bulk-size "$LOADGEN_BULK_SIZE" \
    --mode "bulk" \
    --create-pct 100 \
    --read-pct 0 \
    --update-pct 0 \
    --delete-pct "$DELETE_PERCENTAGE" \
    --read-targets "$READ_TARGETS" \
    --read-path "$READ_PATH" \
    --timeout-s "$LOADGEN_TIMEOUT_S"
  echo "✅ Pre-seed completed"
fi

BENCH_SUITE=${BENCH_SUITE:-}
if [ -z "$BENCH_SUITE" ]; then
  if [ "$SINGLE_NODE" = "1" ]; then
    # Single-node: one load generator instance
    target/release/replication-loadgen \
      --leader "$LEADER_URL" \
      --total "$PER_NODE_OPS" \
      --concurrency "$LOADGEN_CONCURRENCY" \
      --bulk-size "$LOADGEN_BULK_SIZE" \
      --mode "$LOADGEN_MODE" \
      --create-pct "$CREATE_PERCENTAGE" \
      --read-pct "$READ_PERCENTAGE" \
      --update-pct "$UPDATE_PERCENTAGE" \
      --delete-pct "$DELETE_PERCENTAGE" \
      --read-targets "$READ_TARGETS" \
      --read-path "$READ_PATH" \
      --timeout-s "$LOADGEN_TIMEOUT_S" &
    LG1=$!
    wait $LG1
    echo "✅ Loadgen completed in $(( $(date +%s) - START_TIME )) seconds"
    TOTAL_DONE=$PER_NODE_OPS
  else
    # Run one load generator per node worth of operations (all writes still target the leader)
    target/release/replication-loadgen \
      --leader "$LEADER_URL" \
      --total "$PER_NODE_OPS" \
      --concurrency "$LOADGEN_CONCURRENCY" \
      --bulk-size "$LOADGEN_BULK_SIZE" \
      --mode "$LOADGEN_MODE" \
      --create-pct "$CREATE_PERCENTAGE" \
      --read-pct "$READ_PERCENTAGE" \
      --update-pct "$UPDATE_PERCENTAGE" \
      --delete-pct "$DELETE_PERCENTAGE" \
      --read-targets "$READ_TARGETS" \
      --read-path "$READ_PATH" \
      --timeout-s "$LOADGEN_TIMEOUT_S" &
    LG1=$!

    target/release/replication-loadgen \
      --leader "$LEADER_URL" \
      --total "$PER_NODE_OPS" \
      --concurrency "$LOADGEN_CONCURRENCY" \
      --bulk-size "$LOADGEN_BULK_SIZE" \
      --mode "$LOADGEN_MODE" \
      --create-pct "$CREATE_PERCENTAGE" \
      --read-pct "$READ_PERCENTAGE" \
      --update-pct "$UPDATE_PERCENTAGE" \
      --delete-pct "$DELETE_PERCENTAGE" \
      --read-targets "$READ_TARGETS" \
      --read-path "$READ_PATH" \
      --timeout-s "$LOADGEN_TIMEOUT_S" &
    LG2=$!

    target/release/replication-loadgen \
      --leader "$LEADER_URL" \
      --total "$PER_NODE_OPS" \
      --concurrency "$LOADGEN_CONCURRENCY" \
      --bulk-size "$LOADGEN_BULK_SIZE" \
      --mode "$LOADGEN_MODE" \
      --create-pct "$CREATE_PERCENTAGE" \
      --read-pct "$READ_PERCENTAGE" \
      --update-pct "$UPDATE_PERCENTAGE" \
      --delete-pct "$DELETE_PERCENTAGE" \
      --read-targets "$READ_TARGETS" \
      --read-path "$READ_PATH" \
      --timeout-s "$LOADGEN_TIMEOUT_S" &
    LG3=$!

    wait $LG1
    wait $LG2
    wait $LG3
    echo "✅ Loadgen completed in $(( $(date +%s) - START_TIME )) seconds"
    TOTAL_DONE=$TOTAL_OPS
  fi
else
  echo "📚 Running benchmark suite: $BENCH_SUITE"
  case "$BENCH_SUITE" in
    concurrency_scaling)
      CONC_LIST=${CONC_LIST:-"128 256 512 1024 2048"}
      for C in $CONC_LIST; do
        echo "\n➡️  Suite[concurrency_scaling] Running with concurrency=$C"
        if [ "$SINGLE_NODE" = "1" ]; then
          target/release/replication-loadgen \
            --leader "$LEADER_URL" \
            --total "$PER_NODE_OPS" \
            --concurrency "$C" \
            --bulk-size "$LOADGEN_BULK_SIZE" \
            --mode "$LOADGEN_MODE" \
            --create-pct "$CREATE_PERCENTAGE" \
            --read-pct "$READ_PERCENTAGE" \
            --update-pct "$UPDATE_PERCENTAGE" \
            --delete-pct "$DELETE_PERCENTAGE" \
            --read-targets "$READ_TARGETS" \
            --read-path "$READ_PATH" \
            --timeout-s "$LOADGEN_TIMEOUT_S"
        else
          target/release/replication-loadgen \
            --leader "$LEADER_URL" \
            --total "$PER_NODE_OPS" \
            --concurrency "$C" \
            --bulk-size "$LOADGEN_BULK_SIZE" \
            --mode "$LOADGEN_MODE" \
            --create-pct "$CREATE_PERCENTAGE" \
            --read-pct "$READ_PERCENTAGE" \
            --update-pct "$UPDATE_PERCENTAGE" \
            --delete-pct "$DELETE_PERCENTAGE" \
            --read-targets "$READ_TARGETS" \
            --read-path "$READ_PATH" \
            --timeout-s "$LOADGEN_TIMEOUT_S" & LG1=$!
          target/release/replication-loadgen \
            --leader "$LEADER_URL" \
            --total "$PER_NODE_OPS" \
            --concurrency "$C" \
            --bulk-size "$LOADGEN_BULK_SIZE" \
            --mode "$LOADGEN_MODE" \
            --create-pct "$CREATE_PERCENTAGE" \
            --read-pct "$READ_PERCENTAGE" \
            --update-pct "$UPDATE_PERCENTAGE" \
            --delete-pct "$DELETE_PERCENTAGE" \
            --read-targets "$READ_TARGETS" \
            --read-path "$READ_PATH" \
            --timeout-s "$LOADGEN_TIMEOUT_S" & LG2=$!
          target/release/replication-loadgen \
            --leader "$LEADER_URL" \
            --total "$PER_NODE_OPS" \
            --concurrency "$C" \
            --bulk-size "$LOADGEN_BULK_SIZE" \
            --mode "$LOADGEN_MODE" \
            --create-pct "$CREATE_PERCENTAGE" \
            --read-pct "$READ_PERCENTAGE" \
            --update-pct "$UPDATE_PERCENTAGE" \
            --delete-pct "$DELETE_PERCENTAGE" \
            --read-targets "$READ_TARGETS" \
            --read-path "$READ_PATH" \
            --timeout-s "$LOADGEN_TIMEOUT_S" & LG3=$!
          wait $LG1; wait $LG2; wait $LG3;
        fi
      done
      ;;
    durability_profiles)
      PROFILES=${PROFILES:-"high_throughput balanced durable_security"}
      for P in $PROFILES; do
        echo "\n➡️  Suite[durability_profiles] STORAGE_PROFILE=$P (restarting cluster)"
        # Recursively invoke this script so nodes are started with the right STORAGE_PROFILE
        STORAGE_PROFILE="$P" BENCH_SUITE= \
          LIGHT_READS="$LIGHT_READS" \
          CREATE_PERCENTAGE="$CREATE_PERCENTAGE" READ_PERCENTAGE="$READ_PERCENTAGE" \
          UPDATE_PERCENTAGE="$UPDATE_PERCENTAGE" DELETE_PERCENTAGE="$DELETE_PERCENTAGE" \
          LOADGEN_CONCURRENCY="$LOADGEN_CONCURRENCY" LOADGEN_BULK_SIZE="$LOADGEN_BULK_SIZE" \
          LOADGEN_MODE="$LOADGEN_MODE" LOADGEN_TIMEOUT_S="$LOADGEN_TIMEOUT_S" \
          SINGLE_NODE="$SINGLE_NODE" \
          bash "$0" "$PER_NODE_OPS"
      done
      ;;
    heavy_vs_light_reads)
      for LR in 0 count status; do
        case "$LR" in
          1|true|status)
            RP="/status";;
          count)
            RP="/api/products/count";;
          0|*)
            RP="/api/products";;
        esac
        echo "\n➡️  Suite[heavy_vs_light_reads] LIGHT_READS=$LR (read_path=$RP)"
        if [ "$SINGLE_NODE" = "1" ]; then
          target/release/replication-loadgen \
            --leader "$LEADER_URL" \
            --total "$PER_NODE_OPS" \
            --concurrency "$LOADGEN_CONCURRENCY" \
            --bulk-size "$LOADGEN_BULK_SIZE" \
            --mode "$LOADGEN_MODE" \
            --create-pct "$CREATE_PERCENTAGE" \
            --read-pct "$READ_PERCENTAGE" \
            --update-pct "$UPDATE_PERCENTAGE" \
            --delete-pct "$DELETE_PERCENTAGE" \
            --read-targets "$READ_TARGETS" \
            --read-path "$RP" \
            --timeout-s "$LOADGEN_TIMEOUT_S"
        else
          target/release/replication-loadgen \
            --leader "$LEADER_URL" \
            --total "$PER_NODE_OPS" \
            --concurrency "$LOADGEN_CONCURRENCY" \
            --bulk-size "$LOADGEN_BULK_SIZE" \
            --mode "$LOADGEN_MODE" \
            --create-pct "$CREATE_PERCENTAGE" \
            --read-pct "$READ_PERCENTAGE" \
            --update-pct "$UPDATE_PERCENTAGE" \
            --delete-pct "$DELETE_PERCENTAGE" \
            --read-targets "$READ_TARGETS" \
            --read-path "$RP" \
            --timeout-s "$LOADGEN_TIMEOUT_S" & LG1=$!
          target/release/replication-loadgen \
            --leader "$LEADER_URL" \
            --total "$PER_NODE_OPS" \
            --concurrency "$LOADGEN_CONCURRENCY" \
            --bulk-size "$LOADGEN_BULK_SIZE" \
            --mode "$LOADGEN_MODE" \
            --create-pct "$CREATE_PERCENTAGE" \
            --read-pct "$READ_PERCENTAGE" \
            --update-pct "$UPDATE_PERCENTAGE" \
            --delete-pct "$DELETE_PERCENTAGE" \
            --read-targets "$READ_TARGETS" \
            --read-path "$RP" \
            --timeout-s "$LOADGEN_TIMEOUT_S" & LG2=$!
          target/release/replication-loadgen \
            --leader "$LEADER_URL" \
            --total "$PER_NODE_OPS" \
            --concurrency "$LOADGEN_CONCURRENCY" \
            --bulk-size "$LOADGEN_BULK_SIZE" \
            --mode "$LOADGEN_MODE" \
            --create-pct "$CREATE_PERCENTAGE" \
            --read-pct "$READ_PERCENTAGE" \
            --update-pct "$UPDATE_PERCENTAGE" \
            --delete-pct "$DELETE_PERCENTAGE" \
            --read-targets "$READ_TARGETS" \
            --read-path "$RP" \
            --timeout-s "$LOADGEN_TIMEOUT_S" & LG3=$!
          wait $LG1; wait $LG2; wait $LG3;
        fi
      done
      ;;
    bulk_vs_single)
      for MODE in bulk single; do
        echo "\n➡️  Suite[bulk_vs_single] mode=$MODE"
        if [ "$SINGLE_NODE" = "1" ]; then
          target/release/replication-loadgen \
            --leader "$LEADER_URL" \
            --total "$PER_NODE_OPS" \
            --concurrency "$LOADGEN_CONCURRENCY" \
            --bulk-size "$LOADGEN_BULK_SIZE" \
            --mode "$MODE" \
            --create-pct 100 \
            --read-pct 0 \
            --update-pct 0 \
            --delete-pct 0 \
            --read-targets "$READ_TARGETS" \
            --read-path "$READ_PATH" \
            --timeout-s "$LOADGEN_TIMEOUT_S"
        else
          target/release/replication-loadgen \
            --leader "$LEADER_URL" \
            --total "$PER_NODE_OPS" \
            --concurrency "$LOADGEN_CONCURRENCY" \
            --bulk-size "$LOADGEN_BULK_SIZE" \
            --mode "$MODE" \
            --create-pct 100 \
            --read-pct 0 \
            --update-pct 0 \
            --delete-pct 0 \
            --read-targets "$READ_TARGETS" \
            --read-path "$READ_PATH" \
            --timeout-s "$LOADGEN_TIMEOUT_S" & LG1=$!
          target/release/replication-loadgen \
            --leader "$LEADER_URL" \
            --total "$PER_NODE_OPS" \
            --concurrency "$LOADGEN_CONCURRENCY" \
            --bulk-size "$LOADGEN_BULK_SIZE" \
            --mode "$MODE" \
            --create-pct 100 \
            --read-pct 0 \
            --update-pct 0 \
            --delete-pct 0 \
            --read-targets "$READ_TARGETS" \
            --read-path "$READ_PATH" \
            --timeout-s "$LOADGEN_TIMEOUT_S" & LG2=$!
          target/release/replication-loadgen \
            --leader "$LEADER_URL" \
            --total "$PER_NODE_OPS" \
            --concurrency "$LOADGEN_CONCURRENCY" \
            --bulk-size "$LOADGEN_BULK_SIZE" \
            --mode "$MODE" \
            --create-pct 100 \
            --read-pct 0 \
            --update-pct 0 \
            --delete-pct 0 \
            --read-targets "$READ_TARGETS" \
            --read-path "$READ_PATH" \
            --timeout-s "$LOADGEN_TIMEOUT_S" & LG3=$!
          wait $LG1; wait $LG2; wait $LG3;
        fi
      done
      ;;
    *)
      echo "❓ Unknown BENCH_SUITE='$BENCH_SUITE'. Running default single round."
      # fall back to single round by unsetting BENCH_SUITE and re-running this section
      ;;
  esac
  TOTAL_DONE=$TOTAL_OPS
  echo "✅ Suite completed. Exiting parent process to avoid duplicate validation."
  exit 0
fi

# Compute CRUD duration from original START_TIME and reuse for metrics
CRUD_DURATION=$(( $(date +%s) - START_TIME ))
echo ""

# 5. Wait for replication to stabilize and converge
if [ "$SINGLE_NODE" != "1" ]; then
  echo "⏳ Waiting for replication to stabilize (${REPLICATION_WAIT}s initial wait)..."
  sleep $REPLICATION_WAIT
  echo "🔁 Polling for convergence (up to 30s)..."
  MAX_POLL=15
  for i in $(seq 1 $MAX_POLL); do
    NODE1_PRODUCTS=$(curl -s http://127.0.0.1:$LEADER_PORT/api/products 2>/dev/null || echo "[]")
    NODE2_PRODUCTS=$(curl -s http://127.0.0.1:$FOLLOWER1_PORT/api/products 2>/dev/null || echo "[]")
    NODE3_PRODUCTS=$(curl -s http://127.0.0.1:$FOLLOWER2_PORT/api/products 2>/dev/null || echo "[]")
    NODE1_COUNT=$(echo "$NODE1_PRODUCTS" | jq 'length' 2>/dev/null || echo "0")
    NODE2_COUNT=$(echo "$NODE2_PRODUCTS" | jq 'length' 2>/dev/null || echo "0")
    NODE3_COUNT=$(echo "$NODE3_PRODUCTS" | jq 'length' 2>/dev/null || echo "0")
    echo "  Poll $i: leader=$NODE1_COUNT, f1=$NODE2_COUNT, f2=$NODE3_COUNT"
    if [ "$NODE1_COUNT" = "$NODE2_COUNT" ] && [ "$NODE1_COUNT" = "$NODE3_COUNT" ]; then
      echo "✅ Counts converged"
      break
    fi
    sleep 2
  done
  echo ""
fi

if [ "$SINGLE_NODE" != "1" ]; then
  echo "🔍 Step 5: Comparing product data across all 3 nodes..."

  # Get product counts and data from each node (fresh read)
  echo "Fetching data from all nodes..."
  NODE1_PRODUCTS=$(curl -s http://127.0.0.1:$LEADER_PORT/api/products 2>/dev/null || echo "[]")
  NODE2_PRODUCTS=$(curl -s http://127.0.0.1:$FOLLOWER1_PORT/api/products 2>/dev/null || echo "[]")
  NODE3_PRODUCTS=$(curl -s http://127.0.0.1:$FOLLOWER2_PORT/api/products 2>/dev/null || echo "[]")

  # Count products on each node
  NODE1_COUNT=$(echo "$NODE1_PRODUCTS" | jq 'length' 2>/dev/null || echo "0")
  NODE2_COUNT=$(echo "$NODE2_PRODUCTS" | jq 'length' 2>/dev/null || echo "0")
  NODE3_COUNT=$(echo "$NODE3_PRODUCTS" | jq 'length' 2>/dev/null || echo "0")

  echo "Product counts:"
  echo "  Node 1 (Leader): $NODE1_COUNT products"
  echo "  Node 2 (Follower): $NODE2_COUNT products"
  echo "  Node 3 (Follower): $NODE3_COUNT products"
  echo ""

  PRINT_FULL_LISTS=true
  if [ $TOTAL_OPS -gt 2000 ]; then
    PRINT_FULL_LISTS=false
  fi

  # Display complete product lists from all 3 nodes
  if [ "$PRINT_FULL_LISTS" = true ]; then
    echo "\n📋 COMPLETE PRODUCT LISTS FROM ALL 3 NODES:"
    echo "=============================================="

    echo "\n🔵 NODE 1 (Leader) - Complete Product List:"
    echo "$NODE1_PRODUCTS" | jq '.'

    echo "\n🟢 NODE 2 (Follower) - Complete Product List:"
    echo "$NODE2_PRODUCTS" | jq '.'

    echo "\n🟡 NODE 3 (Follower) - Complete Product List:"
    echo "$NODE3_PRODUCTS" | jq '.'

    echo "\n=============================================="
  fi
  echo ""

  # Create sorted JSON files for comparison
  echo "$NODE1_PRODUCTS" | jq 'sort_by(.id)' 2>/dev/null > /tmp/node1_products.json || echo "$NODE1_PRODUCTS" > /tmp/node1_products.json
  echo "$NODE2_PRODUCTS" | jq 'sort_by(.id)' 2>/dev/null > /tmp/node2_products.json || echo "$NODE2_PRODUCTS" > /tmp/node2_products.json
  echo "$NODE3_PRODUCTS" | jq 'sort_by(.id)' 2>/dev/null > /tmp/node3_products.json || echo "$NODE3_PRODUCTS" > /tmp/node3_products.json

  # Check consistency by comparing complete JSON data
  echo "🔍 Comparing COMPLETE product lists (not just counts)..."

  # Compare Node 1 vs Node 2 (allow diff to return 1 without aborting)
  DIFF_1_2=$(diff /tmp/node1_products.json /tmp/node2_products.json 2>/dev/null || true)
  # Compare Node 2 vs Node 3
  DIFF_2_3=$(diff /tmp/node2_products.json /tmp/node3_products.json 2>/dev/null || true)
  # Compare Node 1 vs Node 3
  DIFF_1_3=$(diff /tmp/node1_products.json /tmp/node3_products.json 2>/dev/null || true)

  if [ -z "$DIFF_1_2" ] && [ -z "$DIFF_2_3" ] && [ -z "$DIFF_1_3" ]; then
      echo "✅ SUCCESS: COMPLETE product lists are IDENTICAL across all 3 nodes!"
      echo "✅ REPLICATION IS WORKING PERFECTLY!"
      echo "   - Node 1: $NODE1_COUNT products"
      echo "   - Node 2: $NODE2_COUNT products"
      echo "   - Node 3: $NODE3_COUNT products"
      echo "   - All nodes have the exact same data"
  else
      echo "❌ FAILURE: Product lists DIFFER between nodes!"
      echo ""
      if [ ! -z "$DIFF_1_2" ]; then
          echo "❌ DIFFERENCE between Node 1 and Node 2:"
          echo "$DIFF_1_2" | head -20
          echo ""
      fi
      if [ ! -z "$DIFF_2_3" ]; then
          echo "❌ DIFFERENCE between Node 2 and Node 3:"
          echo "$DIFF_2_3" | head -20
          echo ""
      fi
      if [ ! -z "$DIFF_1_3" ]; then
          echo "❌ DIFFERENCE between Node 1 and Node 3:"
          echo "$DIFF_1_3" | head -20
          echo ""
      fi
      echo "🔍 SUMMARY:"
      echo "   - Node 1: $NODE1_COUNT products"
      echo "   - Node 2: $NODE2_COUNT products"
      echo "   - Node 3: $NODE3_COUNT products"
  fi

  # Clean up temp files
  rm -f /tmp/node1_products.json /tmp/node2_products.json /tmp/node3_products.json
  echo ""
else
  echo "🔍 Step 5: Fetching product data on single node..."
  NODE1_PRODUCTS=$(curl -s http://127.0.0.1:$LEADER_PORT/api/products 2>/dev/null || echo "[]")
  NODE1_COUNT=$(echo "$NODE1_PRODUCTS" | jq 'length' 2>/dev/null || echo "0")
  echo "Product count (single node): $NODE1_COUNT"
  echo ""
fi

# Performance metrics
OPS_PER_SECOND=$(echo "scale=2; ${TOTAL_DONE:-$TOTAL_OPS} / ${CRUD_DURATION:-1}" | bc -l 2>/dev/null || echo "N/A")
echo "📊 Performance Metrics:"
echo "  Total Operations: ${TOTAL_DONE:-$TOTAL_OPS}"
echo "  Duration: ${CRUD_DURATION:-N/A} seconds"
echo "  Operations per second: $OPS_PER_SECOND"
echo "  Final product count: $NODE1_COUNT"
echo ""

# Check persistence
echo "🔍 Checking EventStore persistence (example dir + legacy root) ..."
RAFTLOG_FILES_EXAMPLE=$(find "$EXPERIMENT_DATA_BASE"/ -name "*.raftlog" 2>/dev/null | wc -l)
RAFTLOG_FILES_ROOT=$(find data/ -name "*.raftlog" 2>/dev/null | wc -l)
RAFTLOG_FILES=$((RAFTLOG_FILES_EXAMPLE + RAFTLOG_FILES_ROOT))
echo "Found $RAFTLOG_FILES raftlog files (example=$RAFTLOG_FILES_EXAMPLE, root=$RAFTLOG_FILES_ROOT)"

if [ "$RAFTLOG_FILES" -ge 3 ]; then
    echo "✅ SUCCESS: All nodes have persisted data to raftlog files"
    # Show file sizes from both locations
    if [ "$RAFTLOG_FILES_EXAMPLE" -gt 0 ]; then
      find "$EXPERIMENT_DATA_BASE"/ -name "*.raftlog" -exec ls -lh {} \; | while read -r line; do
          echo "  $line"
      done
    fi
    if [ "$RAFTLOG_FILES_ROOT" -gt 0 ]; then
      find data/ -name "*.raftlog" -exec ls -lh {} \; | while read -r line; do
          echo "  $line"
      done
    fi
else
    echo "❌ WARNING: Not all nodes have raftlog files"
fi
echo ""

# Cleanup
echo "🧹 Cleaning up processes..."
if [ "$SINGLE_NODE" = "1" ]; then
  kill $NODE1_PID 2>/dev/null || true
else
  kill $NODE1_PID $NODE2_PID $NODE3_PID 2>/dev/null || true
fi
sleep 2

echo "🏁 Benchmark completed!"
echo ""
echo "Summary:"
if [ "$SINGLE_NODE" = "1" ]; then
  echo "  🧪 Mode: Single-node"
  echo "  ✅ Data persistence: $([ "$RAFTLOG_FILES" -ge 1 ] && echo "WORKING" || echo "PARTIAL")"
  echo "  📊 Performance: $OPS_PER_SECOND ops/sec (single node)"
  echo "  🎯 Final state: $NODE1_COUNT products on node"
else
  echo "  🧪 Mode: 3-node cluster"
  echo "  ✅ Distributed replication: $([ "$NODE1_COUNT" = "$NODE2_COUNT" ] && [ "$NODE2_COUNT" = "$NODE3_COUNT" ] && echo "WORKING" || echo "FAILED")"
  echo "  ✅ Data persistence: $([ "$RAFTLOG_FILES" -ge 3 ] && echo "WORKING" || echo "PARTIAL")"
  echo "  📊 Performance: $OPS_PER_SECOND ops/sec across 3 nodes"
  echo "  🎯 Final state: $NODE1_COUNT products replicated consistently"
fi