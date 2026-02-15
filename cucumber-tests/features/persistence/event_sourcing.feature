Feature: Event Sourcing and Persistence
  As a developer of critical applications
  I want Lithair to guarantee data integrity
  In order to be able to reconstruct state at any time

  @core
  Scenario: Event persistence
    Given a Lithair engine with event sourcing enabled
    When I perform a CRUD operation
    Then an event should be created and persisted
    And the event should contain all metadata
    And the log file should be updated atomically

  @core
  Scenario: State reconstruction
    When I restart the server
    Then all events should be replayed
    And state should be identical to before the restart
    And reconstruction should take less than 5 seconds

  @core
  Scenario: Optimized snapshots
    When 1000 events have been created
    Then a snapshot should be generated automatically
    And the snapshot should compress current state
    And old events should be archived
    And snapshot generation should take less than 5 seconds

  @core
  Scenario: Event deduplication
    When the same event is received twice
    Then only the first should be applied
    And the duplicate should be ignored silently
    And integrity should be preserved
