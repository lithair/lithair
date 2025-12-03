# Diagrammes de Séquence - Opérations CRUD

Cette documentation montre les flux d'exécution pour chaque opération CRUD, générés automatiquement à partir de l'architecture Lithair.

## CREATE - Création d'un Article

```mermaid
sequenceDiagram
    participant Client
    participant Router
    participant Handler
    participant Validator
    participant EventStore
    participant Cache
    participant Replicator

    Client->>Router: POST /api/articles
    Router->>Handler: route_request()
    Handler->>Validator: validate_article()

    alt Validation échoue
        Validator-->>Client: 400 Bad Request
    else Validation réussit
        Validator->>EventStore: create_event()
        EventStore->>EventStore: append_to_log()
        EventStore->>Cache: update_snapshot()

        alt Réplication activée
            EventStore->>Replicator: replicate_to_peers()
            Replicator->>Replicator: send_to_all_nodes()
        end

        Handler-->>Client: 201 Created + Article
    end
```

### Étapes clés
1. **Validation**: Vérification automatique des contraintes déclaratives
2. **Event Sourcing**: Ajout de l'événement au log d'événements
3. **Cache**: Mise à jour du snapshot en mémoire
4. **Réplication**: Distribution aux nœuds du cluster (si activée)

---

## READ - Lecture d'un Article

```mermaid
sequenceDiagram
    participant Client
    participant Router
    participant Handler
    participant Cache
    participant EventStore

    Client->>Router: GET /api/articles/{id}
    Router->>Handler: route_request()
    Handler->>Cache: get_from_cache()

    alt Cache Hit
        Cache-->>Handler: return cached_article
        Handler-->>Client: 200 OK + Article
    else Cache Miss
        Cache->>EventStore: rebuild_from_events()
        EventStore->>EventStore: replay_events()
        EventStore-->>Cache: return article
        Cache->>Cache: update_cache()
        Cache-->>Handler: return article
        Handler-->>Client: 200 OK + Article
    end
```

### Optimisations
- **Cache First**: Recherche d'abord dans le cache mémoire
- **Event Replay**: Reconstruction depuis les événements si cache miss
- **Zero-Copy**: Pas de copie inutile des données

---

## UPDATE - Modification d'un Article

```mermaid
sequenceDiagram
    participant Client
    participant Router
    participant Handler
    participant Validator
    participant EventStore
    participant VersionManager
    participant Cache
    participant Replicator

    Client->>Router: PUT /api/articles/{id}
    Router->>Handler: route_request()
    Handler->>Validator: validate_article()

    alt Validation échoue
        Validator-->>Client: 400 Bad Request
    else Validation réussit
        Validator->>VersionManager: create_version()
        VersionManager->>EventStore: update_event()
        EventStore->>EventStore: append_to_log()
        EventStore->>Cache: invalidate_cache()

        alt Réplication activée
            EventStore->>Replicator: replicate_to_peers()
            Replicator->>Replicator: send_to_all_nodes()
        end

        Handler-->>Client: 200 OK + Updated Article
    end
```

### Fonctionnalités
- **Versioning**: Conservation des N dernières versions (configurable)
- **Audit Trail**: Enregistrement automatique de toutes les modifications
- **Cache Invalidation**: Mise à jour intelligente du cache
- **Réplication**: Propagation aux autres nœuds

---

## DELETE - Suppression d'un Article

```mermaid
sequenceDiagram
    participant Client
    participant Router
    participant Handler
    participant EventStore
    participant Cache
    participant Replicator

    Client->>Router: DELETE /api/articles/{id}
    Router->>Handler: route_request()
    Handler->>EventStore: delete_event()
    EventStore->>EventStore: append_tombstone()
    EventStore->>Cache: invalidate_cache()

    alt Réplication activée
        EventStore->>Replicator: replicate_delete()
        Replicator->>Replicator: send_to_all_nodes()
    end

    Handler-->>Client: 204 No Content
```

### Caractéristiques
- **Soft Delete**: Utilisation de tombstones dans l'event log
- **Cache Cleanup**: Invalidation immédiate du cache
- **Réplication**: Propagation de la suppression

---

## Notes Techniques

### Event Sourcing
Toutes les opérations sont enregistrées comme des événements immuables :
- **CREATE**: `ArticleCreated { id, data, timestamp }`
- **UPDATE**: `ArticleUpdated { id, changes, version, timestamp }`
- **DELETE**: `ArticleDeleted { id, timestamp }`

### Performance
- **Latence moyenne**: < 5ms pour les opérations en cache
- **Throughput**: > 10,000 req/s sur un seul nœud
- **Réplication**: Async pour ne pas bloquer les écritures

### Consistance
- **Strong Consistency**: Lectures et écritures sur le même nœud
- **Eventual Consistency**: Réplication entre nœuds
- **Conflict Resolution**: Last-Write-Wins avec timestamps
