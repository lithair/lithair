# 🚀 Lithair Distributed Replication Demo

Demo d'un cluster Lithair multi-nœuds avec réplication automatique des données.

## 🎯 Objectif

Cet exemple montre comment :
- Configurer un cluster Lithair distribué
- Répliquer automatiquement les données entre nœuds
- Utiliser le modèle déclaratif avec attributs de persistance
- Gérer la redirection vers le leader et la réplication HTTP (OpenRaft complet: WIP)

## 🏗️ Architecture

```
Node 1 (Leader)     Node 2 (Follower)    Node 3 (Follower)
┌──────────────┐    ┌────────────────┐    ┌────────────────┐
│ Port: 8080   │◄───┤ Port: 8081     │    │ Port: 8082     │
│ Data: node1  │    │ Data: node2    │◄───┤ Data: node3    │
└──────────────┘    └────────────────┘    └────────────────┘
        ▲                    ▲                       ▲
        └────────────────────┼───────────────────────┘
                         Raft Consensus
```

## 📋 Fonctionnalités

### Modèle Déclaratif avec Réplication
- **Product**: Modèle produit avec clé primaire, champs audités et réplication
- **Attributs de persistance**: `#[persistence(replicate, track_history)]`

### Événements Distribués
- Création/modification d'utilisateurs
- Création/modification de messages
- Statistiques de réplication par nœud

## 🚀 Usage

### Démarrage du Cluster

```bash
# Terminal 1: Lancer le leader (Node 1)
cargo run --release --bin replication-declarative-node -- \
  --node-id 1 \
  --port 8080 \
  --peers "8081,8082"

# Terminal 2: Lancer le follower (Node 2)
cargo run --release --bin replication-declarative-node -- \
  --node-id 2 \
  --port 8081 \
  --peers "8080,8082"

# Terminal 3: Lancer le follower (Node 3)
cargo run --release --bin replication-declarative-node -- \
  --node-id 3 \
  --port 8082 \
  --peers "8080,8081"
```

### Monitorer la Réplication

Chaque nœud affiche ses statistiques toutes les 10 secondes :
```
=== Node 1 Statistics ===
Users: 2 local, 6 total
Messages: 3 local, 9 total  
Replications: 6 received, 12 sent
==============================
```

## 🔧 Configuration

### Attributs Déclaratifs de Persistance (extrait simplifié)

```rust
#[derive(DeclarativeModel)]
pub struct Product {
    #[db(primary_key, indexed)]
    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(replicate, track_history)]
    pub id: Uuid,

    #[db(indexed, unique)]
    #[lifecycle(audited, retention = 90)]
    #[http(expose, validate = "non_empty")]
    #[persistence(replicate, track_history)]
    pub name: String,
}
```

### Options Disponibles
- `replicate`: Réplique sur tous les nœuds du cluster
- `track_history`: Conserve l'historique complet des modifications
- `memory_only`: Données locales uniquement (pas de persistance/réplication)
- `auto_persist`: Persistance automatique des écritures
- `no_replication`: Exclut de la réplication même si persisté

## 📊 Monitoring

### Métriques par Nœud
- **users_created**: Utilisateurs créés localement
- **messages_created**: Messages créés localement  
- **replications_received**: Événements reçus d'autres nœuds
- **replications_sent**: Événements envoyés aux autres nœuds

### Persistence
- Événements persistés dans un EventStore local par nœud (fichiers `.raftlog`)
- Snapshots périodiques pour accélérer la reprise (si activés)

## 🧪 Tests de Réplication

### Scénarios Testés
1. **Création distribuée**: Chaque nœud crée des utilisateurs/messages
2. **Contraintes uniques**: Vérification des doublons cross-nœuds
3. **Clés étrangères**: Cohérence des relations entre entités
4. **Récupération**: Redémarrage de nœuds et rattrapage

### Ordre d'Exécution des Tests
1. Démarrer tous les nœuds
2. Attendre la formation du cluster
3. Exécuter les opérations en parallèle sur chaque nœud
4. Vérifier la cohérence des données répliquées

## 🔮 Prochaines Étapes (TODO)

- [ ] Intégration OpenRaft complète (consensus fort)
- [ ] Gestion des partitions réseau
- [ ] Tests de performance sous charge élevée
- [ ] Interface web de monitoring du cluster

## 🎛️ Arguments de Ligne de Commande

```bash
--node-id <ID>              # ID unique du nœud (obligatoire)
--port <PORT>               # Port d'écoute (défaut: 8080)
--peers "<PORT1>,<PORT2>"   # Autres nœuds: ports des pairs sur localhost
```

## 💡 Notes d'Implémentation

- Serveur HTTP basé sur Hyper (HTTP/1.1)
- Redirection automatique des écritures vers le leader
- Réplication des données via HTTP entre nœuds
- Événements sérialisés en JSON pour le transport réseau

## 🧪 Benchmarks

Un script est fourni pour lancer un benchmark CRUD distribué:

```bash
./bench_1000_crud_parallel.sh 1000
```

Consultez `baseline_results/` à la racine du repo pour des mesures représentatives.

## 🔐 HTTP Hardening Demo (stateless perf + firewall)

Le binaire `replication-hardening-node` lance un serveur HTTP déclaratif minimal pour démontrer :

- Endpoints de performance sans état (`/perf/echo`, `/perf/json`, `/perf/bytes`)
- Gzip (négociation `Accept-Encoding`, seuil configurable)
- Politiques par préfixe (ex : forcer gzip / `no-store` sur `/perf`)
- Firewall (allow/deny IP, CIDR, macros `internal`, `loopback`, etc.)

Par défaut, ce serveur démarre avec une posture « production-like » :

- `/perf/*` et `/metrics` protégés par firewall
- `/status` et `/health` exemptés
- `allow` inclut la macro `internal` (réseaux privés IPv4 + ULA IPv6)

Pour l’ouvrir en local (désactiver la posture firewall par défaut) :

```bash
cargo run -p replication --bin replication-hardening-node -- --port 18320 --open
```

Vous pouvez aussi compiler l’exemple en mode « ouvert par défaut » via la feature :

```bash
cargo run -p replication --features open_by_default --bin replication-hardening-node -- --port 18320
```

Le script de bench stateless lance automatiquement le serveur avec `--open` :

```bash
bash examples/09-replication/bench_http_server_stateless.sh
```

### Mode Single-Node (Isolation du moteur/persistance)

Pour isoler l’overhead réseau/consensus et mesurer uniquement le coût HTTP + moteur + persistance, vous pouvez lancer le benchmark en **mono‑nœud** :

```bash
SINGLE_NODE=1 ./bench_1000_crud_parallel.sh 10000
```

Astuce : combinez avec les variables `LT_` pour comparer JSON vs Binaire, async on/off :

```bash
# Async JSON (Stage A)
SINGLE_NODE=1 LT_OPT_PERSIST=1 LT_ENABLE_BINARY=0 ./bench_1000_crud_parallel.sh 10000

# Binaire (Stage B)
SINGLE_NODE=1 LT_OPT_PERSIST=1 LT_ENABLE_BINARY=1 ./bench_1000_crud_parallel.sh 10000
```

## ⚙️ Runtime (Persistence & Performance)

Pour des benchmarks réalistes à haut débit, le demo supporte des variables d’environnement `LT_` qui pilotent la persistance de l’EventStore :

- `LT_OPT_PERSIST` (1/0) – active l’écriture asynchrone optimisée (writer thread) pour les événements (par défaut activée dans le script de bench).
- `LT_BUFFER_SIZE` (octets) – taille du buffer d’écriture (par défaut 1 048 576 = 1 Mo).
- `LT_MAX_EVENTS_BUFFER` – nombre d’événements à mettre en tampon avant flush (par défaut 2000).
- `LT_FLUSH_INTERVAL_MS` – intervalle de flush périodique (par défaut 5 ms pour les benchs).
- `LT_FSYNC_ON_APPEND` (1/0) – fsync à chaque append (0 recommandé pour les benchs de débit).
- `LT_EVENT_MAX_BATCH` – taille de lot (batch) interne côté EventStore (par défaut 65536 dans le script de bench).
- `LT_ENABLE_BINARY` (1/0) – active le mode binaire (Stage B) : les enveloppes d’événements sont sérialisées en bincode et écrites lignes par lignes (séparées par `\n`). Rejouer/restaurer reste compatible : le moteur reconvertit en JSON lors de la lecture.
- `LT_DISABLE_INDEX` (1/0) – désactive l’index `aggregate_id -> offset` pour éviter des écritures supplémentaires pendant les benchs.
- `LT_DEDUP_PERSIST` (1/0) – contrôle la persistance des IDs d’idempotence. Mettre à `0` pour les benchs éphémères (pas d’exactly‑once cross‑restart nécessaire).

Exemple d’exécution manuelle avec persistance optimisée et binaire :

```bash
export LT_OPT_PERSIST=1
export LT_BUFFER_SIZE=1048576
export LT_MAX_EVENTS_BUFFER=5000
export LT_FLUSH_INTERVAL_MS=2
export LT_FSYNC_ON_APPEND=0
export LT_ENABLE_BINARY=1

./bench_1000_crud_parallel.sh 10000
```

Notes :

- Le script `bench_1000_crud_parallel.sh` exporte déjà des valeurs par défaut adaptées pour le débit, dont `LT_OPT_PERSIST=1`.
- Le mode binaire (`LT_ENABLE_BINARY=1`) maximise la vitesse d’append (3–5× vs JSON selon les charges) tout en conservant des snapshots JSON.

### Profils de stockage prédéfinis (STORAGE_PROFILE)

Le script de bench supporte des profils prêts à l’emploi (sélection via `STORAGE_PROFILE=<nom>`):

- `high_throughput` (par défaut)
  - Objectif : Débit maximum (benchmarks). Async writer ON, binaire ON, index/dedup OFF, gros buffers, fsync OFF, snapshots très espacés.
  - Exemple :
    ```bash
    STORAGE_PROFILE=high_throughput LOADGEN_MODE=bulk LOADGEN_BULK_SIZE=500 \
    ./bench_1000_crud_parallel.sh 10000
    ```

- `balanced`
  - Objectif : Compromis débit/fiabilité. Async ON, binaire ON, index/dedup ON, buffers moyens, fsync OFF.
  - Exemple :
    ```bash
    STORAGE_PROFILE=balanced LOADGEN_MODE=bulk LOADGEN_BULK_SIZE=500 \
    ./bench_1000_crud_parallel.sh 10000
    ```

- `durable_security`
  - Objectif : Durabilité et audit trail. Async ON, binaire OFF (lisibilité), index/dedup ON, fsync ON, snapshots fréquents.
  - Exemple :
    ```bash
    STORAGE_PROFILE=durable_security LOADGEN_MODE=bulk LOADGEN_BULK_SIZE=200 \
    ./bench_1000_crud_parallel.sh 10000
    ```

Chaque profil configure automatiquement les variables `LT_` adéquates (buffers, flush, fsync, index, dedup, snapshots) afin d’adapter le moteur aux besoins de l’application.

### Chemin de données (EXPERIMENT_DATA_BASE)

Par défaut, le script de bench configure la base de données de l’exemple dans:

```
EXPERIMENT_DATA_BASE=examples/09-replication/data
```

Ce chemin est transmis au moteur via la variable d’environnement `EXPERIMENT_DATA_BASE` et remplace `EngineConfig.event_log_path` au démarrage. Vous pouvez donc:

- Laisser le comportement par défaut (les fichiers `.raftlog`/snapshots sont écrits dans le dossier de l’exemple)
- Ou bien surcharger le chemin:

```bash
EXPERIMENT_DATA_BASE=/tmp/lithair_bench \
STORAGE_PROFILE=high_throughput LOADGEN_MODE=bulk LOADGEN_BULK_SIZE=1000 \
./bench_1000_crud_parallel.sh 100000
```

Le script affiche explicitement le chemin utilisé et liste les fichiers persistés en fin de run.

## 🔦 Lectures légères (LIGHT_READS)

Pour éviter le coût de sérialisation JSON de la liste complète (`GET /api/products`), le bench supporte des lectures « légères » configurables via `LIGHT_READS` :

- `LIGHT_READS=0` (défaut) → `GET /api/products` (liste complète, lecture lourde)
- `LIGHT_READS=1`, `true` ou `status` → `GET /status` (très léger)
- `LIGHT_READS=count` → `GET /api/products/count` (léger, retourne `{ "count": N }`)

Endpoints ajoutés par le serveur déclaratif (`lithair-core/src/http/declarative.rs`) :

- `GET /api/{model}/count` → renvoie uniquement le nombre d’éléments
- `GET /api/{model}/random-id` → renvoie un `id` existant (utile pour préremplir les UPDATE sans lister tout)

### A/B test « heavy vs light »

Exemple après pré-seed (5 000 objets par nœud) :

```bash
# Heavy read: liste complète
LIGHT_READS=0 PRESEED_PER_NODE=5000 CREATE_PERCENTAGE=0 READ_PERCENTAGE=100 UPDATE_PERCENTAGE=0 \
  ./bench_1000_crud_parallel.sh 3000

# Light read: compteur
LIGHT_READS=count PRESEED_PER_NODE=5000 CREATE_PERCENTAGE=0 READ_PERCENTAGE=100 UPDATE_PERCENTAGE=0 \
  ./bench_1000_crud_parallel.sh 3000
```

Dans nos mesures récentes :

Observations récentes (3 nœuds, PRESEED_PER_NODE=50000, concurrency=256, lecture seule 3000 ops) :

- Heavy read (liste complète) ≈ 38.6 ops/s, p50 ≈ 6.1 s, p95 ≈ 10 s
- Light read (count) ≈ 10.3k–15.3k ops/s, p50 ≈ 2–3 ms, p95 ≈ 115–128 ms
- Status ≈ 15.1k–24.6k ops/s, p50 ≈ 1–2 ms, p95 ≈ 80–170 ms

Recommandations :
- Évitez `GET /api/products` pour les benchmarks de perf; utilisez `/count` ou `/status`.
- Profil `high_throughput` : par défaut `LOADGEN_CONCURRENCY=256` offre le meilleur compromis débit/tails.
- Profils `balanced` et `durable_security` : rester ≤512 pour contenir les tails d’écriture.
- La suite `BENCH_SUITE=durability_profiles` redémarre le cluster à chaque profil afin d’appliquer correctement les paramètres de stockage.

Astuce : pour des workloads à forte proportion d’UPDATE, le loadgen utilise désormais `GET /api/products/random-id` pour récupérer un `id` léger si la pool d’ID est vide (pas de `GET /api/products`).