# Retention Performance — Validate that disk tiering doesn't kill latency
#
# These scenarios verify that the retention/pinned system meets acceptable
# performance thresholds. Separated from core retention tests because they
# involve larger datasets and longer execution times.

Feature: Retention Performance
  As a Lithair application developer
  I want the retention system to perform well at scale
  So that disk-tiered data remains usable in production

  Background:
    Given a Lithair engine with event sourcing enabled
    And the engine uses a temporary data directory

  # ==================== DISK READ LATENCY ====================

  @performance @retention
  Scenario: Single evicted item retrieval latency
    Given a model with retention limit of 100
    And 10000 items have been created with 1KB bodies
    When I request 100 random evicted items by id
    Then the average retrieval time should be under 5ms per item
    And the p99 retrieval time should be under 20ms

  @performance @retention
  Scenario: Evicted item retrieval scales with event store size
    Given a model with retention limit of 100
    When I create 1000 items with 1KB bodies and measure retrieval time
    And I create 10000 more items with 1KB bodies and measure retrieval time
    Then retrieval time at 11000 items should be less than 3x retrieval at 1000
    # Index-based lookup should be near-constant, not linear

  # ==================== LISTING PERFORMANCE ====================

  @performance @retention
  Scenario: Listing with pinned fields at scale
    Given a model with retention limit of 100
    And fields "title", "author", "date", "category" are pinned
    When I create 100000 items
    And I list all items (pinned fields only)
    Then the listing should return 100000 items
    And the listing should complete in under 200ms
    And no disk reads should be performed

  @performance @retention
  Scenario: Filtered listing performance
    Given a model with retention limit of 100
    And field "category" is pinned
    When I create 50000 items across 10 categories
    And I filter items where category is "tech"
    Then the filter should return approximately 5000 items
    And the filter should complete in under 50ms

  # ==================== REPLAY PERFORMANCE ====================

  @performance @retention @replay
  Scenario: Replay with retention is faster than full projection
    Given a model with retention limit of 100
    When I create 50000 items and persist them
    And I measure full replay time (no retention)
    And I measure retention-aware replay time
    Then retention-aware replay should be at least 2x faster
    # Only 100 items fully projected vs 50000

  @performance @retention @replay
  Scenario: Startup time stays bounded with large event stores
    Given a model with retention limit of 1000
    And fields "title" and "date" are pinned
    When 100000 events have been persisted
    And the engine is restarted
    Then startup time should be under 10 seconds
    And memory usage should be under 100MB

  # ==================== SEARCH PERFORMANCE ====================

  @performance @retention @search
  Scenario: Pinned field search at scale
    Given a model with retention limit of 100
    And field "title" is pinned
    When I create 100000 items with varied titles
    And I search for a term present in 50 titles
    Then the search should return 50 results
    And the search should complete in under 20ms

  @performance @retention @search
  Scenario: Disk field search with index
    Given a model with retention limit of 100
    And field "body" is not pinned but has a disk index
    When I create 10000 items with 1KB bodies
    And I search for a term present in 10 bodies
    Then the search should return 10 results
    And the search should complete in under 500ms

  # ==================== EVICTION OVERHEAD ====================

  @performance @retention
  Scenario: Write throughput with retention eviction
    Given a model with retention limit of 1000
    When I measure write throughput without retention (10000 writes)
    And I measure write throughput with retention (10000 writes)
    Then retention write throughput should be at least 80% of no-retention
    # Eviction adds overhead but should be minimal

  @performance @retention
  Scenario: Memory stability under sustained writes
    Given a model with retention limit of 1000
    And field "title" is pinned
    When I continuously write items for 30 seconds
    And I sample memory usage every 5 seconds
    Then memory usage should stay within 20% of the initial measurement
    And no memory leaks should be detected
