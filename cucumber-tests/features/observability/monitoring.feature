# language: fr
Fonctionnalité: Observabilité et Monitoring
  En tant qu'ingénieur DevOps
  Je veux que Lithair expose des métriques détaillées
  Afin de monitorer la santé et les performances du système

  Contexte:
    Soit un serveur Lithair avec monitoring activé
    Et que les endpoints Prometheus soient configurés
    Et que les health checks soient implémentés

  Scénario: Health checks complets
    Quand j'appelle "/health"
    Alors je dois recevoir le statut "UP" ou "DOWN"
    Quand j'appelle "/ready"
    Alors je dois savoir si le serveur est prêt pour le trafic
    Quand j'appelle "/info"
    Alors je dois recevoir la version et les informations système

  Scénario: Métriques Prometheus
    Quand j'appelle "/observe/metrics"
    Alors je dois recevoir des métriques au format Prometheus
    Et les métriques doivent inclure: requêtes/sec, latence, mémoire
    Et les métriques doivent être étiquetées par endpoint et statut

  Scénario: Performance profiling
    Quand j'appelle "/observe/perf/cpu"
    Alors je dois recevoir l'utilisation CPU actuelle
    Quand j'appelle "/observe/perf/memory"
    Alors je dois recevoir l'utilisation mémoire détaillée
    Quand j'appelle "/observe/perf/latency"
    Alors je dois recevoir les percentiles de latence

  Scénario: Logging structuré
    Quand une erreur se produit
    Alors elle doit être logguée avec niveau ERROR
    Et contenir timestamp, contexte et stack trace
    Quand une requête est traitée
    Alors elle doit être logguée avec niveau INFO
    Et contenir méthode, URL, latence et statut

  Scénario: Alertes automatiques
    Quand la latence dépasse 100ms
    Alors une alerte doit être générée
    Quand la mémoire dépasse 80%
    Alors une alerte critique doit être émise
    Quand le taux d'erreur dépasse 5%
    Alors une alerte doit être déclenchée
