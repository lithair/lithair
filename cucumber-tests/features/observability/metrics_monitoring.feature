# language: fr
Fonctionnalité: Métriques et Monitoring Lithair
  En tant qu'administrateur système
  Je veux monitorer les performances et l'état de santé de Lithair
  Afin d'anticiper les problèmes et optimiser les performances

  Contexte:
    Soit un serveur Lithair avec le monitoring activé
    Et que les métriques soient collectées automatiquement
    Et que l'endpoint /metrics soit exposé

  Scénario: Métriques de performance HTTP
    Quand le serveur traite des requêtes HTTP
    Alors le nombre de requêtes par seconde doit être mesuré
    Et les temps de réponse moyens doivent être tracked
    Et les codes de statut doivent être comptabilisés
    Et les métriques doivent être disponibles sur /metrics

  Scénario: Monitoring de l'utilisation mémoire
    Quand le serveur fonctionne sous charge
    Alors l'utilisation mémoire doit être monitorée en temps réel
    Et les pics de mémoire doivent être détectés
    Et les fuites de mémoire doivent être identifiées
    Et les alertes doivent être déclenchées si nécessaire

  Scénario: Métriques de concurrence et throughput
    Quand 1000 requêtes simultanées sont traitées
    Alors le nombre de connections actives doit être mesuré
    Et le throughput par thread doit être calculé
    Et la latence P95, P99 doit être tracked
    Et les goulots d'étranglement doivent être identifiés

  Scénario: Health checks automatiques
    Quand l'endpoint /health est appelé
    Alors le statut du serveur doit être vérifié
    Et les dépendances externes doivent être testées
    Et un rapport de santé détaillé doit être retourné
    Et le code de statut doit refléter l'état réel

  Scénario: Alertes et notifications proactives
    Quand l'utilisation CPU dépasse 80%
    Et que la latence moyenne dépasse 100ms
    Et que le taux d'erreur dépasse 5%
    Alors une alerte doit être générée automatiquement
    Et les administrateurs doivent être notifiés
    Et les actions correctives doivent être suggérées

  Scénario: Agrégation et rétention des métriques
    Quand les métriques sont collectées pendant 24h
    Alors les données doivent être agrégées par intervalles
    Et les métriques détaillées doivent être archivées
    Et les tendances à long terme doivent être calculées
    Et l'espace de stockage doit être optimisé
