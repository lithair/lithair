# Deploying Lithair with systemd or Kubernetes

Two non-Docker deployment paths ship under
[`examples/deployment/`](../../examples/deployment):

- [`systemd/lithair.service`](../../examples/deployment/systemd/lithair.service)
  — run the binary directly on a Linux host.
- [`k8s/`](../../examples/deployment/k8s) — plain Kubernetes manifests
  (Deployment, Service, PVC, ServiceMonitor). No Helm, no Kustomize.

## Which path?

| Path                 | Use when                                                       |
|----------------------|----------------------------------------------------------------|
| **Docker Compose**   | Dev or single-host deploy with a container runtime. See [deployment-docker.md](deployment-docker.md). |
| **systemd**          | Bare-metal or a VM with no container runtime.                  |
| **Kubernetes**       | An orchestrated cluster you already operate.                   |

All three run the same single-node server. Lithair's cluster mode is **not yet
production-ready** (issue #104), so every path here deploys exactly one
instance backed by one event store.

## systemd

The unit runs an installed binary as a dedicated non-root user, logs to the
journal, and hardens the filesystem.

### 1. Build and install the binary

```bash
cargo build --release -p hello-world
sudo install -m 0755 target/release/hello-world /usr/local/bin/lithair
```

Swap `hello-world` for any workspace example crate (e.g. `blog`, `rest-api`).
Only `hello-world` currently reads `HOST`/`PORT` from the env — see
[deployment-docker.md](deployment-docker.md#other-examples).

### 2. Create the service user and data directory

```bash
sudo useradd --system --home-dir /var/lib/lithair \
  --shell /usr/sbin/nologin lithair
sudo mkdir -p /var/lib/lithair
sudo chown lithair:lithair /var/lib/lithair
```

The event store lives in `/var/lib/lithair`. Losing it means losing the
database — back it up like any database storage.

### 3. Install and start the unit

```bash
sudo cp examples/deployment/systemd/lithair.service \
  /etc/systemd/system/lithair.service
sudo systemctl daemon-reload
sudo systemctl enable --now lithair
```

### 4. Verify

```bash
systemctl status lithair
curl -fsS http://localhost:8080/health    # → {"status":"healthy"}
curl -fsS http://localhost:8080/ready     # → {"status":"ready", ...}
journalctl -u lithair -f                   # tail logs
```

### Configuration

The unit sets `HOST=0.0.0.0` and `PORT=8080`. Add per-model retention overrides
as extra `Environment=` lines (form `LT_<MODEL>_MEMORY_RETENTION`, where
`<MODEL>` is the uppercased final segment of the model type name):

```ini
Environment="LT_EMAIL_MEMORY_RETENTION=2000"
Environment="LT_EMAIL_MEMORY_DURATION=30d"
```

See [retention.md](../features/retention.md) for the full set. Run
`sudo systemctl daemon-reload && sudo systemctl restart lithair` after editing.

### Hardening notes

The unit applies `NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome`,
and `PrivateTmp`. Because `ProtectSystem=strict` makes the whole filesystem
read-only, `ReadWritePaths=/var/lib/lithair` re-opens the event store directory
for writes. If you move the data directory, update both `WorkingDirectory` and
`ReadWritePaths`.

## Kubernetes

```bash
kubectl apply -f examples/deployment/k8s/
```

This creates a Deployment (1 replica), a ClusterIP Service, a 5Gi PVC, and a
ServiceMonitor.

### Single replica

The Deployment pins `replicas: 1` and uses the `Recreate` strategy. Each pod
runs an independent event store; a second replica would silently diverge.
Until cluster mode is production-ready (issue #104), scale vertically by
raising the container's CPU/memory, not the replica count.

### Point at your own image

There is no official published image yet. The Deployment references the
placeholder `ghcr.io/lithair/lithair:0.12.0`. Build and push from the repo
[`Dockerfile`](../../Dockerfile):

```bash
docker build -t ghcr.io/youorg/lithair:0.12.0 .
docker push ghcr.io/youorg/lithair:0.12.0
```

Then edit `image:` in `examples/deployment/k8s/deployment.yaml`.

### Test it

```bash
kubectl rollout status deploy/lithair
kubectl port-forward svc/lithair 8080:8080 &
curl -fsS http://localhost:8080/health     # → {"status":"healthy"}
```

The Service is `ClusterIP` (in-cluster only). Expose it externally with your
own Ingress or LoadBalancer.

### Metrics

`servicemonitor.yaml` requires the **Prometheus Operator** CRDs
(`monitoring.coreos.com/v1`). Without them, applying it fails with
`no matches for kind "ServiceMonitor"` — skip the file and instead add the
commented pod scrape annotations shown in
`examples/deployment/k8s/servicemonitor.yaml` so a plain Prometheus discovers
`/metrics`. Adjust the `release:` label to match your operator's
`serviceMonitorSelector`.

### Storage sizing

The PVC requests 5Gi (`ReadWriteOnce`). The event log is append-only, so size
for lifetime write volume, not the live working set. See the capacity model in
`docs/operations/capacity-planning.md` (TODO per issue #106).
