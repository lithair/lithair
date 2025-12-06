# Lithair reliability tests - Recovery, Corruption, Concurrency
# Validates engine robustness in real-world conditions

Feature: ENGINE RELIABILITY TEST - Recovery & Durability
  As a developer
  I want to validate the reliability of the Lithair engine
  In crash, corruption and concurrency scenarios

  Background:
    Given the Lithair engine is initialized in MaxDurability mode

  # ==================== RECOVERY AFTER CRASH TEST ====================

  @core
  Scenario: Recovery - Recovery after simulated crash
    Given an engine with persistence in "/tmp/lithair-recovery-test"

    # Phase 1: Write data
    When I create 10000 articles directly in the engine
    And I modify 2000 articles directly in the engine
    And I wait for complete engine flush
    Then the events.raftlog file must contain exactly 12000 events

    # Phase 2: Simulate crash (brutal stop without shutdown)
    When I simulate an engine crash

    # Phase 3: Restart and recovery
    When I restart the engine from "/tmp/lithair-recovery-test"
    And I reload all events from disk

    # Phase 4: Post-recovery verifications
    Then the engine must have 10000 articles in memory after recovery
    And all articles must be identical to the pre-crash state
    And the events.raftlog file must contain exactly 12000 events
    And no data must be lost

    # Phase 5: Continue after recovery
    When I create 1000 additional articles after recovery
    And I wait for complete engine flush
    Then the events.raftlog file must contain exactly 13000 events
    And the engine must have 11000 articles in memory

  # ==================== FILE CORRUPTION TEST ====================

  @core
  Scenario: Corruption - Corrupted file detection
    Given an engine with persistence in "/tmp/lithair-corruption-test"

    # Phase 1: Create valid data
    When I create 5000 articles directly in the engine
    And I wait for complete engine flush
    Then the events.raftlog file must contain exactly 5000 events

    # Phase 2: Corrupt the file (truncate)
    When I truncate the events.raftlog file to 50% of its size

    # Phase 3: Recovery attempt with corrupted file
    When I restart the engine from "/tmp/lithair-corruption-test"
    And I try to reload events from disk

    # Phase 4: Verifications
    Then the engine must detect the corruption
    And the engine must load only valid events
    And the number of loaded articles must be less than 5000
    And no panic must occur

  # ==================== CONCURRENCY TEST ====================

  @core
  Scenario: Concurrency - Parallel writes with SCC2
    Given an engine with persistence in "/tmp/lithair-concurrency-test"

    # Phase 1: Reference sequential writes
    When I create 1000 articles directly in the engine
    And I wait for complete engine flush
    Then the events.raftlog file must contain exactly 1000 events

    # Phase 2: Parallel writes (10 threads)
    When I launch 10 threads that each create 1000 articles in parallel
    And I wait for all threads to complete
    And I wait for complete engine flush

    # Phase 3: Integrity verifications
    Then the engine must have 11000 articles in memory
    And the events.raftlog file must contain exactly 11000 events
    And no article must be duplicated
    And no article must be lost
    And all IDs must be unique
    And the events.raftlog file must not be corrupted

  @core
  Scenario: Deduplication in concurrency - Same event re-emitted
    When I launch 10 threads that each re-emit the same idempotent event 100 times
    Then the idempotent event must be applied only once in presence of concurrency
    And the deduplication file must contain exactly 1 identifier for this event

  # ==================== FSYNC DURABILITY TEST ====================

  @core
  Scenario: Durability - MaxDurability fsync validation
    Given an engine with persistence in "/tmp/lithair-durability-test"

    # Phase 1: Write with MaxDurability
    When I create 1000 articles directly in the engine
    And I force an immediate fsync

    # Phase 2: Immediate verification on disk
    Then the 1000 articles must be readable from the file
    And the events.raftlog file must not be empty
    And the file size must match the written data

    # Phase 3: Immediate crash after write
    When I simulate a crash immediately after write
    And I restart the engine from "/tmp/lithair-durability-test"
    And I reload all events from disk

    # Phase 4: Zero loss validation
    Then the engine must have 1000 articles in memory after recovery
    And no data must be lost despite the immediate crash

  # ==================== LONG-DURATION STRESS TEST ====================

  @stress
  Scenario: Stress - Long-duration stability (1 minute)
    Given an engine with persistence in "/tmp/lithair-stress-longue-duree"

    # Phase 1: Continuous injection for 60 seconds
    When I run a continuous injection of articles for 60 seconds
    And I measure the average throughput over the period

    # Phase 2: Stability verifications
    Then the average throughput must remain greater than 200000 articles/sec
    And the throughput must not degrade by more than 10% over the period
    And no memory leak must be detected
    And the engine must remain responsive

    # Phase 3: Post-stress verifications
    And I wait for complete engine flush
    Then all events must be persisted
    And the events.raftlog file must not be corrupted
    And the engine must be able to restart correctly
