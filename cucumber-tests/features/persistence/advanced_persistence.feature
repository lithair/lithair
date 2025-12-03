# language: fr
@advanced @wip
Fonctionnalité: Persistance Avancée Multi-Fichiers
  En tant que développeur d'applications critiques à haute disponibilité
  Je veux une persistance robuste avec multi-fichiers et vérification d'intégrité
  Afin de garantir zéro perte de données même en cas de crash

  Contexte:
    Soit un moteur Lithair avec persistance multi-fichiers activée
    Et que le mode de vérification strict soit activé

  Scénario: Synchronisation Mémoire <-> Fichier en temps réel
    Quand je crée 100 articles en mémoire
    Alors chaque article doit être écrit immédiatement sur disque
    Et la lecture du fichier doit retourner exactement 100 articles
    Et les checksums mémoire/fichier doivent correspondre
    Et aucune donnée ne doit être perdue en cas de crash immédiat

  Scénario: Multi-Tables avec fichiers séparés
    Étant donné une base avec 3 tables: "articles", "users", "comments"
    Quand j'insère des données dans chaque table
    Alors 3 fichiers distincts doivent être créés: "articles.raft", "users.raft", "comments.raft"
    Et chaque fichier doit contenir uniquement les données de sa table
    Et la taille totale des fichiers doit correspondre aux données insérées
    Et je peux lire chaque table indépendamment

  Scénario: Transactions ACID avec WAL (Write-Ahead Log)
    Quand je démarre une transaction multi-tables
    Et j'insère 10 articles, 5 users, 20 comments
    Alors le WAL doit contenir toutes les opérations dans l'ordre
    Et aucune donnée ne doit être visible avant le commit
    Quand je commit la transaction
    Alors toutes les données doivent apparaître atomiquement
    Et le WAL doit être vidé après confirmation
    Et les fichiers de données doivent être à jour

  Scénario: Rollback en cas d'échec de transaction
    Quand je démarre une transaction
    Et j'insère 50 articles valides
    Et j'insère 1 article invalide qui provoque une erreur
    Alors la transaction doit être rollback automatiquement
    Et aucun des 51 articles ne doit être persisté
    Et l'état mémoire doit être restauré
    Et les fichiers ne doivent pas être modifiés

  Scénario: Vérification d'intégrité avec checksums CRC32
    Étant donné 500 articles persistés avec checksums
    Quand je lis chaque article depuis le disque
    Alors le checksum CRC32 doit être vérifié pour chaque lecture
    Et toute corruption doit être détectée immédiatement
    Et un log d'erreur doit être généré pour les corruptions
    Et les articles corrompus doivent être marqués comme invalides

  Scénario: Compaction et optimisation des fichiers
    Étant donné un fichier de 10000 événements avec 3000 suppressions
    Quand je lance la compaction manuelle
    Alors un nouveau fichier optimisé doit être créé
    Et il doit contenir uniquement les 7000 événements actifs
    Et l'ancien fichier doit être archivé avec timestamp
    Et la taille du fichier doit être réduite d'au moins 30%
    Et toutes les données doivent rester accessibles

  Scénario: Sauvegarde incrémentielle avec delta
    Étant donné une base de données avec 1000 articles
    Quand je modifie 50 articles
    Et je lance une sauvegarde incrémentielle
    Alors seuls les 50 articles modifiés doivent être sauvegardés
    Et un fichier delta "backup-TIMESTAMP.delta" doit être créé
    Et la restauration doit reconstruire l'état exact
    Et le temps de backup doit être inférieur à 100ms

  Scénario: Réplication asynchrone des fichiers
    Étant donné 3 nœuds Lithair en cluster
    Quand j'écris 200 articles sur le leader
    Alors les fichiers doivent être répliqués sur tous les followers
    Et chaque nœud doit avoir des fichiers identiques
    Et les checksums doivent correspondre entre nœuds
    Et la latence de réplication doit être inférieure à 50ms

  Scénario: Lecture optimisée avec cache mémoire
    Étant donné 10000 articles persistés sur disque
    Et un cache LRU de 1000 entrées
    Quand je lis 100 articles fréquemment accédés
    Alors 99% des lectures doivent venir du cache
    Et seulement 1 article doit être lu depuis le disque
    Et la latence moyenne doit être inférieure à 0.1ms
    Et le taux de hit cache doit être supérieur à 95%

  Scénario: Gestion de plusieurs versions de format
    Étant donné des fichiers au format v1, v2, et v3
    Quand je charge les données avec migration automatique
    Alors tous les formats doivent être lus correctement
    Et les données doivent être migrées vers le format v3
    Et les anciens fichiers doivent être conservés en backup
    Et aucune donnée ne doit être perdue pendant la migration

  Scénario: Performances d'écriture batch
    Quand j'écris 10000 articles en mode batch
    Alors toutes les écritures doivent être groupées en lots de 1000
    Et le débit doit dépasser 50000 écritures/seconde
    Et l'utilisation mémoire doit rester stable
    Et tous les articles doivent être persistés correctement
    Et la vérification finale doit confirmer 10000 articles

  Scénario: Récupération après crash pendant l'écriture
    Étant donné une écriture batch de 5000 articles en cours
    Quand le serveur crash au milieu (après 2500 articles)
    Et je redémarre le serveur
    Alors les 2500 premiers articles doivent être présents
    Et les 2500 suivants doivent être absents
    Et le WAL doit être rejoué automatiquement
    Et l'état doit être cohérent (pas de corruption)
    Et je peux continuer à écrire normalement

  Scénario: Monitoring de l'espace disque
    Étant donné un quota disque de 1GB
    Quand l'utilisation atteint 90%
    Alors une alerte WARNING doit être émise
    Et la compaction automatique doit démarrer
    Quand l'utilisation atteint 95%
    Alors les écritures non-critiques doivent être bloquées
    Et une alerte CRITICAL doit être envoyée
    Et un nettoyage d'urgence doit être déclenché

  Scénario: Chiffrement des données au repos (AES-256)
    Étant donné le chiffrement AES-256-GCM activé
    Quand j'écris 1000 articles sensibles
    Alors chaque fichier doit être chiffré avec une clé unique
    Et les données en clair ne doivent jamais toucher le disque
    Et la lecture doit déchiffrer automatiquement
    Et les performances ne doivent pas dégrader de plus de 10%
    Et les fichiers doivent être illisibles sans la clé

  Scénario: Audit trail complet de persistance
    Quand j'effectue 100 opérations variées (CRUD)
    Alors chaque opération doit être loggée dans l'audit trail
    Et chaque log doit contenir: timestamp, user_id, operation, data_hash
    Et l'audit trail doit être immuable (append-only)
    Et je peux reconstituer l'historique complet
    Et détecter toute tentative de modification

  Scénario: Backup à chaud sans interruption de service
    Étant donné un serveur en production avec trafic continu
    Quand je lance un backup complet
    Alors le backup doit se faire sans bloquer les écritures
    Et les lectures doivent continuer normalement
    Et le backup doit être consistent (snapshot à un instant T)
    Et la performance ne doit pas dégrader de plus de 5%
    Et le fichier de backup doit être compressé (gzip)

  Scénario: Restauration point-in-time
    Étant donné des backups horaires depuis 7 jours
    Quand je veux restaurer l'état d'il y a 3 jours, 14h35
    Alors le système doit identifier le snapshot + deltas nécessaires
    Et restaurer l'état exact à ce timestamp
    Et toutes les données postérieures doivent être ignorées
    Et la restauration doit prendre moins de 2 minutes

  Scénario: Gestion de fichiers volumineux (>10GB)
    Étant donné une table avec 10 millions d'articles (15GB de données)
    Quand j'effectue des opérations CRUD
    Alors le fichier doit être fragmenté en chunks de 1GB
    Et chaque chunk doit avoir son propre index
    Et les lectures doivent cibler le bon chunk directement
    Et les performances doivent rester constantes
    Et la mémoire utilisée ne doit pas dépasser 500MB

  Scénario: Détection et réparation automatique de corruption
    Étant donné un fichier avec 5 blocs corrompus sur 1000
    Quand le système détecte la corruption au démarrage
    Alors les blocs corrompus doivent être identifiés précisément
    Et le système doit tenter une réparation depuis le WAL
    Et si impossible, restaurer depuis le dernier snapshot
    Et les blocs irrécupérables doivent être marqués
    Et un rapport de corruption doit être généré
