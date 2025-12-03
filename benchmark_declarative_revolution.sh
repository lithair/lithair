#!/bin/bash

# Lithair DeclarativeModel COMPLETE Benchmark
# Démontre la révolution : 1 struct → Backend complet !

echo "🔥 Lithair DeclarativeModel REVOLUTION Benchmark"
echo "=================================================="
echo ""

# Create data directory for benchmark
mkdir -p data

# Compile benchmark 
echo "🛠️  Compiling DeclarativeModel benchmark..."
cd examples/raft_replication_demo

# Add benchmark binary to Cargo.toml
if ! grep -q "declarative_benchmark" Cargo.toml; then
    echo "" >> Cargo.toml
    echo "[[bin]]" >> Cargo.toml
    echo "name = \"declarative_benchmark\"" >> Cargo.toml
    echo "path = \"declarative_benchmark.rs\"" >> Cargo.toml
fi

echo "⚡ Building benchmark with release optimizations..."
cargo build --release --bin declarative_benchmark

if [ $? -eq 0 ]; then
    echo "✅ Build successful!"
    echo ""
    echo "🚀 Launching DeclarativeModel REVOLUTION benchmark..."
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    
    # Run the benchmark
    cargo run --release --bin declarative_benchmark
    
    echo ""
    echo "📊 Benchmark completed!"
    echo ""
    echo "📁 Check generated data:"
    echo "   - Event storage: data/benchmark_users.events"
    echo "   - Performance logs above"
    echo ""
    echo "🔥 REVOLUTION DEMONSTRATED:"
    echo "   🎯 1 struct annotation → Complete backend system"
    echo "   ⚡ SCC2 Engine ultra-performance"
    echo "   📁 EventStore real persistence" 
    echo "   🌐 Auto-generated REST API"
    echo "   🔐 Auto-generated RBAC security"
    echo "   📝 Auto-generated audit trail"
    echo ""
    echo "🚀 Lithair: The future of backend development is HERE!"
    
else
    echo "❌ Build failed. Check compilation errors above."
    exit 1
fi