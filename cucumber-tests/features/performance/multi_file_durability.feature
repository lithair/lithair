# Multi-File Durability Test for Lithair
# Verify that each structure has its own file with CRC32

Feature: Multi-File Durability of Lithair
  As a developer
  I want to verify that Lithair supports multiple structures
  With a separate file per data type and validated CRC32

  Background:
    Given multi-file persistence is enabled

  # ==================== ISOLATION PER STRUCTURE TEST ====================

  @critical @multifile
  Scenario: Each structure has its own file
    Given a multi-file store in "/tmp/lithair-multifile-test"
    When I create 100 "articles" with aggregate_id "articles"
    And I create 50 "users" with aggregate_id "users"
    And I create 75 "products" with aggregate_id "products"
    And I flush all stores
    Then the file "articles/events.raftlog" must exist
    And the file "users/events.raftlog" must exist
    And the file "products/events.raftlog" must exist
    And the file "articles/events.raftlog" must contain exactly 100 lines
    And the file "users/events.raftlog" must contain exactly 50 lines
    And the file "products/events.raftlog" must contain exactly 75 lines

  @critical @multifile @isolation
  Scenario: Data isolation between structures
    Given a multi-file store in "/tmp/lithair-isolation-test"
    When I create 50 "articles" with aggregate_id "articles"
    And I create 30 "users" with aggregate_id "users"
    And I flush all stores
    Then the file "articles/events.raftlog" must contain only "articles" events
    And the file "users/events.raftlog" must contain only "users" events
    And no "users" event must be in "articles/events.raftlog"
    And no "articles" event must be in "users/events.raftlog"

  # ==================== MULTI-FILE CRC32 TEST ====================

  @critical @multifile @crc32
  Scenario: CRC32 validated on all files
    Given a multi-file store in "/tmp/lithair-multifile-crc32"
    When I create 100 "articles" with aggregate_id "articles"
    And I create 100 "users" with aggregate_id "users"
    And I flush all stores
    Then all events in "articles/events.raftlog" must have a valid CRC32
    And all events in "users/events.raftlog" must have a valid CRC32
    And the format of each line must be "<crc32>:<json>"

  @critical @multifile @corruption
  Scenario: Per-file corruption detection
    Given a multi-file store in "/tmp/lithair-corruption-test"
    When I create 50 "articles" with aggregate_id "articles"
    And I flush all stores
    And I deliberately corrupt a line in "articles/events.raftlog"
    Then reading "articles/events.raftlog" must detect 1 corrupted event
    And other files must not be affected

  # ==================== MULTI-FILE RECOVERY TEST ====================

  @critical @multifile @recovery
  Scenario: Recovery after crash with multi-files
    Given a multi-file store in "/tmp/lithair-multifile-crash"
    When I create 200 "articles" with aggregate_id "articles"
    And I create 150 "users" with aggregate_id "users"
    And I create 100 "orders" with aggregate_id "orders"
    And I flush all stores with fsync
    And I simulate a brutal crash
    When I reload the multi-file store from "/tmp/lithair-multifile-crash"
    Then I must recover exactly 200 "articles"
    And I must recover exactly 150 "users"
    And I must recover exactly 100 "orders"
    And all CRC32 must be valid

  # ==================== MULTI-FILE PERFORMANCE TEST ====================

  @performance @multifile
  Scenario: Multi-file write performance
    Given a multi-file store in "/tmp/lithair-multifile-perf"
    When I measure the time to create 1000 events distributed across 5 structures
    And I flush all stores
    Then the total multifile time must be less than 10 seconds
    And each structure must have approximately 200 events
    And all files must exist with valid CRC32

  @performance @multifile @concurrent
  Scenario: Concurrent writes on multiple structures
    Given a multi-file store in "/tmp/lithair-concurrent-test"
    When I launch 5 concurrent tasks each writing 100 events on a different structure
    And I wait for all tasks to complete
    And I flush all stores
    Then each structure must have exactly 100 events
    And no data must be mixed between structures
    And all CRC32 must be valid

  # ==================== GLOBAL STORE TEST ====================

  @multifile @global
  Scenario: Events without aggregate_id go in global
    Given a multi-file store in "/tmp/lithair-global-test"
    When I create 50 events without aggregate_id
    And I create 30 "articles" with aggregate_id "articles"
    And I flush all stores
    Then the file "global/events.raftlog" must contain exactly 50 lines
    And the file "articles/events.raftlog" must contain exactly 30 lines

  # ==================== SELECTIVE READ TEST ====================

  @multifile @read
  Scenario: Selective read by structure
    Given a multi-file store in "/tmp/lithair-selective-read"
    When I create 100 "articles" with aggregate_id "articles"
    And I create 100 "users" with aggregate_id "users"
    And I create 100 "products" with aggregate_id "products"
    And I flush all stores
    When I read only the "articles" structure
    Then I must get exactly 100 events
    And all must be of type "articles"
    When I read all structures
    Then I must get exactly 300 events in total
