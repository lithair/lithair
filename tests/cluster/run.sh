#!/bin/sh
# Runs inside the cidx Docker client container. No host Cargo/Probatum required.
set -eu
umask 077
nonce=$(od -An -N8 -tx1 /dev/urandom | tr -d ' \n')
export COMPOSE_PROJECT_NAME="lithair-cluster-$nonce"
export LITHAIR_CLUSTER_NODE_IMAGE="lithair-cluster-node:$nonce"
export LITHAIR_CLUSTER_DRIVER_IMAGE="lithair-cluster-driver:$nonce"
# Valid placeholders let cleanup parse Compose even if a build fails before
# subnet allocation. No node is started until the real addresses are assigned.
export LITHAIR_CLUSTER_NODE1_IP=0.0.0.0 LITHAIR_CLUSTER_NODE2_IP=0.0.0.0 LITHAIR_CLUSTER_NODE3_IP=0.0.0.0
report="/work/.probatum/runs/$COMPOSE_PROJECT_NAME"
staging=$(mktemp -d)
mkdir -p "$report" "$staging/node" "$staging/driver"
# A clean CI checkout has no evidence parents yet. They must be traversable by
# the workspace owner when upload-artifact runs outside this root container.
owner="$(stat -c %u /work):$(stat -c %g /work)"
chown "$owner" /work/.probatum /work/.probatum/runs "$report"
record_environment() {
printf '%s\n' "export COMPOSE_PROJECT_NAME=$COMPOSE_PROJECT_NAME" \
    "export LITHAIR_CLUSTER_NODE_IMAGE=$LITHAIR_CLUSTER_NODE_IMAGE" \
    "export LITHAIR_CLUSTER_DRIVER_IMAGE=$LITHAIR_CLUSTER_DRIVER_IMAGE" \
    "export LITHAIR_CLUSTER_NODE1_IP=$LITHAIR_CLUSTER_NODE1_IP" \
    "export LITHAIR_CLUSTER_NODE2_IP=$LITHAIR_CLUSTER_NODE2_IP" \
    "export LITHAIR_CLUSTER_NODE3_IP=$LITHAIR_CLUSTER_NODE3_IP" > "$report/project.env"
}
record_environment
compose() { docker compose -f /work/tests/cluster/compose.yml "$@"; }
cleanup() {
    result=$?
    trap - EXIT INT TERM
    if [ -f "$report/verdict.json" ]; then cat "$report/verdict.json"; fi
    compose logs --no-color > "$report/containers.log" 2>&1 || true
    docker cp "$COMPOSE_PROJECT_NAME-driver:/evidence/." "$report/evidence" 2>/dev/null || true
    # A cleanup failure is itself a failed gate; never silently leave this run behind.
    compose down --volumes --remove-orphans --timeout 5 >> "$report/cleanup.log" 2>&1 || result=1
    for network in "$COMPOSE_PROJECT_NAME-subnet-probe" "$COMPOSE_PROJECT_NAME-replication"; do
        if docker network inspect "$network" >/dev/null 2>&1; then
            docker network rm "$network" >> "$report/cleanup.log" 2>&1 || result=1
        fi
    done
    remaining=$(docker container ls -aq --filter "label=com.docker.compose.project=$COMPOSE_PROJECT_NAME") || result=1
    if [ -n "$remaining" ]; then
        echo "Unremoved containers: $remaining" >> "$report/cleanup.log"
        result=1
    fi
    for kind in network volume; do
        remaining=$(docker "$kind" ls -q --filter "label=com.docker.compose.project=$COMPOSE_PROJECT_NAME") || result=1
        if [ -n "$remaining" ]; then
            echo "Unremoved $kind resources: $remaining" >> "$report/cleanup.log"
            result=1
        fi
    done
    docker image rm "$LITHAIR_CLUSTER_NODE_IMAGE" "$LITHAIR_CLUSTER_DRIVER_IMAGE" >> "$report/cleanup.log" 2>&1 || true
    rm -rf "$staging"
    chown -R "$owner" "$report"
    echo "Cluster evidence: .probatum/runs/$COMPOSE_PROJECT_NAME (exit $result)"
    exit "$result"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
cp /work/target/debug/examples/cluster-compose-node "$staging/node/node"
cp /work/tests/cluster/Node.Dockerfile "$staging/node/Dockerfile"
cp /work/tests/cluster/Driver.Dockerfile "$staging/driver/Dockerfile"
cp /work/tests/cluster/checks.py /work/tests/cluster/compose.yml /work/tests/cluster/probatum.toml "$staging/driver/"
docker build -t "$LITHAIR_CLUSTER_NODE_IMAGE" "$staging/node"
docker build -t "$LITHAIR_CLUSTER_DRIVER_IMAGE" "$staging/driver"
# Ask Docker's allocator for a free pool, then reserve it explicitly. Concurrent
# allocators may win the gap between removal and reservation; retry on conflict.
# No host subnet is hardcoded and Docker remains the overlap authority.
reserved=false
for attempt in 1 2 3 4 5 6 7 8; do
    probe="$COMPOSE_PROJECT_NAME-subnet-probe"
    docker network create --internal --label "com.docker.compose.project=$COMPOSE_PROJECT_NAME" "$probe" >/dev/null
    subnet=$(docker network inspect -f '{{(index .IPAM.Config 0).Subnet}}' "$probe")
    docker network rm "$probe" >/dev/null
    if docker network create --internal --subnet "$subnet" \
        --label "com.docker.compose.project=$COMPOSE_PROJECT_NAME" \
        "$COMPOSE_PROJECT_NAME-replication" > "$report/replication-network.id"; then
        reserved=true
        break
    fi
done
if [ "$reserved" != true ]; then echo 'Could not reserve an isolated replication subnet' >&2; exit 1; fi
case "$subnet" in *.*.*.*/*) ;; *) echo 'An IPv4 Docker pool is required' >&2; exit 1;; esac
if [ "${subnet#*/}" -gt 29 ]; then echo 'Docker pool is too small for three nodes' >&2; exit 1; fi
base=${subnet%/*}
prefix=${base%.*}
octet=${base##*.}
export LITHAIR_CLUSTER_NODE1_IP="$prefix.$((octet + 2))"
export LITHAIR_CLUSTER_NODE2_IP="$prefix.$((octet + 3))"
export LITHAIR_CLUSTER_NODE3_IP="$prefix.$((octet + 4))"
record_environment
docker network inspect "$COMPOSE_PROJECT_NAME-replication" > "$report/replication-network.json"
compose run --rm --no-deps prepare
compose up -d node1 node2 node3
compose run -T --no-deps --name "$COMPOSE_PROJECT_NAME-driver" driver > "$report/verdict.json"
