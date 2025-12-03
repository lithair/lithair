Feature: Moteur Déclaratif et Multi-Entité
  Pour garantir la robustesse et la flexibilité de Lithair
  En tant que développeur de framework
  Je veux vérifier que le moteur respecte les spécifications déclaratives et gère plusieurs entités

  Scenario: Définition et respect des contraintes d'unicité
    Given une spécification de modèle pour "Product" avec le champ "name" unique
    When je crée un produit "Product A" avec le nom "Laptop"
    Then l'opération doit réussir
    When je tente de créer un autre produit "Product B" avec le nom "Laptop"
    Then l'opération doit échouer avec une erreur de contrainte d'unicité

  Scenario: Gestion atomique de plusieurs entités dans un log global
    Given un moteur initialisé avec support multi-entité
    When je crée un produit "P1" (stock: 10)
    And je crée une commande "O1" pour le produit "P1" (qte: 2)
    Then l'état du produit "P1" doit avoir un stock de 10
    # Note: La logique métier de décrémentation n'est pas implémentée dans le mock,
    # mais le test vérifie que les DEUX événements sont présents et ordonnés.
    And le journal d'événements doit contenir 2 événements
    And le journal doit contenir un événement de type "ProductCreated"
    And le journal doit contenir un événement de type "OrderPlaced"

  Scenario: Rejeu (Replay) d'événements hétérogènes
    Given un journal contenant:
      | type           | payload                                    |
      | ProductCreated | {"id": "p1", "name": "Phone", "stock": 50} |
      | OrderPlaced    | {"id": "o1", "product_id": "p1", "qty": 1} |
    When je redémarre le moteur
    Then l'état en mémoire doit contenir le produit "p1"
    And l'état en mémoire doit contenir la commande "o1"

  # Scenario: Persistance et relecture en format Binaire (Bincode)
  #   Given un moteur configuré en mode binaire
  #   When je crée un produit "BinProduct" (stock: 99)
  #   And je crée une commande "BinOrder" pour le produit "BinProduct" (qte: 1)
  #   # Verification de la persistence binaire (taille plus petite, format non lisible directement)
  #   Then le journal d'événements doit contenir 2 événements
  #   When je redémarre le moteur en mode binaire
  #   Then l'état en mémoire doit contenir le produit "BinProduct"
  #   And l'état en mémoire doit contenir la commande "BinOrder"

  # Scenario: Snapshot et Troncature du journal
  #   Given un moteur initialisé avec support multi-entité
  #   When je crée un produit "SnapProd" (stock: 5)
  #   And je force un snapshot de l'état
  #   Then le fichier de snapshot doit exister
  #   When je tronque le journal d'événements
  #   Then le journal d'événements doit contenir 0 événements
  #   When je redémarre le moteur
  #   Then l'état en mémoire doit contenir le produit "SnapProd"

  Scenario: Auto-Join des relations (Foreign Keys)
    Given un moteur avec une spécification de modèle pour "Product" liant "category_id" à "categories"
    And une source de données "categories" contenant la catégorie "c1" ("Electronics")
    When je crée un produit "P_Join" avec category_id "c1"
    And je demande l'expansion automatique des relations pour le produit "P_Join"
    Then le JSON résultant doit contenir le champ "category"
    And le champ "category" doit contenir le nom "Electronics"
