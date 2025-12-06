# Stress Test 1 Million Events with Snapshots
# Validation of snapshot system performance and reliability at large scale

Feature: STRESS TEST - Snapshots with massive volumes
  As a developer
  I want to verify that snapshots work correctly at large scale
  To guarantee acceptable recovery times even with millions of events

  Background:
    Given multi-file persistence is enabled

  # ==================== QUICK VALIDATION 10K ====================

  @quick
  Scenario: 10K events - Quick snapshot validation
    Given a multi-file store with snapshot threshold at 1000 in "/tmp/lithair-stress-snap-10k"

    # Phase 1: Massive creation
    When I create 10000 "Article" with aggregate_id "articles"
    And I flush all stores

    # Phase 2: Snapshot creation
    When I create a snapshot for "articles" with complex state of 10000 elements
    Then the snapshot for "articles" must exist
    And the snapshot for "articles" must have a valid CRC32
    And the snapshot for "articles" must contain 10000 events

    # Phase 3: Add additional events
    When I create 100 "Article" with aggregate_id "articles"
    And I flush all stores

    # Phase 4: Events to replay validation
    When I recover events after snapshot for "articles"
    Then the number of events to replay must be 100

    # Phase 5: Integrity validation
    And all CRC32 must be valid
    And the total number of events for "articles" must be 10100

  # ==================== 100K EVENTS TEST ====================

  @medium
  Scenario: 100K events - Snapshot performance with moderate load
    Given a multi-file store with snapshot threshold at 10000 in "/tmp/lithair-stress-snap-100k"

    # Phase 1: Create 100K events
    When I create 100000 events with measured throughput for "orders"
    Then the creation throughput must be greater than 100 evt/s

    # Phase 2: Snapshot after 100K
    When I create a snapshot for "orders" with state of 100000 elements
    And I flush all stores
    Then the snapshot for "orders" must exist
    And the snapshot for "orders" must contain 100000 events

    # Phase 3: Add 1000 post-snapshot events
    When I create 1000 "Order" with aggregate_id "orders"
    And I flush all stores

    # Phase 4: Recovery validation after crash
    When I simulate a brutal crash
    And I reload the multi-file store from "/tmp/lithair-stress-snap-100k"

    # Phase 5: Events to replay validation
    When I recover events after snapshot for "orders"
    Then the number of events to replay must be 1000
    And the total number of events for "orders" must be 101000
    And all CRC32 must be valid

  # ==================== 500K EVENTS TEST ====================

  @large
  Scenario: 500K events - High performance stress test
    Given a multi-file store with snapshot threshold at 50000 in "/tmp/lithair-stress-snap-500k"

    # Phase 1: Creation by batch of 100K
    When I create 500000 events in batches of 100000 for "products"
    Then the total creation time must be less than 60 seconds

    # Phase 2: Snapshot creation
    When I create a snapshot for "products" with state of 500000 elements
    And I flush all stores with fsync

    # Phase 3: Snapshot validation
    Then the snapshot for "products" must exist
    And the snapshot for "products" must have a valid CRC32

    # Phase 4: Post-snapshot events
    When I create 5000 "Product" with aggregate_id "products"
    And I flush all stores

    # Phase 5: Recovery test
    When I simulate a brutal crash
    And I reload the multi-file store from "/tmp/lithair-stress-snap-500k"

    # Phase 6: Performance
    When I measure the complete recovery time for "products"
    And I measure the recovery time after snapshot for "products"
    Then recovery with snapshot must be at least 80x faster

  # ==================== STRESS TEST 1 MILLION ====================

  @stress @1m
  Scenario: 1 MILLION events - Ultimate snapshot validation
    Given a multi-file store with snapshot threshold at 100000 in "/tmp/lithair-stress-snap-1m"

    # Phase 1: Create 1M events
    When I create 1000000 events in batches of 100000 for "transactions"
    Then the total creation time must be less than 120 seconds
    And the average throughput must be greater than 8000 evt/s

    # Phase 2: Snapshot after 1M
    When I create a snapshot for "transactions" with state of 1000000 elements
    And I flush all stores with fsync

    # Phase 3: Snapshot validation
    Then the snapshot for "transactions" must exist
    And the snapshot for "transactions" must contain 1000000 events
    And the snapshot for "transactions" must have a valid CRC32
    And the snapshot file size must be reasonable

    # Phase 4: Post-snapshot events
    When I create 10000 "Transaction" with aggregate_id "transactions"
    And I flush all stores

    # Phase 5: Recovery test after crash
    When I simulate a brutal crash
    And I reload the multi-file store from "/tmp/lithair-stress-snap-1m"

    # Phase 6: Recovery performance
    When I measure the complete recovery time for "transactions"
    And I measure the recovery time after snapshot for "transactions"
    Then recovery with snapshot must be at least 90x faster
    And recovery time with snapshot must be less than 5 seconds

    # Phase 7: Final validation
    And the total number of events for "transactions" must be 1010000
    And all CRC32 must be valid

  # ==================== MULTI-AGGREGATE STRESS ====================

  @multi
  Scenario: Multi-aggregate - 100K events distributed across 100 aggregates
    Given a multi-file store with snapshot threshold at 500 in "/tmp/lithair-stress-multi"

    # Phase 1: Distributed creation
    When I create 100000 events distributed across 100 aggregates
    And I flush all stores

    # Phase 2: Snapshot creation for each aggregate
    When I create snapshots for all aggregates

    # Phase 3: Validation
    Then 100 snapshots must exist
    And all snapshots must have a valid CRC32

    # Phase 4: Post-snapshot addition
    When I create 1000 events distributed across 100 aggregates
    And I flush all stores

    # Phase 5: Recovery test
    When I simulate a brutal crash
    And I reload the multi-file store from "/tmp/lithair-stress-multi"

    # Phase 6: Final validation
    Then each aggregate must have 1010 events
    And distributed recovery must use snapshots

  # ==================== SNAPSHOT ROTATION ====================

  @rotation
  Scenario: Snapshot rotation - Multiple snapshot management
    Given a multi-file store with snapshot threshold at 1000 in "/tmp/lithair-stress-rotation"

    # Phase 1: First batch + snapshot
    When I create 5000 "Event" with aggregate_id "rotating"
    And I flush all stores
    When I create a snapshot for "rotating" with state of 5000 elements
    Then the snapshot for "rotating" must exist

    # Phase 2: Second batch + new snapshot
    When I create 5000 "Event" with aggregate_id "rotating"
    And I flush all stores
    When I create a snapshot for "rotating" with state of 10000 elements
    Then the snapshot for "rotating" must contain 10000 events

    # Phase 3: Third batch + final snapshot
    When I create 5000 "Event" with aggregate_id "rotating"
    And I flush all stores
    When I create a snapshot for "rotating" with state of 15000 elements
    Then the snapshot for "rotating" must contain 15000 events

    # Phase 4: Recovery validation
    When I simulate a brutal crash
    And I reload the multi-file store from "/tmp/lithair-stress-rotation"
    Then the total number of events for "rotating" must be 15000
