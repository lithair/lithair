Feature: HTTP Server Performance
  En tant que framework haute performance
  Je veux garantir des throughputs élevés et une latence faible
  Pour supporter des applications en production sous charge

  Background:
    Given un serveur Lithair démarre sur le port "21500"
    And le serveur utilise la persistence dans "/tmp/cucumber-perf-test"

  @performance @critical
  Scenario: Throughput écriture - Minimum 1000 req/s
    Given le serveur est prêt à recevoir des requêtes
    When je crée 1000 articles en parallèle avec 10 workers
    Then le temps total doit être inférieur à 1 seconde
    And le throughput doit être supérieur à 1000 requêtes par seconde
    And tous les articles doivent être persistés
    And aucune erreur ne doit être enregistrée

  @performance @critical
  Scenario: Throughput lecture - Minimum 5000 req/s
    Given le serveur contient 100 articles pré-créés
    When je lis 5000 fois la liste des articles avec 20 workers
    Then le temps total doit être inférieur à 1 seconde
    And le throughput doit être supérieur à 5000 requêtes par seconde
    And la latence p95 doit être inférieure à 50 millisecondes
    And aucune erreur de connexion ne doit survenir

  @performance
  Scenario: Charge mixte 80/20 - Minimum 2000 req/s
    Given le serveur contient 50 articles pré-créés
    When je lance une charge mixte pendant 10 secondes:
      | type     | pourcentage | workers |
      | lecture  | 80          | 16      |
      | écriture | 20          | 4       |
    Then le throughput total doit être supérieur à 2000 requêtes par seconde
    And le taux d'erreur doit être inférieur à 0.1%
    And la latence p99 doit être inférieure à 100 millisecondes

  @performance @durability
  Scenario: Performance avec persistence fsync
    Given le serveur a fsync activé sur chaque écriture
    When je crée 500 articles séquentiellement
    Then le temps total doit être inférieur à 2 secondes
    And tous les articles doivent être dans le fichier events.raftlog
    And aucun article ne doit être perdu après un redémarrage brutal

  @performance @http
  Scenario: Keep-Alive HTTP/1.1
    Given le serveur supporte HTTP/1.1 keep-alive
    When je fais 100 requêtes avec la même connexion TCP
    Then toutes les requêtes doivent réussir
    And aucune erreur "Connection reset" ne doit survenir
    And le nombre de connexions TCP doit être exactement 1

  @performance @concurrency
  Scenario: Charge concurrente élevée - 50 workers
    Given le serveur est prêt
    When je lance 50 workers en parallèle
    And chaque worker crée 20 articles
    Then 1000 articles doivent être créés au total
    And le temps total doit être inférieur à 5 secondes
    And tous les articles doivent avoir des IDs uniques
    And aucune corruption de données ne doit être détectée

  @performance @latency
  Scenario: Latence sous charge constante
    Given le serveur est sous charge constante de 500 req/s
    When je mesure la latence pendant 30 secondes
    Then la latence p50 doit être inférieure à 10 millisecondes
    And la latence p95 doit être inférieure à 50 millisecondes
    And la latence p99 doit être inférieure à 100 millisecondes
    And aucun timeout ne doit survenir

  @performance @stress
  Scenario: Test de stress - 10000 articles
    Given le serveur démarre avec une base vide
    When je crée 10000 articles en batches de 100
    Then tous les 10000 articles doivent être créés
    And le temps total doit être inférieur à 30 secondes
    And la mémoire du serveur doit rester sous 500 MB
    And le fichier events.raftlog doit contenir exactement 10000 événements

  @performance @regression
  Scenario: Benchmark de référence
    Given le serveur est en mode benchmark
    When je lance le benchmark standard:
      | opération | nombre | workers |
      | POST      | 1000   | 10      |
      | GET       | 5000   | 20      |
      | PUT       | 500    | 5       |
    Then les métriques doivent être enregistrées
    And le rapport de performance doit être généré
    And les métriques ne doivent pas régresser de plus de 10%
