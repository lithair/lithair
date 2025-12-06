# Stress Test 1 Million Articles - Lithair
# Performance + consistency + durability verification at large scale

Feature: STRESS TEST 1M - Mixed CRUD with Integrity Verification
  As a developer
  I want to verify that Lithair can handle 1 million articles
  With mixed CRUD operations and guarantee data integrity

  Background:
    Given persistence is enabled by default

  # ==================== QUICK VALIDATION ====================

  Scenario: 10K articles - Quick architecture validation
    Given a Lithair server on port 20200 with persistence "/tmp/lithair-stress-10k"

    # Phase 1: Creation
    When I create 10000 articles quickly
    Then I measure the creation throughput

    # Phase 2: Modifications
    When I modify 2000 existing articles
    Then I measure the modification throughput

    # Phase 3: Deletions
    When I delete 1000 articles
    Then I measure the deletion throughput

    # Phase 4: Wait for flush
    And I wait 3 seconds for flush

    # Phase 5: Verifications
    Then the events.raftlog file must exist
    And the events.raftlog file must contain exactly 10000 "ArticleCreated" events
    And the events.raftlog file must contain exactly 2000 "ArticleUpdated" events
    And the events.raftlog file must contain exactly 1000 "ArticleDeleted" events
    And the final state must have 9000 active articles
    And the number of articles in memory must equal the number on disk

    # Phase 6: Metrics
    And I stop the server cleanly
    And I display the final statistics

  # ==================== ULTIMATE STRESS TEST ====================

  Scenario: 1 MILLION articles - Mixed CRUD with complete verification
    Given a Lithair server on port 20200 with persistence "/tmp/lithair-stress-1m"

    # Phase 1: Massive creation
    When I create 1000000 articles quickly
    Then I measure the creation throughput

    # Phase 2: Modifications on subset
    When I modify 200000 existing articles
    Then I measure the modification throughput

    # Phase 3: Deletions on subset
    When I delete 100000 articles
    Then I measure the deletion throughput

    # Phase 4: Wait for complete flush
    And I wait 5 seconds for flush

    # Phase 5: Integrity verifications
    Then the events.raftlog file must exist
    And the events.raftlog file must contain exactly 1000000 "ArticleCreated" events
    And the events.raftlog file must contain exactly 200000 "ArticleUpdated" events
    And the events.raftlog file must contain exactly 100000 "ArticleDeleted" events
    And the final state must have 900000 active articles
    And all events must be in chronological order
    And the number of articles in memory must equal the number on disk
    And all checksums must match

    # Phase 6: Final metrics
    And I stop the server cleanly
    And I display the final statistics

  # ==================== PURE PERFORMANCE TEST ====================

  Scenario: 500K articles - Maximum performance in Performance mode
    Given a Lithair server on port 20201 with persistence "/tmp/lithair-perf-500k"
    And the durability mode is "Performance"

    When I create 500000 articles quickly
    Then the throughput must be greater than 20000 articles/sec
    And the total time must be less than 30 seconds

    When I delete 100000 articles
    Then the deletion throughput must be greater than 15000 articles/sec

  # ==================== CONSISTENCY UNDER LOAD TEST ====================

  Scenario: 100K articles - Guaranteed consistency with MaxDurability
    Given a Lithair server on port 20202 with persistence "/tmp/lithair-coherence-100k"
    And the durability mode is "MaxDurability"

    When I create 100000 articles quickly
    And I modify 50000 existing articles
    And I delete 25000 articles
    And I wait 3 seconds for flush

    Then the final state must have 75000 active articles
    And the events.raftlog file must contain exactly 100000 "ArticleCreated" events
    And the events.raftlog file must contain exactly 50000 "ArticleUpdated" events
    And the events.raftlog file must contain exactly 25000 "ArticleDeleted" events
    And the number of articles in memory must equal the number on disk
    And no event must be missing

  # ==================== RESILIENCE TEST ====================

  Scenario: Resilience - 10K random mixed operations
    Given a Lithair server on port 20203 with persistence "/tmp/lithair-resilience"

    When I run 10000 random CRUD operations
    And I wait 2 seconds for flush

    Then all events must be persisted
    And the number of articles in memory must equal the number on disk
    And data consistency must be validated
