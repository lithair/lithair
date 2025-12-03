# Distribution & Clustering - Stack Technique

## Technologies utilisées

- **Consensus**: Raft protocol (implémentation maison)
- **State management**: SCC2 (State Concurrent Cache v2)
- **Network**: TCP avec heartbeats personnalisés
- **Persistence**: Event sourcing avec WAL (Write-Ahead Log)
- **Serialization**: Bincode pour les messages Raft

## Points de monitoring critiques

- `raft.current_term`: Terme Raft actuel
- `raft.state`: Follower/Candidate/Leader
- `scc2.partitions`: État des partitions réseau
- `election.timeout_ms`: Timeout d'élection configuré
- `log.committed_index`: Index des entrées commitées

## Commandes de debug

```bash
# Voir l'état du cluster
curl http://localhost:8080/admin/cluster/status

# Forcer une élection
curl -X POST http://localhost:8080/admin/cluster/election

# Simuler une partition réseau
curl -X POST http://localhost:8080/admin/debug/partition/1
```

## Logs patterns à surveiller

- `Election timeout`: Échec d'élection
- `Lost leadership`: Perte de leadership
- `Partition detected`: Détection de partition
- `Log replication failed`: Échec de réplication
