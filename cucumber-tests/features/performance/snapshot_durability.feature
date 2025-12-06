# Snapshot Durability Test for Lithair
# Verify that snapshots accelerate recovery

Feature: Snapshot Durability of Lithair
  As a developer
  I want to verify that Lithair supports snapshots
  To accelerate startup and recovery after crash

  Background:
    Given multi-file persistence is enabled

  # ==================== SNAPSHOT CREATION TEST ====================

  @critical @snapshot
  Scenario: Snapshot creation for an aggregate
    Given a multi-file store in "/tmp/lithair-snapshot-basic"
    When I create 100 "articles" with aggregate_id "articles"
    And I flush all stores
    And I create a snapshot for "articles" with state '{"count": 100}'
    Then the snapshot for "articles" must exist
    And the snapshot for "articles" must contain 100 events
    And the snapshot for "articles" must have a valid CRC32

  @critical @snapshot
  Scenario: Global snapshot creation
    Given a multi-file store in "/tmp/lithair-snapshot-global"
    When I create 50 events without aggregate_id
    And I flush all stores
    And I create a global snapshot with state '{"global_count": 50}'
    Then the global snapshot must exist
    And the global snapshot must contain 50 events

  # ==================== RECOVERY WITH SNAPSHOT TEST ====================

  @critical @snapshot @recovery
  Scenario: Recovery with snapshot - fewer events to replay
    Given a multi-file store in "/tmp/lithair-snapshot-recovery"
    When I create 1000 "articles" with aggregate_id "articles"
    And I flush all stores
    And I create a snapshot for "articles" with state '{"processed": 1000}'
    And I create 100 "articles" with aggregate_id "articles"
    And I flush all stores
    Then the total number of events for "articles" must be 1100
    When I recover events after snapshot for "articles"
    Then I must get exactly 100 events
    And all these events must have a valid CRC32

  @critical @snapshot @recovery
  Scenario: Recovery without snapshot - all events
    Given a multi-file store in "/tmp/lithair-no-snapshot"
    When I create 500 "users" with aggregate_id "users"
    And I flush all stores
    When I recover events after snapshot for "users"
    Then I must get exactly 500 events

  # ==================== SNAPSHOT INTEGRITY TEST ====================

  @critical @snapshot @integrity
  Scenario: Snapshot corruption detection
    Given a multi-file store in "/tmp/lithair-snapshot-corrupt"
    When I create 100 "articles" with aggregate_id "articles"
    And I flush all stores
    And I create a snapshot for "articles" with state '{"data": "test"}'
    And I corrupt the snapshot file for "articles"
    Then loading the snapshot for "articles" must fail with corruption error

  @snapshot @integrity
  Scenario: Snapshot with complex state
    Given a multi-file store in "/tmp/lithair-snapshot-complex"
    When I create 200 "products" with aggregate_id "products"
    And I flush all stores
    And I create a snapshot for "products" with complex state
    Then the snapshot for "products" must exist
    And loading the snapshot for "products" must succeed
    And the recovered state must be identical to the saved state

  # ==================== SNAPSHOT PERFORMANCE TEST ====================

  @performance @snapshot
  Scenario: Performance gain measurement with snapshots
    Given a multi-file store in "/tmp/lithair-snapshot-perf"
    When I create 10000 "articles" with aggregate_id "articles"
    And I flush all stores
    And I measure the time to read all "articles" events
    And I create a snapshot for "articles" with state '{"count": 10000}'
    And I create 100 "articles" with aggregate_id "articles"
    And I flush all stores
    And I measure the time to read after snapshot for "articles"
    Then the time with snapshot must be at least 10x faster

  @performance @snapshot @threshold
  Scenario: Automatic snapshot creation threshold
    Given a multi-file store with snapshot threshold at 500 in "/tmp/lithair-snapshot-threshold"
    When I create 400 "logs" with aggregate_id "logs"
    And I flush all stores
    Then a snapshot for "logs" should not be necessary
    When I create 200 "logs" with aggregate_id "logs"
    And I flush all stores
    Then a snapshot for "logs" should be necessary

  # ==================== MULTI-AGGREGATE SNAPSHOTS TEST ====================

  @snapshot @multi
  Scenario: Independent snapshots per aggregate
    Given a multi-file store in "/tmp/lithair-snapshot-multi"
    When I create 100 "articles" with aggregate_id "articles"
    And I create 200 "users" with aggregate_id "users"
    And I create 150 "products" with aggregate_id "products"
    And I flush all stores
    And I create a snapshot for "articles" with state '{"articles": 100}'
    And I create a snapshot for "users" with state '{"users": 200}'
    Then the snapshot for "articles" must contain 100 events
    And the snapshot for "users" must contain 200 events
    And the snapshot for "products" must not exist
    And the snapshot list must contain 2 entries

  @snapshot @multi @delete
  Scenario: Snapshot deletion
    Given a multi-file store in "/tmp/lithair-snapshot-delete"
    When I create 50 "temp" with aggregate_id "temp"
    And I flush all stores
    And I create a snapshot for "temp" with state '{"temp": true}'
    Then the snapshot for "temp" must exist
    When I delete the snapshot for "temp"
    Then the snapshot for "temp" must not exist
