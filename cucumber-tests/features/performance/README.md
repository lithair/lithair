# 🎯 Tests de Performance HTTP Lithair

## 📋 **Objectif**

Valider les performances du serveur HTTP Lithair avec des tests E2E Cucumber :
- Throughput (req/s)
- Latence (p50, p95, p99)
- Stabilité sous charge
- Keep-Alive HTTP/1.1
- Persistence avec fsync

---

## 🎯 **Scénarios de Test**

### **1. Throughput Écriture** ⚡
```gherkin
When je crée 1000 articles en parallèle avec 10 workers
Then le throughput doit être supérieur à 1000 requêtes par seconde
```

**Objectif** : ≥ 1000 req/s  
**Workers** : 10  
**Validation** : Persistence + aucune erreur

### **2. Throughput Lecture** 📖
```gherkin
When je lis 5000 fois la liste des articles avec 20 workers
Then le throughput doit être supérieur à 5000 requêtes par seconde
```

**Objectif** : ≥ 5000 req/s  
**Workers** : 20  
**Validation** : Latence p95 < 50ms

### **3. Charge Mixte 80/20** 🔀
```gherkin
When je lance une charge mixte pendant 10 secondes:
  | type     | pourcentage | workers |
  | lecture  | 80          | 16      |
  | écriture | 20          | 4       |
Then le throughput total doit être supérieur à 2000 requêtes par seconde
```

**Objectif** : ≥ 2000 req/s total  
**Mix** : 80% lectures / 20% écritures  
**Validation** : Taux d'erreur < 0.1%

### **4. Performance avec fsync** 💾
```gherkin
Given le serveur a fsync activé sur chaque écriture
When je crée 500 articles séquentiellement
Then le temps total doit être inférieur à 2 secondes
```

**Objectif** : < 2s pour 500 articles  
**Validation** : Zéro perte après kill brutal

### **5. Keep-Alive HTTP/1.1** 🔌
```gherkin
When je fais 100 requêtes avec la même connexion TCP
Then aucune erreur "Connection reset" ne doit survenir
```

**Objectif** : 1 seule connexion TCP  
**Validation** : Pas de "Connection reset by peer"

---

## 🏗️ **Architecture**

```
cucumber-tests/
├── features/performance/
│   ├── http_performance.feature    # Scénarios Gherkin
│   └── README.md                   # Ce fichier
│
└── src/features/steps/
    └── http_performance_steps.rs   # Implémentation
```

### **World State**

```rust
pub struct Metrics {
    // Performance
    pub throughput: f64,              // req/s
    pub total_duration: Duration,
    pub error_count: usize,
    
    // Latence
    pub latency_p50: Duration,
    pub latency_p95: Duration,
    pub latency_p99: Duration,
    
    // Serveur
    pub base_url: String,
    pub server_port: u16,
    pub persist_path: String,
}
```

---

## 🚀 **Lancer les Tests**

### **Tous les tests de performance**
```bash
cargo test --features cucumber -- --tags @performance
```

### **Tests critiques uniquement**
```bash
cargo test --features cucumber -- --tags "@performance and @critical"
```

### **Test spécifique**
```bash
cargo test --features cucumber -- --name "Throughput écriture"
```

---

## 📊 **Métriques Mesurées**

### **Throughput**
- **Définition** : Nombre de requêtes/seconde
- **Calcul** : `total_requests / duration_seconds`
- **Objectifs** :
  - Écriture : ≥ 1000 req/s
  - Lecture : ≥ 5000 req/s
  - Mixte : ≥ 2000 req/s

### **Latence**
- **p50 (médiane)** : 50% des requêtes
- **p95** : 95% des requêtes
- **p99** : 99% des requêtes
- **Objectifs** :
  - p50 < 10ms
  - p95 < 50ms
  - p99 < 100ms

### **Taux d'Erreur**
- **Définition** : `failed_requests / total_requests * 100`
- **Objectif** : < 0.1%

---

## 🔧 **Implémentation**

### **Workers Parallèles**

```rust
let articles_per_worker = count / workers;
let mut handles = vec![];

for worker_id in 0..workers {
    let handle = thread::spawn(move || {
        let client = Client::new();
        for i in 0..articles_per_worker {
            // Créer article
            // Mesurer latence
        }
    });
    handles.push(handle);
}

for handle in handles {
    handle.join().unwrap();
}
```

### **Mesure de Latence**

```rust
let start = Instant::now();
let response = client.post(&url).json(&article).send();
let latency = start.elapsed();

metrics.latencies.push(latency);
```

### **Calcul Percentiles**

```rust
pub fn calculate_percentile(&self, percentile: f64) -> Duration {
    let mut sorted = self.latencies.clone();
    sorted.sort();
    
    let index = ((percentile / 100.0) * sorted.len() as f64) as usize;
    sorted[index.min(sorted.len() - 1)]
}
```

---

## 🐛 **Problèmes Identifiés**

### **1. Connection Reset**
**Symptôme** : `ConnectionResetError(104, 'Connection reset by peer')`

**Cause** : Serveur ferme la connexion après chaque requête

**Solution** :
```rust
// Dans test_server, lire plusieurs requêtes sur la même connexion
loop {
    let mut buffer = [0; 4096];
    match stream.read(&mut buffer) {
        Ok(0) => break, // Client a fermé
        Ok(_) => {
            // Traiter requête
            // Envoyer réponse
            // Continuer
        }
        Err(_) => break,
    }
}
```

### **2. Performance Faible (133 req/s)**
**Cause** : Serveur HTTP basique avec `std::net`

**Solutions** :
1. **Court terme** : Ajuster objectifs temporairement
2. **Moyen terme** : Utiliser tokio pour async
3. **Long terme** : Intégrer hyper dans Lithair

---

## ✅ **TODO**

### **Implémentation Steps**
- [x] Throughput écriture
- [x] Throughput lecture  
- [ ] Charge mixte
- [ ] Keep-Alive HTTP/1.1
- [ ] Charge concurrente
- [ ] Latence sous charge
- [ ] Test de stress
- [ ] Benchmark de référence

### **Optimisations Serveur**
- [ ] Supporter HTTP/1.1 keep-alive
- [ ] Pool de threads pour les connexions
- [ ] Parser HTTP optimisé
- [ ] Intégration tokio/hyper

### **CI/CD**
- [ ] Intégrer dans pipeline CI
- [ ] Benchmarks automatiques
- [ ] Alertes sur régression
- [ ] Rapports de performance

---

## 📚 **Références**

- [Robot Framework Tests](../../robot-tests/) - Tests similaires
- [test_server](../../examples/test_server/) - Serveur de test
- [Lithair HTTP](../../lithair-core/src/http/) - Module HTTP du framework

---

## 🎯 **Prochaines Étapes**

1. **Fixer Connection Reset** (priorité 1)
2. **Implémenter steps manquants** (charge mixte, keep-alive)
3. **Optimiser test_server** ou intégrer Lithair HTTP
4. **Valider tous les scénarios**
5. **Intégrer dans CI**

**Ces tests E2E Cucumber sont spécifiques à Lithair et complémentaires aux tests Robot Framework !** 🚀
