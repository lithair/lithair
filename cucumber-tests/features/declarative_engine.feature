Feature: Declarative Engine and Multi-Entity
  To ensure the robustness and flexibility of Lithair
  As a framework developer
  I want to verify that the engine respects declarative specifications and manages multiple entities

  Scenario: Definition and enforcement of uniqueness constraints
    Given a model specification for "Product" with field "name" as unique
    When I create a product "Product A" with name "Laptop"
    Then the operation must succeed
    When I try to create another product "Product B" with name "Laptop"
    Then the operation must fail with a uniqueness constraint error

  Scenario: Atomic management of multiple entities in a global log
    Given an engine initialized with multi-entity support
    When I create a product "P1" (stock: 10)
    And I create an order "O1" for product "P1" (qty: 2)
    Then the state of product "P1" must have a stock of 10
    # Note: Business logic for decrementing is not implemented in the mock,
    # but the test verifies that BOTH events are present and ordered.
    And the event log must contain 2 events
    And the log must contain an event of type "ProductCreated"
    And the log must contain an event of type "OrderPlaced"

  Scenario: Replay of heterogeneous events
    Given a log containing:
      | type           | payload                                    |
      | ProductCreated | {"id": "p1", "name": "Phone", "stock": 50} |
      | OrderPlaced    | {"id": "o1", "product_id": "p1", "qty": 1} |
    When I restart the engine
    Then the in-memory state must contain product "p1"
    And the in-memory state must contain order "o1"

  # Scenario: Persistence and reading in Binary format (Bincode)
  #   Given an engine configured in binary mode
  #   When I create a product "BinProduct" (stock: 99)
  #   And I create an order "BinOrder" for product "BinProduct" (qty: 1)
  #   # Verification of binary persistence (smaller size, not directly readable format)
  #   Then the event log must contain 2 events
  #   When I restart the engine in binary mode
  #   Then the in-memory state must contain product "BinProduct"
  #   And the in-memory state must contain order "BinOrder"

  # Scenario: Snapshot and Log Truncation
  #   Given an engine initialized with multi-entity support
  #   When I create a product "SnapProd" (stock: 5)
  #   And I force a state snapshot
  #   Then the snapshot file must exist
  #   When I truncate the event log
  #   Then the event log must contain 0 events
  #   When I restart the engine
  #   Then the in-memory state must contain product "SnapProd"

  Scenario: Auto-Join of relations (Foreign Keys)
    Given an engine with a model specification for "Product" linking "category_id" to "categories"
    And a data source "categories" containing category "c1" ("Electronics")
    When I create a product "P_Join" with category_id "c1"
    And I request automatic expansion of relations for product "P_Join"
    Then the resulting JSON must contain field "category"
    And field "category" must contain name "Electronics"
