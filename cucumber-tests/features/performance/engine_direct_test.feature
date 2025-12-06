@perf
Feature: DIRECT ENGINE TEST - Pure Lithair Performance
  As a developer
  I want to test the Lithair engine DIRECTLY
  Without HTTP overhead to measure true performance

  Background:
    Given the Lithair engine is initialized in MaxDurability mode

  # ==================== 10K TEST - QUICK VALIDATION ====================

  Scenario: 10K articles - Direct engine test
    Given an engine with persistence in "/tmp/lithair-engine-10k"

    # Phase 1: Creation
    When I create 10000 articles directly in the engine
    Then the creation throughput must be greater than 200000 articles/sec

    # Phase 2: Modifications
    When I modify 2000 articles directly in the engine
    Then the modification throughput must be greater than 200000 articles/sec

    # Phase 3: Deletions
    When I delete 1000 articles directly in the engine
    Then the deletion throughput must be greater than 200000 articles/sec

    # Phase 4: Flush and verifications
    And I wait for complete engine flush
    Then the events.raftlog file must exist
    And the events.raftlog file must contain exactly 13000 events
    And the engine must have 9000 articles in memory
    And all events must be persisted

  # ==================== 100K TEST - SCALE UP ====================

  Scenario: 100K articles - Direct scale up test
    Given an engine with persistence in "/tmp/lithair-engine-100k"

    When I create 100000 articles directly in the engine
    Then the creation throughput must be greater than 200000 articles/sec

    When I modify 20000 articles directly in the engine
    Then the modification throughput must be greater than 200000 articles/sec

    When I delete 10000 articles directly in the engine
    And I wait for complete engine flush

    Then the events.raftlog file must contain exactly 130000 events
    And the engine must have 90000 articles in memory

  # ==================== 1M TEST - ULTIMATE STRESS ====================

  @stress
  Scenario: 1 MILLION articles - Ultimate direct stress test
    Given an engine with persistence in "/tmp/lithair-engine-1m"

    # Phase 1: Massive creation
    When I create 1000000 articles directly in the engine
    Then the creation throughput must be greater than 300000 articles/sec
    And the creation time must be less than 5 seconds

    # Phase 2: Modifications
    When I modify 200000 articles directly in the engine
    Then the modification throughput must be greater than 200000 articles/sec

    # Phase 3: Deletions
    When I delete 100000 articles directly in the engine
    Then the deletion throughput must be greater than 200000 articles/sec

    # Phase 4: Complete verifications
    And I wait for complete engine flush
    Then the events.raftlog file must exist
    And the events.raftlog file must contain exactly 1300000 events
    And the engine must have 900000 articles in memory
    And the events.raftlog file size must be approximately 85 MB
    And all events must be in chronological order
    And no event must be missing

  # ==================== CONSISTENCY TEST ====================

  Scenario: Memory/disk consistency verification with 50K articles
    Given an engine with persistence in "/tmp/lithair-engine-coherence"

    When I create 50000 articles directly in the engine
    And I modify 10000 articles directly in the engine
    And I delete 5000 articles directly in the engine
    And I wait for complete engine flush

    Then the number of articles in memory must equal the number reconstructed from disk
    And all checksums must match between memory and disk
