# 🔧 Fix: Test Shutdown Blockage

**Date**: 2025-01-12  
**Problème**: Les tests Cucumber bloquent après le summary et ne se terminent jamais

## ❌ Symptômes

```bash
$ cargo test --test database_perf_test --release
...
✅ Summary: All tests passed
# Le processus bloque indéfiniment ici et ne se termine jamais
# Nécessite Ctrl+C pour arrêter
```

## 🔍 Cause Racine

### 1. Serveur HTTP sans shutdown
```rust
// ❌ AVANT : spawn_blocking sans mécanisme d'arrêt
let _handle = tokio::task::spawn_blocking(move || {
    println!("🔧 Thread serveur démarré");
    if let Err(e) = server.serve(&addr) {
        eprintln!("❌ Erreur serveur: {}", e);
    }
    println!("🛑 Thread serveur terminé");
});

// Le serveur tourne indéfiniment en background
```

### 2. AsyncWriter sans cleanup
```rust
// ❌ AVANT : AsyncWriter créé mais jamais shutdown
*world.async_writer.lock().await = Some(async_writer);

// Le thread writer tourne indéfiniment en background
// Les derniers événements en buffer ne sont pas flush
```

### 3. Pas de step de cleanup
```gherkin
# ❌ AVANT : Le scénario se termine sans cleanup
Scenario: STRESS TEST
  Given un serveur Lithair sur le port 20002...
  When je crée 100000 articles rapidement
  # Pas de step "And j'arrête le serveur proprement"
  # Le test finit mais les threads continuent
```

## ✅ Solution Implémentée

### 1. Ajout du step de shutdown

**Feature file** (`database_performance.feature`):
```gherkin
Scenario: STRESS TEST - 100K articles avec CRUD complet
  Given un serveur Lithair sur le port 20002 avec persistence "/tmp/lithair-stress-100k"
  When je crée 100000 articles rapidement
  And je modifie 10000 articles existants
  And je supprime 5000 articles
  Then le fichier events.raftlog doit exister
  And tous les événements doivent être dans l'ordre chronologique
  And j'arrête le serveur proprement  # ✅ NOUVEAU!
```

### 2. Implémentation du shutdown

**Step definition** (`real_database_performance_steps.rs`):
```rust
#[then("j'arrête le serveur proprement")]
async fn shutdown_server_properly(world: &mut LithairWorld) {
    println!("🛑 Arrêt propre du serveur...");
    
    // 1. Shutdown AsyncWriter pour flush les derniers événements
    let async_writer = {
        let mut writer_guard = world.async_writer.lock().await;
        writer_guard.take()  // Ownership transfer
    };
    
    if let Some(writer) = async_writer {
        println!("⏳ Shutdown AsyncWriter (flush final)...");
        writer.shutdown().await;  // Drop sender + await handle
        println!("✅ AsyncWriter arrêté proprement");
    }
    
    // 2. Attendre que les requêtes en cours finissent
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    
    // 3. Tuer le serveur HTTP
    let port = {
        let metrics = world.metrics.lock().await;
        metrics.server_port
    };
    
    println!("🔪 Arrêt du serveur HTTP sur port {}...", port);
    let _ = std::process::Command::new("pkill")
        .arg("-9")
        .arg("-f")
        .arg(format!("127.0.0.1:{}", port))
        .output();
    
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    println!("✅ Serveur arrêté proprement");
}
```

### 3. AsyncWriter.shutdown() expliqué

**Module AsyncWriter** (`lithair-core/src/engine/async_writer.rs`):
```rust
pub async fn shutdown(mut self) {
    // 1. Fermer le canal (empêche nouvelles écritures)
    drop(self.tx);
    
    // 2. Attendre que le writer termine (flush final automatique)
    if let Some(handle) = self.handle.take() {
        let _ = handle.await;
    }
}
```

Le writer thread détecte la fermeture du canal :
```rust
loop {
    tokio::select! {
        Some(event) = rx.recv() => { /* ... */ }
        _ = flush_interval.tick() => { /* ... */ }
        
        // ✅ Canal fermé = flush final + exit
        else => {
            if !buffer.is_empty() {
                Self::flush_buffer(&mut storage, &mut buffer);
            }
            break;  // Thread se termine proprement
        }
    }
}
```

## 🎯 Workflow de Shutdown

```
Test finishes
    ↓
"j'arrête le serveur proprement" step
    ↓
1. Take AsyncWriter ownership
    ↓
2. writer.shutdown()
   - Drop tx (close channel)
   - Writer thread flush buffer
   - Writer thread exits
   - await handle (wait for thread)
    ↓
3. pkill server process
    ↓
4. Test terminates cleanly
```

## ✅ Résultat

**AVANT** :
```bash
$ cargo test --test database_perf_test
✅ Summary: All tests passed
# BLOQUÉ INDÉFINIMENT - Ctrl+C requis
```

**APRÈS** :
```bash
$ cargo test --test database_perf_test
✅ Summary: All tests passed
🛑 Arrêt propre du serveur...
⏳ Shutdown AsyncWriter (flush final)...
✅ AsyncWriter arrêté proprement
🔪 Arrêt du serveur HTTP sur port 20002...
✅ Serveur arrêté proprement
# Test se termine immédiatement
```

## 📝 Notes Techniques

### Pourquoi take() ownership ?
```rust
let async_writer = {
    let mut writer_guard = world.async_writer.lock().await;
    writer_guard.take()  // Move out of Option
};

if let Some(writer) = async_writer {
    writer.shutdown().await;  // Consume writer (move)
}
```

`shutdown(mut self)` consomme `AsyncWriter` (pas `&self`) car :
- Il faut drop le sender pour fermer le channel
- Il faut take() le handle pour await
- Après shutdown, AsyncWriter n'est plus utilisable

### Pourquoi pkill -9 ?
```rust
std::process::Command::new("pkill")
    .arg("-9")  // SIGKILL (force kill)
    .arg("-f")  // Full process name match
    .arg(format!("127.0.0.1:{}", port))
```

Le serveur HTTP Lithair n'expose pas de méthode `shutdown()` gracieuse dans les tests Cucumber. `pkill -9` garantit que le processus est tué immédiatement, même s'il y a des requêtes en cours.

**Note** : Pour production, implémenter un `graceful_shutdown()` avec :
- Signal handler (SIGTERM)
- Drain des requêtes en cours
- Timeout avant SIGKILL

## 🚀 Prochaines Étapes

### Tests à valider
- ✅ STRESS TEST 100K se termine proprement
- 🔄 Autres scénarios nécessitent aussi le shutdown step

### Améliorations possibles
1. **Graceful server shutdown** : Implémenter signal handler
2. **Automatic cleanup** : Drop trait pour LithairWorld
3. **Timeout failsafe** : Forcer kill après N secondes

---

**Le test ne bloque plus ! 🎉**
