# language: fr
# Event Sourcing vs CRUD - Benchmark Performance
# Validation de la philosophie "append-only" de Lithair

Fonctionnalité: Benchmark Event Sourcing vs CRUD traditionnel
  En tant que développeur
  Je veux comparer les performances event sourcing vs CRUD
  Afin de valider que l'approche append-only est performante

  Contexte:
    Soit la persistence activée par défaut

  # ==================== WRITE PERFORMANCE ====================

  @benchmark @write
  Scénario: Performance d'écriture append-only vs random I/O
    Soit un serveur Lithair sur le port 21000 avec persistence "/tmp/lithair-bench-write"
    Quand je mesure le temps pour créer 10000 articles en mode append
    Alors le temps append-only doit être inférieur à 15 secondes
    Et le throughput append doit être supérieur à 500 writes/sec
    Et toutes les écritures doivent être séquentielles dans le fichier

  @benchmark @write @bulk
  Scénario: Performance d'écriture bulk - Correction massive de données
    Soit un serveur Lithair sur le port 21001 avec persistence "/tmp/lithair-bench-bulk"
    Et 1000 produits existants avec des prix incorrects
    Quand je corrige les 1000 prix en créant des événements PriceUpdated
    Alors les 1000 événements de correction doivent être créés en moins de 2 secondes
    Et l'historique doit montrer 2000 événements (1000 Created + 1000 Updated)
    Et aucune donnée originale ne doit être perdue

  # ==================== READ PERFORMANCE ====================

  @benchmark @read
  Scénario: Performance de lecture depuis mémoire (projection)
    Soit un serveur Lithair sur le port 21002 avec persistence "/tmp/lithair-bench-read"
    Et 10000 articles chargés en mémoire
    Quand je mesure le temps pour 100000 lectures aléatoires
    Alors le temps moyen par lecture doit être inférieur à 0.1ms
    Et le throughput doit être supérieur à 100000 reads/sec
    Et aucune lecture ne doit accéder au disque

  @benchmark @read @history
  Scénario: Performance de lecture d'historique d'entité
    Soit un serveur Lithair sur le port 21003 avec persistence "/tmp/lithair-bench-history"
    Et un article avec 100 événements dans son historique
    Quand je récupère l'historique complet de l'article
    Alors la réponse doit arriver en moins de 50ms
    Et l'historique doit contenir exactement 100 événements
    Et les événements doivent être ordonnés chronologiquement

  # ==================== DATA ADMIN FEATURES ====================

  @benchmark @admin @history
  Scénario: API Data Admin - Endpoint History
    Soit un serveur Lithair sur le port 21004 avec persistence "/tmp/lithair-bench-admin"
    Et 100 articles avec des historiques variés
    Quand j'appelle GET /_admin/data/models/Article/{id}/history pour chaque article
    Alors toutes les réponses doivent arriver en moins de 100ms chacune
    Et chaque réponse doit contenir event_count, events, et timestamps
    Et les events doivent inclure les types Created, Updated, AdminEdit

  @benchmark @admin @edit
  Scénario: API Data Admin - Event-Sourced Edit
    Soit un serveur Lithair sur le port 21005 avec persistence "/tmp/lithair-bench-edit"
    Et un article existant avec id "test-article-001"
    Quand j'appelle POST /_admin/data/models/Article/{id}/edit avec {"title": "Nouveau titre"}
    Alors un nouvel événement AdminEdit doit être créé
    Et l'événement ne doit PAS remplacer les événements précédents
    Et l'historique doit maintenant contenir un événement de plus
    Et le timestamp de l'AdminEdit doit être postérieur aux précédents

  @benchmark @admin @bulk-edit
  Scénario: API Data Admin - Édition bulk event-sourced
    Soit un serveur Lithair sur le port 21006 avec persistence "/tmp/lithair-bench-bulk-edit"
    Et 500 articles existants
    Quand je corrige le champ "category" de tous les 500 articles via l'API edit
    Alors 500 événements AdminEdit doivent être créés en moins de 3 secondes
    Et aucun événement original ne doit être modifié
    Et l'audit trail doit être complet pour les 500 articles

  # ==================== STARTUP PERFORMANCE ====================

  @benchmark @startup
  Scénario: Performance de replay au démarrage
    Soit un fichier events.raftlog avec 100000 événements
    Quand je démarre un serveur Lithair avec ce fichier
    Alors le replay doit prendre moins de 10 secondes
    Et 100000 entités doivent être chargées en mémoire
    Et le serveur doit être prêt à recevoir des requêtes

  @benchmark @startup @snapshot
  Scénario: Performance de démarrage avec snapshot
    Soit un snapshot contenant 100000 entités
    Et un fichier events.raftlog avec 1000 événements post-snapshot
    Quand je démarre un serveur Lithair avec snapshot activé
    Alors le démarrage doit prendre moins de 3 secondes
    Et seulement 1000 événements doivent être rejoués
    Et l'état final doit être identique au scénario sans snapshot

  # ==================== INTEGRITY UNDER LOAD ====================

  @benchmark @integrity
  Scénario: Intégrité event sourcing sous charge
    Soit un serveur Lithair sur le port 21007 avec persistence "/tmp/lithair-bench-integrity"
    Quand je lance 100 threads qui créent chacun 100 articles
    Et je lance 50 threads qui modifient des articles aléatoires
    Et je lance 20 threads qui récupèrent des historiques
    Alors tous les 10000 articles doivent être créés
    Et aucun événement ne doit être perdu
    Et l'ordre des événements doit être globalement cohérent
    Et la validation CRC32 doit passer pour tous les événements

  @benchmark @integrity @hash-chain
  Scénario: Intégrité avec hash chain (future feature)
    Soit un serveur Lithair avec hash chain activé
    Et 1000 événements créés
    Quand je vérifie l'intégrité de la chaîne
    Alors chaque événement doit référencer le hash du précédent
    Et toute modification manuelle du fichier doit être détectée
    Et la chaîne doit être validable de bout en bout

  # ==================== COMPACTION PERFORMANCE ====================

  @benchmark @compaction
  Scénario: Performance de compaction avec snapshot
    Soit un serveur Lithair sur le port 21008 avec persistence "/tmp/lithair-bench-compact"
    Et 50000 événements dont 40000 sont obsolètes
    Quand je déclenche une compaction avec snapshot
    Alors un snapshot doit être créé avec l'état consolidé
    Et les 40000 événements obsolètes doivent être archivés
    Et la compaction doit prendre moins de 5 secondes
    Et l'espace disque utilisé doit diminuer d'au moins 50%

  @benchmark @compaction @retention
  Scénario: Compaction avec politique de rétention
    Soit un serveur Lithair avec politique de rétention "garder 10 derniers par entité"
    Et 100 entités avec 50 événements chacune (5000 total)
    Quand je déclenche une compaction
    Alors seulement 1000 événements doivent rester (100 entités x 10 events)
    Et un snapshot doit capturer l'état avant compaction
    Et le hash chain doit être préservé pour les événements restants

  # ==================== COMPARISON VS CRUD ====================

  @benchmark @comparison
  Scénario: Comparaison directe append-only vs UPDATE simulé
    Soit un serveur Lithair sur le port 21009 avec persistence "/tmp/lithair-bench-compare"
    Et 1000 articles existants
    Quand je mesure le temps pour 1000 modifications en mode append (événements)
    Et je simule 1000 modifications en mode CRUD (lecture-écriture-réécriture)
    Alors le mode append doit être au moins 3x plus rapide
    Et le mode append doit utiliser moins de CPU
    Et les deux modes doivent produire le même état final

  @benchmark @comparison @audit
  Scénario: Valeur ajoutée de l'audit trail
    Soit un serveur Lithair sur le port 21010 avec persistence "/tmp/lithair-bench-audit"
    Et 100 articles avec des modifications multiples
    Quand je demande "qui a modifié quoi et quand" pour chaque article
    Alors la réponse doit être instantanée (< 10ms) grâce à l'historique
    Et l'information doit inclure timestamps, event_types, et données
    Et aucune table d'audit séparée ne doit être nécessaire
