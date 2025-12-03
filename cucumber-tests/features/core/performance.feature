# language: fr
Fonctionnalité: Performance Ultra-Haute
  En tant que développeur d'applications critiques
  Je veux que Lithair offre des performances exceptionnelles
  Afin de servir des millions de requêtes par seconde

  Contexte:
    Soit un serveur Lithair démarré
    Et que le moteur SCC2 soit activé
    Et que les optimisations lock-free soient configurées

  Scénario: Serveur HTTP avec performances maximales
    Quand je démarre le serveur SCC2 sur le port 18321
    Alors le serveur doit répondre en moins de 1ms
    Et supporter plus de 40M requêtes/seconde
    Et consommer moins de 100MB de mémoire

  Scénario: Benchmark JSON throughput
    Quand j'envoie 1000 requêtes JSON de 64KB
    Alors le throughput doit dépasser 20GB/s
    Et la latence moyenne doit être inférieure à 0.5ms

  Scénario: Concurrence massive
    Quand 1000 clients se connectent simultanément
    Alors aucun client ne doit être rejeté
    Et le serveur doit maintenir la latence sous 10ms

  Scénario: Évolution des performances sous charge
    Quand la charge augmente de 10x à 100x
    Alors les performances doivent dégrader linéairement
    Et le serveur ne doit jamais crasher
