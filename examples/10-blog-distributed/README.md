# 10 - Blog Distributed

The blog from example 04, but replicated across a Raft cluster.
Write to the leader, read from any node.

## Run a 3-node cluster

```bash
# Terminal 1 - Leader
cargo run -p blog-distributed --bin blog_replicated_node -- \
  --node-id 0 --port 8080 --peers 8081,8082

# Terminal 2 - Follower
cargo run -p blog-distributed --bin blog_replicated_node -- \
  --node-id 1 --port 8081 --peers 8080,8082

# Terminal 3 - Follower
cargo run -p blog-distributed --bin blog_replicated_node -- \
  --node-id 2 --port 8082 --peers 8080,8081
```

## Test replication

```bash
# Login on leader
curl -X POST http://localhost:8080/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"admin","password":"password123"}'

# Create article on leader
curl -X POST http://localhost:8080/api/articles \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <token>" \
  -d '{"title":"Hello","content":"Replicated!","author_id":"admin"}'

# Read from any follower
curl http://localhost:8081/api/articles
curl http://localhost:8082/api/articles

# Cluster health
curl http://localhost:8080/_raft/health | jq
```

## What you learn

- Combining DeclarativeModel + RBAC + Raft in one binary
- `.with_raft_cluster()` configuration
- Automatic write-to-leader redirection
- Data consistency across nodes
- Cluster health monitoring

## Prerequisites

Understand [04-blog](../04-blog/) and [09-replication](../09-replication/) first.
