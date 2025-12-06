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

  @core
  Scenario: Persistent deduplication after restart
    When an idempotent event is applied before and after engine restart
    Then the engine should reject the duplicate after restart

  @advanced @multifile
  Scenario: Multi-file routing by aggregate
    When I persist events on multiple aggregates in a multi-file event store
    Then events should be distributed by aggregate into distinct files
    And each aggregate file should contain only events for that aggregate

  @advanced @multifile @dedup
  Scenario: Persistent deduplication in multi-file mode
    When an idempotent event is applied before and after engine restart in multi-file mode
    Then the engine should reject the duplicate after restart
    And the deduplication file should be global in multi-file mode

  @advanced @multifile @rotation
  Scenario: Log rotation in multi-file mode
    When I generate enough events to trigger log rotation in multi-file mode
    Then the rotation aggregate log should be rotated
    And log files for that aggregate should remain readable after rotation

  @advanced @multifile @relations
  Scenario: Dynamic relations between articles and users in multi-file mode
    When I create a linked user and article in multi-file mode
    Then dynamic relations should be reconstructed in memory from multi-file events
    And events should be distributed by data table and relation table

  @advanced @versioning
  Scenario: Upcasting of versioned ArticleCreated events
    When I replay ArticleCreated v1 and v2 events via versioned deserializers
    Then article state should reflect current schema (slug v2, slug absent in v1)

  @core
  Scenario: Recovery after corruption
    When the state file is corrupted
    Then the system should detect corruption
    And rebuild from last valid snapshot
    And continue to function normally
