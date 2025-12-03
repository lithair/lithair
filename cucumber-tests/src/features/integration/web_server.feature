# language: fr
Fonctionnalité: Serveur Web Complet
  En tant que développeur web
  Je veux que Lithair serve des applications web complètes
  Afin de remplacer complètement la stack traditionnelle

  Contexte:
    Soit une application Lithair avec frontend intégré
    Et que les assets soient chargés en mémoire
    Et que les APIs REST soient exposées

  Scénario: Service des pages HTML
    Quand un client demande la page d'accueil
    Alors la page doit être servie depuis la mémoire
    Et le chargement doit prendre moins de 10ms
    Et contenir tous les assets CSS/JS

  Scénario: API CRUD complète
    Quand je fais un GET sur "/api/articles"
    Alors je dois recevoir la liste des articles
    Quand je fais un POST sur "/api/articles"
    Alors un nouvel article doit être créé
    Quand je fais un PUT sur "/api/articles/1"
    Alors l'article 1 doit être mis à jour
    Quand je fais un DELETE sur "/api/articles/1"
    Alors l'article 1 doit être supprimé

  Scénario: CORS pour frontend externe
    Quand mon frontend Next.js appelle l'API Lithair
    Alors les headers CORS doivent être corrects
    Et toutes les méthodes HTTP doivent être autorisées
    Et les origines approuvées doivent être configurées

  Scénario: WebSockets temps réel
    Quand je me connecte via WebSocket
    Alors la connexion doit être établie instantanément
    Et les événements doivent être poussés en temps réel
    Et la connexion doit rester stable sous charge

  Scénario: Cache intelligent des assets
    Quand un asset statique est demandé
    Alors il doit être servi depuis le cache SCC2
    Et le cache doit avoir un hit rate > 95%
    Et les assets doivent être compressés automatiquement
