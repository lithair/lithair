# language: fr
# Dual-Mode Serialization - JSON (simd-json) + Binary (rkyv)
# Test des deux modes de sérialisation pour Lithair

Fonctionnalité: Sérialisation dual-mode JSON et rkyv
  En tant que développeur
  Je veux pouvoir utiliser JSON ou rkyv pour la sérialisation
  Afin d'optimiser les performances selon le contexte d'utilisation

  Contexte:
    Soit un type de test "Article" avec les champs id, title, price

  # ==================== JSON MODE (simd-json) ====================

  @serialization @json
  Plan du Scénario: Sérialisation JSON roundtrip
    Soit un article avec id "<id>" titre "<titre>" et prix <prix>
    Quand je sérialise l'article en mode JSON
    Et je désérialise les données JSON
    Alors l'article désérialisé doit avoir id "<id>"
    Et l'article désérialisé doit avoir titre "<titre>"
    Et l'article désérialisé doit avoir prix <prix>

    Exemples:
      | id          | titre                   | prix  |
      | art-001     | Premier article         | 19.99 |
      | art-002     | Article avec accents éè | 29.50 |
      | art-003     | Article unicode 日本語   | 99.00 |

  @serialization @json @benchmark
  Scénario: Performance sérialisation JSON
    Soit 1000 articles générés aléatoirement
    Quand je mesure le temps pour sérialiser les 1000 articles en JSON
    Et je mesure le temps pour désérialiser les 1000 articles JSON
    Alors le throughput JSON serialize doit être supérieur à 10 MB/s
    Et le throughput JSON deserialize doit être supérieur à 100 MB/s

  @serialization @json @simd
  Scénario: Vérification utilisation simd-json pour parsing
    Soit des données JSON valides représentant un article
    Quand je désérialise avec simd-json
    Alors le parsing doit utiliser les instructions SIMD si disponibles
    Et le résultat doit être identique à serde_json

  # ==================== BINARY MODE (rkyv) ====================

  @serialization @rkyv
  Plan du Scénario: Sérialisation rkyv roundtrip
    Soit un article avec id "<id>" titre "<titre>" et prix <prix>
    Quand je sérialise l'article en mode rkyv
    Et je désérialise les données rkyv
    Alors l'article désérialisé doit avoir id "<id>"
    Et l'article désérialisé doit avoir titre "<titre>"
    Et l'article désérialisé doit avoir prix <prix>

    Exemples:
      | id          | titre                   | prix  |
      | art-001     | Premier article         | 19.99 |
      | art-002     | Deuxieme article test   | 29.50 |
      | art-003     | Troisieme article test  | 99.00 |

  @serialization @rkyv @benchmark
  Scénario: Performance sérialisation rkyv
    Soit 1000 articles générés aléatoirement
    Quand je mesure le temps pour sérialiser les 1000 articles en rkyv
    Et je mesure le temps pour désérialiser les 1000 articles rkyv
    Alors le throughput rkyv serialize doit être supérieur à 500 MB/s
    Et le throughput rkyv deserialize doit être supérieur à 1000 MB/s

  @serialization @rkyv @zero-copy
  Scénario: Accès zero-copy avec rkyv
    Soit un article sérialisé en rkyv
    Quand j'accède aux données en mode zero-copy
    Alors aucune allocation mémoire ne doit être effectuée
    Et je dois pouvoir lire le titre sans désérialiser

  # ==================== COMPARISON JSON vs RKYV ====================

  @serialization @comparison
  Scénario: Comparaison taille des données
    Soit un article avec id "test-size" titre "Test de taille comparative" et prix 42.50
    Quand je sérialise en JSON
    Et je sérialise en rkyv
    Alors la taille rkyv doit être inférieure ou égale à la taille JSON

  @serialization @comparison @benchmark
  Scénario: Benchmark comparatif JSON vs rkyv
    Soit 10000 articles générés aléatoirement
    Quand je benchmark la sérialisation JSON sur 10000 articles
    Et je benchmark la sérialisation rkyv sur 10000 articles
    Alors rkyv serialize doit être au moins 5x plus rapide que JSON serialize
    Et rkyv deserialize doit être au moins 3x plus rapide que JSON deserialize

  # ==================== MODE SELECTION ====================

  @serialization @mode-selection
  Plan du Scénario: Sélection du mode via Accept header
    Quand je reçois un header Accept "<accept>"
    Alors le mode sélectionné doit être "<mode>"

    Exemples:
      | accept                      | mode   |
      | application/json            | Json   |
      | application/octet-stream    | Binary |
      | application/x-rkyv          | Binary |
      | text/html                   | Json   |
      | */*                         | Json   |

  @serialization @content-type
  Plan du Scénario: Content-Type selon le mode
    Soit le mode de sérialisation "<mode>"
    Alors le content-type doit être "<content_type>"

    Exemples:
      | mode   | content_type             |
      | Json   | application/json         |
      | Binary | application/octet-stream |

  # ==================== ERROR HANDLING ====================

  @serialization @errors @json
  Scénario: Gestion erreur JSON invalide
    Soit des données JSON malformées "{ invalid json"
    Quand je tente de désérialiser en JSON
    Alors une erreur JsonDeserializeError doit être retournée
    Et le message doit indiquer la position de l'erreur

  @serialization @errors @rkyv
  Scénario: Gestion erreur rkyv données corrompues
    Soit des données binaires aléatoires de 100 bytes
    Quand je tente de désérialiser en rkyv
    Alors une erreur RkyvDeserializeError ou RkyvValidationError doit être retournée

  # ==================== INTEGRATION HTTP ====================

  @serialization @http @json
  Scénario: Requête HTTP avec JSON
    Soit un serveur Lithair sur le port 22000
    Quand j'envoie une requête POST avec Content-Type "application/json"
    Et le corps contient un article en JSON
    Alors la réponse doit être en JSON
    Et le Content-Type de réponse doit être "application/json"

  @serialization @http @rkyv
  Scénario: Requête HTTP avec rkyv
    Soit un serveur Lithair sur le port 22001
    Quand j'envoie une requête POST avec Content-Type "application/octet-stream"
    Et le corps contient un article en rkyv
    Et le header Accept est "application/octet-stream"
    Alors la réponse doit être en format binaire rkyv
    Et le Content-Type de réponse doit être "application/octet-stream"

  @serialization @http @negotiation
  Scénario: Négociation de contenu automatique
    Soit un serveur Lithair sur le port 22002
    Quand j'envoie une requête avec Accept "application/octet-stream, application/json;q=0.5"
    Alors le serveur doit répondre en rkyv (priorité plus haute)
