# Tests de fiabilité Lithair - Recovery, Corruption, Concurrence
# Valide la robustesse du moteur dans des conditions réelles

Feature: TEST FIABILITÉ MOTEUR - Recovery & Durabilité
  En tant que développeur
  Je veux valider la fiabilité du moteur Lithair
  Dans des scénarios de crash, corruption et concurrence

  Background:
    Given le moteur Lithair est initialisé en mode MaxDurability

  # ==================== TEST RECOVERY APRÈS CRASH ====================

  @core
  Scenario: Recovery - Récupération après crash simulé
    Given un moteur avec persistence dans "/tmp/lithair-recovery-test"

    # Phase 1 : Écriture de données
    When je crée 10000 articles directement dans le moteur
    And je modifie 2000 articles directement dans le moteur
    And j'attends le flush complet du moteur
    Then le fichier events.raftlog doit contenir exactement 12000 événements

    # Phase 2 : Simuler un crash (arrêt brutal sans shutdown)
    When je simule un crash du moteur

    # Phase 3 : Redémarrage et recovery
    When je redémarre le moteur depuis "/tmp/lithair-recovery-test"
    And je recharge tous les événements depuis le disque

    # Phase 4 : Vérifications post-recovery
    Then le moteur doit avoir 10000 articles en mémoire après recovery
    And tous les articles doivent être identiques à l'état pré-crash
    And le fichier events.raftlog doit contenir exactement 12000 événements
    And aucune donnée ne doit être perdue

    # Phase 5 : Continuer après recovery
    When je crée 1000 articles supplémentaires après recovery
    And j'attends le flush complet du moteur
    Then le fichier events.raftlog doit contenir exactement 13000 événements
    And le moteur doit avoir 11000 articles en mémoire

  # ==================== TEST CORRUPTION FICHIER ====================

  @core
  Scenario: Corruption - Détection de fichier corrompu
    Given un moteur avec persistence dans "/tmp/lithair-corruption-test"

    # Phase 1 : Créer des données valides
    When je crée 5000 articles directement dans le moteur
    And j'attends le flush complet du moteur
    Then le fichier events.raftlog doit contenir exactement 5000 événements

    # Phase 2 : Corrompre le fichier (tronquer)
    When je tronque le fichier events.raftlog à 50% de sa taille

    # Phase 3 : Tentative de recovery avec fichier corrompu
    When je redémarre le moteur depuis "/tmp/lithair-corruption-test"
    And je tente de recharger les événements depuis le disque

    # Phase 4 : Vérifications
    Then le moteur doit détecter la corruption
    And le moteur doit charger uniquement les événements valides
    And le nombre d'articles chargés doit être inférieur à 5000
    And aucun panic ne doit se produire

  # ==================== TEST CONCURRENCE ====================

  @core
  Scenario: Concurrence - Écritures parallèles avec SCC2
    Given un moteur avec persistence dans "/tmp/lithair-concurrency-test"

    # Phase 1 : Écritures séquentielles de référence
    When je crée 1000 articles directement dans le moteur
    And j'attends le flush complet du moteur
    Then le fichier events.raftlog doit contenir exactement 1000 événements

    # Phase 2 : Écritures parallèles (10 threads)
    When je lance 10 threads qui créent chacun 1000 articles en parallèle
    And j'attends que tous les threads terminent
    And j'attends le flush complet du moteur

    # Phase 3 : Vérifications d'intégrité
    Then le moteur doit avoir 11000 articles en mémoire
    And le fichier events.raftlog doit contenir exactement 11000 événements
    And aucun article ne doit être dupliqué
    And aucun article ne doit être perdu
    And tous les IDs doivent être uniques
    And le fichier events.raftlog ne doit pas être corrompu

  @core
  Scenario: Déduplication en concurrence - Même événement réémis
    When je lance 10 threads qui réémettent chacun 100 fois le même événement idempotent
    Then l'événement idempotent ne doit être appliqué qu'une seule fois en présence de concurrence
    And le fichier de déduplication doit contenir exactement 1 identifiant pour cet événement

  # ==================== TEST DURABILITÉ FSYNC ====================

  @core
  Scenario: Durabilité - Validation fsync MaxDurability
    Given un moteur avec persistence dans "/tmp/lithair-durability-test"

    # Phase 1 : Écriture avec MaxDurability
    When je crée 1000 articles directement dans le moteur
    And je force un fsync immédiat

    # Phase 2 : Vérification immédiate sur disque
    Then les 1000 articles doivent être lisibles depuis le fichier
    And le fichier events.raftlog ne doit pas être vide
    And la taille du fichier doit correspondre aux données écrites

    # Phase 3 : Crash immédiat après écriture
    When je simule un crash immédiatement après l'écriture
    And je redémarre le moteur depuis "/tmp/lithair-durability-test"
    And je recharge tous les événements depuis le disque

    # Phase 4 : Validation zéro perte
    Then le moteur doit avoir 1000 articles en mémoire après recovery
    And aucune donnée ne doit être perdue malgré le crash immédiat

  # ==================== TEST STRESS LONGUE DURÉE ====================

  @stress
  Scenario: Stress - Stabilité longue durée (1 minute)
    Given un moteur avec persistence dans "/tmp/lithair-stress-longue-duree"

    # Phase 1 : Injection continue pendant 60 secondes
    When je lance une injection continue d'articles pendant 60 secondes
    And je mesure le throughput moyen sur la période

    # Phase 2 : Vérifications de stabilité
    Then le throughput moyen doit rester supérieur à 200000 articles/sec
    And le throughput ne doit pas dégrader de plus de 10% sur la période
    And aucune fuite mémoire ne doit être détectée
    And le moteur doit rester responsive

    # Phase 3 : Vérifications post-stress
    And j'attends le flush complet du moteur
    Then tous les événements doivent être persistés
    And le fichier events.raftlog ne doit pas être corrompu
    And le moteur doit pouvoir redémarrer correctement
