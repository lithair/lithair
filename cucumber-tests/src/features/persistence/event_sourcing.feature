Feature: Event Sourcing and Persistence
  As a critical application developer
  I want Lithair to guarantee data integrity
  In order to be able to reconstruct state at any time

  Background:
    Given a Lithair engine with event sourcing enabled
    And events are persisted in "events.raftlog"
    And snapshots are created periodically

  Scenario: Event persistence
    When I perform a CRUD operation
    Then an event must be created and persisted
    And the event must contain all metadata
    And the log file must be updated atomically

  Scenario: State reconstruction
    When I restart the server
    Then all events must be replayed
    And the state must be identical to before restart
    And reconstruction must take less than 5 seconds

  Scenario: Optimized snapshots
    When 1000 events have been created
    Then a snapshot must be generated automatically
    And the snapshot must compress the current state
    And old events must be archived

  Scenario: Event deduplication
    When the same event is received twice
    Then only the first must be applied
    And the duplicate must be ignored silently
    And integrity must be preserved

  Scenario: Recovery after corruption
    When the state file is corrupted
    Then the system must detect the corruption
    And rebuild from the last valid snapshot
    And continue to function normally
