# language: fr
Fonctionnalité: Sessions Persistantes Lithair
  En tant qu'utilisateur d'une application web
  Je veux que ma session reste active entre les redémarrages du serveur
  Afin de ne pas avoir à me reconnecter constamment

  Contexte:
    Soit un serveur Lithair avec sessions persistantes activées
    Et que le store de sessions soit configuré pour la persistance
    Et que les cookies de session soient sécurisés

  Scénario: Création et persistance d'une session
    Quand un utilisateur se connecte avec des identifiants valides
    Alors une session doit être créée avec un ID unique
    Et la session doit être persistée dans le store
    Et un cookie sécurisé doit être retourné
    Et le cookie doit avoir les attributs HttpOnly, Secure, SameSite

  Scénario: Reconnexion automatique après redémarrage
    Quand un utilisateur a une session active
    Et que le serveur redémarre
    Alors l'utilisateur doit rester connecté
    Et sa session doit être rechargée depuis le store persistant
    Et toutes les données de session doivent être intactes

  Scénario: Timeout de session inactivité
    Quand un utilisateur est inactif pendant 30 minutes
    Alors sa session doit expirer automatiquement
    Et sa prochaine requête doit être traitée comme anonyme
    Et les données de session doivent être nettoyées

  Scénario: Gestion multi-utilisateurs simultanés
    Quand 100 utilisateurs se connectent simultanément
    Alors chaque utilisateur doit recevoir une session unique
    Et les sessions ne doivent pas se confliter
    Et le store doit gérer la concurrence sans corruption

  Scénario: Sécurité des sessions contre hijacking
    Quand une session est créée pour une adresse IP
    Et que la même session est utilisée depuis une autre IP
    Alors la session doit être invalidée pour sécurité
    Et l'utilisateur doit être déconnecté
    Et un événement de sécurité doit être loggué

  Scénario: Nettoyage des sessions expirées
    Quand 1000 sessions expirent
    Alors le processus de nettoyage doit s'exécuter
    Et les sessions expirées doivent être supprimées du store
    Et l'espace de stockage doit être libéré
    Et les performances doivent rester stables
