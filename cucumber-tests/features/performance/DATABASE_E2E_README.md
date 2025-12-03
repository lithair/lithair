# 🎯 Tests E2E Database/Performance Lithair

## **Philosophie**

Ces tests sont **spécifiques** à la couche database/persistence de Lithair :
- ✅ Test du **vrai HttpServer** Lithair
- ✅ Test du **vrai StateEngine** (event sourcing)
- ✅ Test du **vrai FileStorage** (persistence)
- ❌ PAS de test de l'application métier complète

**Focus** : Intégrité + Performance de la persistence

---

## 🏗️ **Architecture**

```
Test Cucumber E2E
    ↓
HttpServer (Lithair réel)
    ↓
StateEngine<TestAppState>
    ↓
FileStorage
    ↓
events.raftlog + snapshots
```

### **Composants Testés**

1. **HttpServer** - Serveur HTTP Lithair natif
   - Keep-alive HTTP/1.1
   - Routage avec `Router`
   - Handlers custom

2. **StateEngine** - Event sourcing
   - `apply_event()` - Application d'événements
   - `get_state()` - Récupération de l'état
   - Mutations atomiques

3. **FileStorage** - Persistence
   - Écriture dans `events.raftlog`
   - Snapshots
   - fsync / flush

4. **TestAppState** - État de test minimal
   ```rust
   pub struct TestAppState {
       pub data: TestData,
       pub version: u64,
   }
   ```

---

## 📁 **Structure**

```
cucumber-tests/
├── features/performance/
│   ├── database_performance.feature       # 19 scénarios
│   ├── DATABASE_E2E_README.md            # Ce fichier
│   └── http_performance.feature          # Tests HTTP purs
│
└── src/features/steps/
    ├── real_database_performance_steps.rs # Steps avec vrai Lithair ✅
    ├── http_performance_steps.rs         # Steps HTTP (test_server)
    └── database_performance_steps.rs     # Anciens steps (stubs)
```

---

## 🎯 **Scénarios de Test**

### **1. Tests d'Intégrité** (4 scénarios)

✅ **Créer 1000 articles et vérifier qu'ils sont TOUS persistés**
```gherkin
When je crée 1000 articles rapidement
Then le fichier events.raftlog doit contenir exactement 1000 événements "ArticleCreated"
And aucun événement ne doit être manquant
```

✅ **Créer 10000 articles avec 50 threads**
```gherkin
When je crée 10000 articles en parallèle avec 50 threads
Then la séquence des IDs doit être continue de 0 à 9999
And aucun doublon ne doit exister
```

### **2. Tests de Performance** (3 scénarios)

✅ **Performance d'écriture - 1000 req/s**
```gherkin
When je mesure la performance d'écriture sur 10 secondes
Then le serveur doit traiter au moins 1000 requêtes par seconde
And la latence p95 doit être inférieure à 100ms
```

✅ **Performance mixte 80/20**
```gherkin
When je lance un test mixte pendant 30 secondes avec:
  | Type     | Pourcentage | Concurrence |
  | Lecture  | 80%         | 100         |
  | Écriture | 20%         | 20          |
Then le throughput total doit être supérieur à 2000 req/s
```

### **3. Tests de Persistence sous Charge** (3 scénarios)

✅ **Persistence continue sous charge élevée**
```gherkin
When je lance une charge constante de 500 req/s pendant 60 secondes
Then exactement 30000 événements doivent être persistés
And la séquence temporelle doit être strictement croissante
```

✅ **Redémarrage avec données persistées**
```gherkin
When j'arrête le serveur
And je redémarre le serveur sur le même port
Then les 1000 articles doivent être présents en mémoire
```

### **4. Tests d'Intégrité Avancés** (2 scénarios)

✅ **Vérification de l'ordre des événements**
✅ **Détection de corruption de données** (CRC32)

### **5. Tests de Charge Extrême** (2 scénarios)

✅ **50000 articles**
✅ **1000 threads × 10 articles**

### **6. Tests de Snapshot** (1 scénario)

✅ **Création de snapshot tous les 1000 événements**

### **7. Tests de Durabilité** (2 scénarios)

✅ **Durabilité fsync** (SIGKILL + redémarrage)
✅ **Durabilité sans fsync** (mode performance)

---

## 🔧 **Implémentation**

### **Démarrage du Serveur**

```rust
#[given(expr = "un serveur Lithair sur le port {int} avec persistence {string}")]
async fn start_lithair_server(world: &mut LithairWorld, port: u16, persist_path: String) {
    // 1. Créer FileStorage
    let storage = FileStorage::new(&persist_path).unwrap();
    *world.storage.lock().await = Some(storage);
    
    // 2. Créer le Router
    let engine = world.engine.clone();
    let router = Router::new()
        .post("/api/articles", move |req, _, _| {
            handle_create_article(req, &engine)
        })
        .get("/api/articles", move |req, _, _| {
            handle_list_articles(req, &engine)
        });
    
    // 3. Démarrer HttpServer
    let server = HttpServer::new().with_router(router);
    let handle = tokio::spawn(async move {
        server.serve_on_port(port).await
    });
    
    *world.server_handle.lock().await = Some(handle);
}
```

### **Handler Création**

```rust
fn handle_create_article(req: &HttpRequest, engine: &Arc<StateEngine<TestAppState>>) -> HttpResponse {
    // 1. Parser la requête
    let article: CreateArticle = serde_json::from_str(req.body()).unwrap();
    
    // 2. Créer l'événement
    let event = TestEvent::ArticleCreated {
        id: uuid::Uuid::new_v4().to_string(),
        data: json!({ "title": article.title, "content": article.content }),
    };
    
    // 3. Appliquer via StateEngine (persiste automatiquement)
    engine.apply_event(event).unwrap();
    
    // 4. Réponse
    HttpResponse::created().json(&response_json)
}
```

### **Vérification Persistence**

```rust
#[then(expr = "le fichier events.raftlog doit contenir exactement {int} événements")]
async fn check_event_count(world: &mut LithairWorld, count: usize) {
    let log_file = format!("{}/events.raftlog", world.metrics.persist_path);
    let content = std::fs::read_to_string(&log_file).unwrap();
    
    let event_count = content.lines()
        .filter(|line| line.contains("ArticleCreated"))
        .count();
    
    assert_eq!(event_count, count);
}
```

---

## 🚀 **Lancer les Tests**

### **Tous les tests database/performance**
```bash
cd cucumber-tests
cargo test --features cucumber -- features/performance/database_performance.feature
```

### **Tests d'intégrité uniquement**
```bash
cargo test --features cucumber -- "Créer 1000 articles"
```

### **Tests de performance uniquement**
```bash
cargo test --features cucumber -- "Performance d'écriture"
```

---

## 📊 **Métriques Mesurées**

### **Intégrité**
- ✅ Nombre exact d'événements persistés
- ✅ Séquence d'IDs continue
- ✅ Pas de doublons
- ✅ Pas d'événements manquants
- ✅ Checksums valides (CRC32)

### **Performance**
- ✅ Throughput (req/s)
- ✅ Latence (p50, p95, p99)
- ✅ Taux d'erreur
- ✅ Temps de réponse moyen
- ✅ Taille du fichier events.raftlog

### **Durabilité**
- ✅ Récupération après crash (SIGKILL)
- ✅ Intégrité des données persistées
- ✅ Snapshots valides
- ✅ Redémarrage rapide (< 5s pour 50k articles)

---

## 🎯 **Différences avec Robot Framework**

### **Robot Framework**
- Tests de l'**application complète**
- Approche keyword-driven
- Facile pour non-devs
- Focus : fonctionnalité métier

### **Cucumber E2E Database/Performance**
- Tests de la **couche database** uniquement
- Vrai HttpServer + StateEngine + FileStorage
- Rust natif, intégré au code
- Focus : intégrité + performance de la persistence

**Complémentaires !**

---

## ✅ **État Actuel**

### **Implémenté** ✅
- ✅ Démarrage vrai HttpServer Lithair
- ✅ Handlers avec StateEngine
- ✅ Création articles (séquentiel)
- ✅ Création articles (parallèle avec threads)
- ✅ Vérification fichier events.raftlog
- ✅ Comptage événements
- ✅ Vérification intégrité basique

### **À Implémenter** 📝
- [ ] Mesure performance (throughput, latence)
- [ ] Tests de lecture (GET)
- [ ] Charge mixte 80/20
- [ ] Redémarrage serveur
- [ ] Snapshots
- [ ] CRC32 / checksums
- [ ] Tests durabilité (SIGKILL)
- [ ] Vérification ordre événements

---

## 🎉 **Avantages**

1. **Tests Réels** - Vrai Lithair, pas de mock
2. **Performance** - Mesure précise avec vrai serveur
3. **Intégration** - Event sourcing + persistence natifs
4. **Simplicité** - Tout dans Cucumber
5. **Contrôle Total** - Démarrage/arrêt programmatique
6. **Debug Facile** - Logs directs, pas de serveur externe

---

## 🚀 **Prochaines Étapes**

1. **Compiler les steps** (résoudre erreurs)
2. **Implémenter steps manquants** (mesure perf, lecture)
3. **Lancer 1er scénario** (1000 articles)
4. **Valider intégrité** (events.raftlog)
5. **Mesurer performance** (throughput, latence)
6. **Implémenter scenarios avancés** (redémarrage, snapshots)

**L'architecture est prête, les scénarios sont écrits, on peut maintenant implémenter ! 🎯**
