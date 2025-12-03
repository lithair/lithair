# language: fr
Fonctionnalité: Event Sourcing et Persistance
  En tant que développeur d'applications critiques
  Je veux que Lithair garantisse l'intégrité des données
  Afin de pouvoir reconstruire l'état à tout moment

  Contexte:
    Soit un moteur Lithair avec event sourcing activé
    Et que les événements soient persistés dans "events.raftlog"
    Et que les snapshots soient créés périodiquement

  Scénario: Persistance des événements
    Quand j'effectue une opération CRUD
    Alors un événement doit être créé et persisté
    Et l'événement doit contenir toutes les métadonnées
    Et le fichier de log doit être mis à jour atomiquement

  Scénario: Reconstruction de l'état
    Quand je redémarre le serveur
    Alors tous les événements doivent être rejoués
    Et l'état doit être identique à avant le redémarrage
    Et la reconstruction doit prendre moins de 5 secondes

  Scénario: Snapshots optimisés
    Quand 1000 événements ont été créés
    Alors un snapshot doit être généré automatiquement
    Et le snapshot doit compresser l'état actuel
    Et les anciens événements doivent être archivés

  Scénario: Déduplication des événements
    Quand le même événement est reçu deux fois
    Alors seul le premier doit être appliqué
    Et le doublon doit être ignoré silencieusement
    Et l'intégrité doit être préservée

  Scénario: Récupération après corruption
    Quand le fichier d'état est corrompu
    Alors le système doit détecter la corruption
    Et reconstruire depuis le dernier snapshot valide
    Et continuer à fonctionner normalement
