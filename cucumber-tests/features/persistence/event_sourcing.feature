# language: fr
Fonctionnalité: Event Sourcing et Persistance
  En tant que développeur d'applications critiques
  Je veux que Lithair garantisse l'intégrité des données
  Afin de pouvoir reconstruire l'état à tout moment

  @core
  Scénario: Persistance des événements
    Soit un moteur Lithair avec event sourcing activé
    Quand j'effectue une opération CRUD
    Alors un événement doit être créé et persisté
    Et l'événement doit contenir toutes les métadonnées
    Et le fichier de log doit être mis à jour atomiquement

  @core
  Scénario: Reconstruction de l'état
    Quand je redémarre le serveur
    Alors tous les événements doivent être rejoués
    Et l'état doit être identique à avant le redémarrage
    Et la reconstruction doit prendre moins de 5 secondes

  @core
  Scénario: Snapshots optimisés
    Quand 1000 événements ont été créés
    Alors un snapshot doit être généré automatiquement
    Et le snapshot doit compresser l'état actuel
    Et les anciens événements doivent être archivés
    Et la génération du snapshot doit prendre moins de 5 secondes

  @core
  Scénario: Déduplication des événements
    Quand le même événement est reçu deux fois
    Alors seul le premier doit être appliqué
    Et le doublon doit être ignoré silencieusement
    Et l'intégrité doit être préservée

  @core
  Scénario: Déduplication persistante après redémarrage
    Quand un événement idempotent est appliqué avant et après redémarrage du moteur
    Alors le moteur doit rejeter le doublon après redémarrage

  @advanced @multifile
  Scénario: Routage multi-fichiers par agrégat
    Quand je persiste des événements sur plusieurs agrégats dans un event store multi-fichiers
    Alors les événements doivent être répartis par agrégat dans des fichiers distincts
    Et chaque fichier d'agrégat ne doit contenir que les événements de cet agrégat

  @advanced @multifile @dedup
  Scénario: Déduplication persistante en mode multi-fichiers
    Quand un événement idempotent est appliqué avant et après redémarrage du moteur en mode multi-fichiers
    Alors le moteur doit rejeter le doublon après redémarrage
    Et le fichier de déduplication doit être global en mode multi-fichiers

  @advanced @multifile @rotation
  Scénario: Rotation des logs en mode multi-fichiers
    Quand je génère suffisamment d'événements pour provoquer une rotation du log en mode multi-fichiers
    Alors le log de l'agrégat de rotation doit être rotaté
    Et les fichiers de log de cet agrégat doivent rester lisibles après rotation

  @advanced @multifile @relations
  Scénario: Relations dynamiques entre articles et utilisateurs en mode multi-fichiers
    Quand je crée un utilisateur et un article liés en mode multi-fichiers
    Alors les relations dynamiques doivent être reconstruites en mémoire à partir des événements multi-fichiers
    Et les événements doivent être répartis par table de données et par table de relations

  @advanced @versioning
  Scénario: Upcasting d'événements ArticleCreated versionnés
    Quand je rejoue des événements ArticleCreated v1 et v2 via des désérialiseurs versionnés
    Alors l'état des articles doit refléter le schéma courant (slug v2, slug absent en v1)

  @core
  Scénario: Récupération après corruption
    Quand le fichier d'état est corrompu
    Alors le système doit détecter la corruption
    Et reconstruire depuis le dernier snapshot valide
    Et continuer à fonctionner normalement
