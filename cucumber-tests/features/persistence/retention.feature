# Retention & Pinned Fields — Memory/Disk Tiering for Event-Sourced State
#
# Lithair is memory-first, but not memory-only. The #[retention] annotation
# controls how many items stay fully projected in RAM. Older items are evicted
# to disk-only, except for #[pinned] fields which remain in memory for fast
# listing and filtering. The event store remains the single source of truth.

Feature: Retention and Pinned Fields
  As a Lithair application developer
  I want to control which data stays in memory and which lives on disk
  So that my application can handle large datasets without unbounded RAM growth

  Background:
    Given a Lithair engine with event sourcing enabled
    And the engine uses a temporary data directory

  # ==================== BACKWARD COMPATIBILITY ====================

  @core @retention
  Scenario: Default behavior without retention annotation
    Given a model with no retention annotation
    When I create 500 items
    Then all 500 items should be fully in memory
    And all fields should be accessible without disk reads

  # ==================== CORE RETENTION BEHAVIOR ====================

  @core @retention
  Scenario: Retention limit evicts oldest items from full memory
    Given a model with retention limit of 100
    When I create 150 items sequentially
    Then only the 100 most recent items should be fully in memory
    And the 50 oldest items should be evicted from full memory

  @core @retention
  Scenario: Evicted items are still accessible from disk
    Given a model with retention limit of 10
    When I create 20 items with known content
    Then the 10 oldest items should be evicted
    When I request evicted item 1 by id
    Then the full item should be returned from disk
    And all fields should match the original content

  @core @retention
  Scenario: New writes evict the oldest item when at capacity
    Given a model with retention limit of 5
    And 5 items exist in memory
    When I create a 6th item
    Then the 1st item should be evicted from full memory
    And the 6th item should be fully in memory
    And the total item count should be 6

  @core @retention
  Scenario: Deletion of an in-memory item frees a retention slot
    Given a model with retention limit of 5
    And 5 items exist in memory
    When I delete the 3rd item
    And I create a new item
    Then 5 items should be fully in memory
    And no items should be evicted

  # ==================== PINNED FIELDS ====================

  @core @retention @pinned
  Scenario: Pinned fields remain in memory after eviction
    Given a model with retention limit of 10
    And fields "title" and "author" are pinned
    And field "content" is not pinned
    When I create 20 items with title, author, and content
    Then the 10 oldest items should be evicted from full memory
    But the "title" field of evicted items should be in memory
    And the "author" field of evicted items should be in memory

  @core @retention @pinned
  Scenario: Non-pinned fields are not in memory after eviction
    Given a model with retention limit of 10
    And fields "title" and "author" are pinned
    And field "content" is not pinned
    When I create 20 items with title, author, and content
    Then accessing "content" of an evicted item requires a disk read
    And the disk read returns the correct content

  @core @retention @pinned
  Scenario: All fields of non-evicted items are in memory
    Given a model with retention limit of 10
    And fields "title" is pinned
    When I create 5 items
    Then all 5 items should have all fields in memory
    And no disk reads should be required

  # ==================== MEMORY FOOTPRINT ====================

  @core @retention @memory
  Scenario: Memory footprint stays bounded with retention
    Given a model with retention limit of 100
    And fields "title" and "date" are pinned
    And field "body" is not pinned
    When I create 1000 items where each body is 10KB
    Then memory used by the model should be less than 5MB
    # Without retention: 1000 * 10KB = ~10MB bodies alone
    # With retention: 100 * 10KB bodies + 1000 * ~200B pinned ≈ 1.2MB

  # ==================== LISTING AND FILTERING ====================

  @core @retention @pinned @filter
  Scenario: Listing uses pinned fields without disk access
    Given a model with retention limit of 10
    And fields "title", "author", and "date" are pinned
    When I create 100 items with varied titles and authors
    And I list all items
    Then the listing should return 100 items
    And each item should include title, author, and date
    And no disk reads should be performed

  @core @retention @pinned @filter
  Scenario: Filtering on pinned fields is fast
    Given a model with retention limit of 10
    And field "category" is pinned
    When I create 100 items across 5 categories
    And I filter items where category is "tech"
    Then only items with category "tech" should be returned
    And the filter should execute in under 10ms
    And no disk reads should be performed

  @core @retention @pinned @filter
  Scenario: Requesting full item detail loads non-pinned fields
    Given a model with retention limit of 10
    And field "title" is pinned
    And field "body" is not pinned
    When I create 50 items
    And I request the full detail of evicted item 1
    Then the response should include both title and body
    And exactly 1 disk read should be performed

  # ==================== SEARCH ====================

  @core @retention @search
  Scenario: Search on pinned fields returns results from all items
    Given a model with retention limit of 10
    And field "title" is pinned
    When I create 50 items with varied titles
    And 3 items have "lithair" in their title
    And I search for "lithair" in title
    Then exactly 3 results should be returned
    And no disk reads should be performed

  @core @retention @search
  Scenario: Search on non-pinned fields scans the event store
    Given a model with retention limit of 10
    And field "title" is pinned
    And field "body" is not pinned
    When I create 50 items with varied body content
    And 2 items contain "remboursement" in their body
    And I search for "remboursement" in body
    Then exactly 2 results should be returned
    And the search should complete in under 500ms

  # ==================== RESTART AND REPLAY ====================

  @core @retention @replay
  Scenario: Event replay respects retention policy
    Given a model with retention limit of 10
    And 50 items have been created and persisted
    When the engine is restarted
    Then only the 10 most recent items should be fully in memory
    And the 40 oldest items should have only pinned fields in memory
    And all 50 items should be accessible by id

  @core @retention @replay
  Scenario: Disk index is rebuilt on replay
    Given a model with retention limit of 10
    And 50 items have been created and persisted
    When the engine is restarted
    And I request evicted item 5 by id
    Then the item should be loaded from disk via the index
    And the content should match the original

  @core @retention @replay @snapshot
  Scenario: Snapshots work with retention policy
    Given a model with retention limit of 100
    When I create 500 items
    And a snapshot is triggered
    And 50 more items are created
    And the engine is restarted
    Then only the 100 most recent items should be fully in memory
    And all 550 items should be accessible by id
    And replay should only process events after the snapshot

  # ==================== CONFIGURATION ====================

  @core @retention @config
  Scenario: Retention configurable by item count
    Given a model with retention configured as "memory = 200"
    When I create 300 items
    Then 200 items should be fully in memory
    And 100 items should be evicted

  @core @retention @config
  Scenario: Retention configurable by duration
    Given a model with retention configured as "memory = 30d"
    And items have been created over a span of 60 days
    When the retention policy is evaluated
    Then items older than 30 days should be evicted
    And items within 30 days should be fully in memory

  @core @retention @config
  Scenario: Retention configurable by memory budget
    Given a model with retention configured as "max_mb = 50"
    When I create items until projected memory exceeds 50MB
    Then items should be evicted oldest-first
    And projected memory should stay under 50MB

  @core @retention @config
  Scenario: Runtime override via environment variable
    Given a model with retention limit of 100
    And the environment variable "LT_EMAIL_MEMORY_RETENTION" is set to "50"
    When the engine is initialized
    Then the effective retention limit should be 50
    And environment config should take precedence over annotation

  # ==================== EDGE CASES ====================

  @core @retention @edge
  Scenario: Retention of 0 means all items disk-only except pinned
    Given a model with retention limit of 0
    And field "title" is pinned
    When I create 10 items
    Then no items should be fully in memory
    And all 10 items should have only "title" in memory
    And all items should be accessible from disk

  @core @retention @edge
  Scenario: No pinned fields means evicted items have no memory footprint
    Given a model with retention limit of 5
    And no fields are pinned
    When I create 10 items
    Then the 5 evicted items should only have their id indexed
    And memory footprint of evicted items should be minimal

  @core @retention @edge
  Scenario: Concurrent writes respect retention limit
    Given a model with retention limit of 100
    When 200 items are created concurrently from 10 threads
    Then exactly 200 items should exist in total
    And at most 100 items should be fully in memory
    And all 200 items should be accessible by id

  @core @retention @edge
  Scenario: Update to an evicted item promotes it back to memory
    Given a model with retention limit of 5
    And 10 items exist with items 1-5 evicted
    When I update evicted item 3
    Then item 3 should be fully in memory
    And the oldest in-memory item should be evicted to make room

  # ==================== INTEGRATION WITH EXISTING FEATURES ====================

  @retention @hash-chain
  Scenario: Hash chain integrity preserved across retention boundary
    Given a model with retention limit of 5
    And hash chain verification is enabled
    When I create 20 items
    Then the event store hash chain should be valid
    And verification should cover all 20 events regardless of retention

  @retention @replication
  Scenario: Retention policy is per-node in a cluster
    Given a 3-node cluster
    And the model has retention limit of 10
    When 50 items are created on the leader
    And items are replicated to followers
    Then each node should apply its own retention policy
    And each node should have only 10 items fully in memory
    And all 50 items should be accessible on any node
