Feature: Benchmarks Isolés - Mémoire vs Disque vs E2E

  Scenario: BENCH 1 - Lecture pure en mémoire (StateEngine)
    Given un serveur Lithair sur le port 20010 avec persistence "/tmp/lithair-bench-read"
    And 10000 articles pré-chargés en mémoire
    When je lis 100000 articles aléatoires via GET
    Then le temps de lecture moyen doit être inférieur à 1ms
    And le throughput de lecture doit dépasser 50000 req/sec

  Scenario: BENCH 2 - Écriture pure sur disque (FileStorage)
    Given un serveur Lithair sur le port 20011 avec persistence "/tmp/lithair-bench-write"
    When je crée 50000 articles en mode écriture directe
    Then le fichier events.raftlog doit contenir exactement 50000 événements "ArticleCreated"
    And le throughput d'écriture doit être mesuré

  Scenario: BENCH 3 - E2E complet (HTTP + Mémoire + Disque en //)
    Given un serveur Lithair sur le port 20012 avec persistence "/tmp/lithair-bench-e2e"
    When je crée 50000 articles via HTTP POST
    Then tous les articles doivent être en mémoire
    And le fichier events.raftlog doit contenir exactement 50000 événements "ArticleCreated"
    And le throughput E2E doit être mesuré

  Scenario: BENCH 4 - Mix Lecture/Écriture (Production realistic)
    Given un serveur Lithair sur le port 20013 avec persistence "/tmp/lithair-bench-mix"
    And 10000 articles pré-chargés en mémoire
    When je lance 80% lectures et 20% écritures pendant 30 secondes
    Then le throughput total doit être mesuré
    And les latences P50, P95, P99 doivent être calculées
