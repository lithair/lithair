# Lithair Durability Test
# Verify that MaxDurability guarantees ZERO data loss

Feature: Durability and Data Persistence of Lithair
  As a developer
  I want to verify that Lithair in MaxDurability mode
  Guarantees ZERO data loss

  Background:
    Given persistence is enabled by default

  # ==================== DURABILITY TEST ====================

  Scenario: MaxDurability mode - ZERO loss guarantee on 1000 articles
    Given a Lithair server on port 20100 with persistence "/tmp/lithair-durability-test"
    When I create 1000 articles quickly
    And I wait 3 seconds for flush
    Then the events.raftlog file must exist
    And the events.raftlog file must contain exactly 1000 "ArticleCreated" events
    And no event must be missing
    And the event checksum must be valid

  Scenario: Performance verification with MaxDurability
    Given a Lithair server on port 20101 with persistence "/tmp/lithair-perf-durable"
    When I measure the time to create 500 articles
    And I wait 2 seconds for flush
    Then the total time must be less than 5 seconds
    And all 500 events must be persisted
    And the events.raftlog file must exist

  Scenario: Memory vs Disk consistency with MaxDurability
    Given a Lithair server on port 20102 with persistence "/tmp/lithair-consistency"
    When I create 100 articles quickly
    And I wait 2 seconds for flush
    Then the number of articles in memory must equal the number on disk
    And all checksums must match

  Scenario: Complete CRUD with durability verification
    Given a Lithair server on port 20103 with persistence "/tmp/lithair-crud-durable"
    When I create 50 articles quickly
    And I modify 25 existing articles
    And I delete 10 articles
    And I wait 3 seconds for flush
    Then the events.raftlog file must contain exactly 50 "ArticleCreated" events
    And the events.raftlog file must contain exactly 25 "ArticleUpdated" events
    And the events.raftlog file must contain exactly 10 "ArticleDeleted" events
    And the final state must have 40 active articles

  # ==================== CRITICAL FSYNC TEST ====================

  @critical @fsync
  Scenario: Fsync guarantees immediate persistence to disk
    # This test verifies that fsync actually writes data to physical disk
    # and not just to the OS buffer
    Given a Lithair server on port 20104 with persistence "/tmp/lithair-fsync-test"
    And MaxDurability mode is enabled with fsync
    When I create 100 critical articles
    And I force an immediate flush with fsync
    Then the 100 articles must be readable from the file immediately
    And the file must not be empty
    # Simulate a "crash" by reading the file directly without cache
    When I read the file directly with O_DIRECT if available
    Then the data must be present on the physical disk

  @critical @crash-recovery
  Scenario: Recovery after brutal crash with MaxDurability
    # Verify that after a brutal crash (no clean shutdown),
    # data flushed with fsync is recoverable
    Given a Lithair server on port 20105 with persistence "/tmp/lithair-crash-test"
    And MaxDurability mode is enabled with fsync
    When I create 500 critical articles
    And I force an immediate flush with fsync
    And I simulate a brutal server crash without shutdown
    # Restart and verify
    When I restart the server from "/tmp/lithair-crash-test"
    Then the 500 articles must be present after recovery
    And no flushed data must be lost
