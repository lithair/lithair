# Test de Durabilité Lithair
# Vérifier que MaxDurability garantit ZÉRO perte de données

Feature: Durabilité et Persistance des Données Lithair
  En tant que développeur
  Je veux vérifier que Lithair en mode MaxDurability
  Garantit ZÉRO perte de données

  Background:
    Given la persistence est activée par défaut

  # ==================== TEST DURABILITÉ ====================

  Scenario: Mode MaxDurability - Garantie ZÉRO perte sur 1000 articles
    Given un serveur Lithair sur le port 20100 avec persistence "/tmp/lithair-durability-test"
    When je crée 1000 articles rapidement
    And j'attends 3 secondes pour le flush
    Then le fichier events.raftlog doit exister
    And le fichier events.raftlog doit contenir exactement 1000 événements "ArticleCreated"
    And aucun événement ne doit être manquant
    And le checksum des événements doit être valide

  Scenario: Vérification performance avec MaxDurability
    Given un serveur Lithair sur le port 20101 avec persistence "/tmp/lithair-perf-durable"
    When je mesure le temps pour créer 500 articles
    And j'attends 2 secondes pour le flush
    Then le temps total doit être inférieur à 5 secondes
    And tous les 500 événements doivent être persistés
    And le fichier events.raftlog doit exister

  Scenario: Cohérence Mémoire vs Disque avec MaxDurability
    Given un serveur Lithair sur le port 20102 avec persistence "/tmp/lithair-consistency"
    When je crée 100 articles rapidement
    And j'attends 2 secondes pour le flush
    Then le nombre d'articles en mémoire doit égaler le nombre sur disque
    And tous les checksums doivent correspondre

  Scenario: CRUD complet avec vérification durabilité
    Given un serveur Lithair sur le port 20103 avec persistence "/tmp/lithair-crud-durable"
    When je crée 50 articles rapidement
    And je modifie 25 articles existants
    And je supprime 10 articles
    And j'attends 3 secondes pour le flush
    Then le fichier events.raftlog doit contenir exactement 50 événements "ArticleCreated"
    And le fichier events.raftlog doit contenir exactement 25 événements "ArticleUpdated"
    And le fichier events.raftlog doit contenir exactement 10 événements "ArticleDeleted"
    And l'état final doit avoir 40 articles actifs

  # ==================== TEST FSYNC CRITIQUE ====================

  @critical @fsync
  Scenario: Fsync garantit la persistance immédiate sur disque
    # Ce test vérifie que fsync écrit vraiment les données sur le disque physique
    # et non pas seulement dans le buffer de l'OS
    Given un serveur Lithair sur le port 20104 avec persistence "/tmp/lithair-fsync-test"
    And le mode MaxDurability est activé avec fsync
    When je crée 100 articles critiques
    And je force un flush avec fsync immédiat
    Then les 100 articles doivent être lisibles depuis le fichier immédiatement
    And le fichier ne doit pas être vide
    # Simuler un "crash" en lisant directement le fichier sans passer par le cache
    When je lis le fichier directement avec O_DIRECT si disponible
    Then les données doivent être présentes sur le disque physique

  @critical @crash-recovery
  Scenario: Recovery après crash brutal avec MaxDurability
    # Vérifie qu'après un crash brutal (pas de shutdown propre), 
    # les données flushées avec fsync sont récupérables
    Given un serveur Lithair sur le port 20105 avec persistence "/tmp/lithair-crash-test"
    And le mode MaxDurability est activé avec fsync
    When je crée 500 articles critiques
    And je force un flush avec fsync immédiat
    And je simule un crash brutal du serveur sans shutdown
    # Redémarrer et vérifier
    When je redémarre le serveur depuis "/tmp/lithair-crash-test"
    Then les 500 articles doivent être présents après recovery
    And aucune donnée flushée ne doit être perdue
