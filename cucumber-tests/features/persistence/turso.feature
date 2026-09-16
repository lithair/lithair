Feature: Optional SQL storage alongside native Lithair models
  Each model has one authoritative store. SQL models retain validation and
  permission hooks, while native event history remains a native capability.

  Scenario: Committed SQL data survives reopening the database
    Given an isolated Turso model store
    When I create a valid SQL document
    And I reopen the Turso database
    Then the SQL document is readable

  Scenario: A rejected batch leaves no partial data
    Given an isolated Turso model store
    When I submit a SQL batch with a duplicate primary key
    Then the SQL batch is rejected without partial data

  Scenario: Validation and permissions still apply to SQL models
    Given an isolated Turso model store
    Then invalid SQL documents are rejected
    And unauthorized SQL reads and writes are denied

  Scenario: Model declarations generate native and SQL routes in one application
    Then native and SQL HTTP models coexist and survive server restart

  Scenario: SQL clients can discover all 55 documents using pagination metadata
    Then the SQL HTTP list provides a continuation until all 55 documents are read

  @turso-migrations
  Scenario: Declared schema migrations preserve data across restarts
    Given SQL documents stored by the first application version
    When the application upgrades its declared SQL model
    Then the migrated documents and schema survive another restart

  @turso-migrations
  Scenario: Failed schema migrations preserve the previous application data
    Given SQL documents stored by the first application version
    When a declared SQL migration fails
    Then the previous SQL model can still read every original document
