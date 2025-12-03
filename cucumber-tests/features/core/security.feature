# language: fr
Fonctionnalité: Sécurité Enterprise
  En tant qu'administrateur système
  Je veux que Lithair fournisse des protections avancées
  Afin de sécuriser mes applications contre les menaces

  Contexte:
    Soit un serveur Lithair avec firewall activé
    Et que les politiques de sécurité soient configurées
    Et que le middleware RBAC soit initialisé

  Scénario: Protection contre les attaques DDoS
    Quand une IP envoie plus de 100 requêtes/minute
    Alors cette IP doit être bloquée automatiquement
    Et un message d'erreur 429 doit être retourné
    Et l'incident doit être loggué

  Scénario: Contrôle d'accès par rôles (RBAC)
    Quand un utilisateur "Customer" accède à "/admin"
    Alors il doit recevoir une erreur 403 Forbidden
    Quand un utilisateur "Admin" accède à "/admin"
    Alors il doit recevoir une réponse 200 OK

  Scénario: Validation des tokens JWT
    Quand je fournis un token JWT valide
    Alors ma requête doit être acceptée
    Quand je fournis un token JWT expiré
    Alors ma requête doit être rejetée avec 401

  Scénario: Filtrage IP géographique
    Quand une requête provient d'une IP autorisée
    Alors elle doit être traitée normalement
    Quand une requête provient d'une IP bloquée
    Alors elle doit être rejetée avec 403

  Scénario: Rate limiting par endpoint
    Quand j'appelle "/api/sensitive" plus de 10 fois/minute
    Alors je dois être limité après la 10ème requête
    Et pouvoir continuer après 1 minute d'attente
