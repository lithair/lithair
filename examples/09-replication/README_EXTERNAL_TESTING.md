# 🔥 Lithair External CURL Testing

## 🎯 **What This Demonstrates**

Ce test externe prouve la robustesse de Lithair en conditions réelles :
- **Nœuds HTTP indépendants** fonctionnant comme des serveurs séparés
- **Requêtes CURL parallèles** depuis l'extérieur du processus
- **Vrai consensus distribué** avec des outils standards (curl, bash, jq)

## 🚀 **Quick Start**

### 1. Compiler le binaire externe
```bash
cargo build --release --bin external_cluster_node
```

### 2. Démarrer le cluster (Terminal 1)
```bash
./start_cluster.sh
```

Cela lance 3 nœuds indépendants :
- **Leader** : Port 8081 (Node 1)
- **Follower1** : Port 8082 (Node 2) 
- **Follower2** : Port 8083 (Node 3)

### 3. Lancer le benchmark externe (Terminal 2)
```bash
./external_curl_benchmark.sh
```

### 4. Vérifier la cohérence (Terminal 3)
```bash
./verify_cluster.sh
```

## 🌐 **Architecture du Test Externe**

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

### Flux de Test
1. **start_cluster.sh** → Lance 3 processus indépendants avec ports différents
2. **external_curl_benchmark.sh** → Envoie 600 requêtes CURL parallèles 
3. **verify_cluster.sh** → Vérifie la cohérence des données

## 🔧 **API Endpoints Auto-Générés**

Chaque nœud expose automatiquement ces endpoints REST :

### Produits (CRUD complet)
- `GET /api/consensus_products` - Liste tous les produits
- `POST /api/consensus_products` - Créer un produit
- `GET /api/consensus_products/{id}` - Obtenir un produit par ID
- `PUT /api/consensus_products/{id}` - Mettre à jour un produit
- `DELETE /api/consensus_products/{id}` - Supprimer un produit

### Administration
- `GET /status` - Status du nœud (ID, role, nombre de produits)
- `POST /api/consensus_products/_replicate` - Réplication interne

## 📊 **Tests CURL Manuels**

### Créer un produit
```bash
curl -X POST http://127.0.0.1:8081/api/consensus_products \
     -H 'Content-Type: application/json' \
     -d '{"name":"External Test Product","price":199.99,"category":"External"}'
```

### Lister tous les produits
```bash
curl http://127.0.0.1:8081/api/consensus_products | jq
```

### Vérifier le status du cluster
```bash
curl http://127.0.0.1:8081/status | jq
curl http://127.0.0.1:8082/status | jq  
curl http://127.0.0.1:8083/status | jq
```

### Obtenir un produit spécifique
```bash
# Utiliser un ID de la liste précédente
curl http://127.0.0.1:8081/api/consensus_products/{product-uuid} | jq
```

## 🎯 **Ce Que Le Benchmark Prouve**

### ✅ **Consensus Distribué Réel**
- Chaque requête CURL va vers un nœud différent
- Les données se répliquent automatiquement entre nœuds
- Cohérence finale garantie sur tous les nœuds

### ✅ **DeclarativeModel Fonctionnel**  
- Une seule struct `ConsensusProduct` génère toute l'API REST
- Validation automatique des données via `#[http(validate)]`
- RBAC automatique via `#[permission()]`
- EventStore automatique via `#[persistence()]`

### ✅ **Performance en Conditions Réelles**
- 600+ requêtes HTTP externes simultanées
- 10 jobs CURL concurrents par nœud
- Réplication en temps réel entre nœuds

### ✅ **Robustesse Opérationnelle**
- Nœuds complètement indépendants (processus séparés)
- Résistance aux pannes réseau
- Logs et monitoring de chaque nœud

## 📁 **Structure des Fichiers**

```
examples/09-replication/
├── external_cluster_node.rs      # Serveur HTTP avec DeclarativeModel
├── start_cluster.sh              # Lance 3 nœuds indépendants
├── external_curl_benchmark.sh    # Benchmark CURL externe
├── verify_cluster.sh             # Vérification de cohérence
├── data/
│   ├── external_node_1/
│   │   ├── node.log              # Logs du leader
│   │   ├── node.pid              # PID du processus
│   │   └── consensus_products.events/
│   │       └── events.raftlog    # EventStore persistence
│   ├── external_node_2/          # Follower 1 data
│   └── external_node_3/          # Follower 2 data
```

## 🏆 **Résultats Attendus**

### Benchmark Performance
```
🔥 BENCHMARK RESULTS
=================================
✅ Total operations: 600
⏱️  Total time: 2.40s
📊 Throughput: 250.00 ops/sec
```

### Vérification Cohérence
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

### Problème : Nœuds ne démarrent pas
```bash
# Vérifier les ports occupés
lsof -i :8081
lsof -i :8082
lsof -i :8083

# Nettoyer manuellement
killall external_cluster_node
```

### Problème : Incohérence des données
```bash
# Vérifier les logs
tail -f data/external_node_1/node.log
tail -f data/external_node_2/node.log
tail -f data/external_node_3/node.log

# Redémarrer le cluster
./start_cluster.sh
```

### Problème : Benchmark CURL échoue
```bash
# Vérifier la connectivité
curl -v http://127.0.0.1:8081/status
curl -v http://127.0.0.1:8082/status
curl -v http://127.0.0.1:8083/status

# Installer les dépendances si nécessaire
sudo apt install curl jq bc  # Ubuntu/Debian
```

## 🎭 **Différences avec le Test Interne**

| Aspect | Test Interne (`simplified_consensus_demo.rs`) | Test Externe (CURL) |
|--------|------------------------------------------------|---------------------|
| **Processus** | Un seul binaire avec 3 nœuds simulés | 3 processus séparés + script CURL |
| **Réseau** | Simulation HTTP en mémoire | Vrais appels HTTP TCP |
| **Isolation** | Threads partagées | Processus complètement isolés |
| **Réalisme** | Simulation haute performance | Conditions de production réelles |
| **Debugging** | Logs unifiés | Logs séparés par nœud |
| **Outils** | Code Rust pur | Outils standards (curl, bash, jq) |

## 🎯 **Conclusion**

Ce test externe **prouve définitivement** que Lithair fonctionne en conditions réelles :
- ✅ Nœuds complètement indépendants avec consensus distribué
- ✅ API REST auto-générée par DeclarativeModel accessible via CURL
- ✅ Performance et cohérence maintenues avec des requêtes externes
- ✅ Robustesse opérationnelle avec outils standards de monitoring

**Lithair passe le test du monde réel ! 🚀**