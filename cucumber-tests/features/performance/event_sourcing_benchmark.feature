# Event Sourcing vs CRUD - Performance Benchmark
# Validation of Lithair's "append-only" philosophy

Feature: Event Sourcing vs traditional CRUD benchmark
  As a developer
  I want to compare event sourcing vs CRUD performance
  In order to validate that the append-only approach is performant

  Background:
    Given persistence is enabled by default

  # ==================== WRITE PERFORMANCE ====================

  @benchmark @write
  Scenario: Append-only vs random I/O write performance
    Given a Lithair server on port 21000 with persistence "/tmp/lithair-bench-write"
    When I measure the time to create 10000 articles in append mode
    Then append-only time must be less than 15 seconds
    And append throughput must be greater than 500 writes/sec
    And all writes must be sequential in the file

  @benchmark @write @bulk
  Scenario: Bulk write performance - Massive data correction
    Given a Lithair server on port 21001 with persistence "/tmp/lithair-bench-bulk"
    And 1000 existing products with incorrect prices
    When I correct the 1000 prices by creating PriceUpdated events
    Then the 1000 correction events must be created in less than 2 seconds
    And history must show 2000 events (1000 Created + 1000 Updated)
    And no original data must be lost

  # ==================== READ PERFORMANCE ====================

  @benchmark @read
  Scenario: Read performance from memory (projection)
    Given a Lithair server on port 21002 with persistence "/tmp/lithair-bench-read"
    And 10000 articles loaded in memory
    When I measure the time for 100000 random reads
    Then average time per read must be less than 0.1ms
    And throughput must be greater than 100000 reads/sec
    And no read must access disk

  @benchmark @read @history
  Scenario: Entity history read performance
    Given a Lithair server on port 21003 with persistence "/tmp/lithair-bench-history"
    And an article with 100 events in its history
    When I retrieve the complete history of the article
    Then the response must arrive in less than 50ms
    And the history must contain exactly 100 events
    And events must be ordered chronologically

  # ==================== DATA ADMIN FEATURES ====================

  @benchmark @admin @history
  Scenario: Data Admin API - History endpoint
    Given a Lithair server on port 21004 with persistence "/tmp/lithair-bench-admin"
    And 100 articles with varied histories
    When I call GET /_admin/data/models/Article/{id}/history for each article
    Then all responses must arrive in less than 100ms each
    And each response must contain event_count, events, and timestamps
    And events must include Created, Updated, AdminEdit types

  @benchmark @admin @edit
  Scenario: Data Admin API - Event-Sourced Edit
    Given a Lithair server on port 21005 with persistence "/tmp/lithair-bench-edit"
    And an existing article with id "test-article-001"
    When I call POST /_admin/data/models/Article/{id}/edit with {"title": "New title"}
    Then a new AdminEdit event must be created
    And the event must NOT replace previous events
    And history must now contain one more event
    And the AdminEdit timestamp must be later than previous ones

  @benchmark @admin @bulk-edit
  Scenario: Data Admin API - Event-sourced bulk edit
    Given a Lithair server on port 21006 with persistence "/tmp/lithair-bench-bulk-edit"
    And 500 existing articles
    When I correct the "category" field of all 500 articles via the edit API
    Then 500 AdminEdit events must be created in less than 3 seconds
    And no original event must be modified
    And audit trail must be complete for all 500 articles

  # ==================== STARTUP PERFORMANCE ====================

  @benchmark @startup
  Scenario: Replay performance at startup
    Given an events.raftlog file with 100000 events
    When I start a Lithair server with this file
    Then replay must take less than 10 seconds
    And 100000 entities must be loaded in memory
    And the server must be ready to receive requests

  @benchmark @startup @snapshot
  Scenario: Startup performance with snapshot
    Given a snapshot containing 100000 entities
    And an events.raftlog file with 1000 post-snapshot events
    When I start a Lithair server with snapshot enabled
    Then startup must take less than 3 seconds
    And only 1000 events must be replayed
    And final state must be identical to scenario without snapshot

  # ==================== INTEGRITY UNDER LOAD ====================

  @benchmark @integrity
  Scenario: Event sourcing integrity under load
    Given a Lithair server on port 21007 with persistence "/tmp/lithair-bench-integrity"
    When I launch 100 threads that each create 100 articles
    And I launch 50 threads that modify random articles
    And I launch 20 threads that retrieve histories
    Then all 10000 articles must be created
    And no event must be lost
    And event order must be globally consistent
    And CRC32 validation must pass for all events

  @benchmark @integrity @hash-chain
  Scenario: Integrity with SHA256 hash chain
    Given a Lithair server with hash chain enabled
    And 1000 created events
    When I verify the integrity of the chain
    Then each event must reference the hash of the previous one
    And any manual file modification must be detected
    And the chain must be validatable end-to-end
    And verification should complete in less than 2 seconds

  # ==================== COMPACTION PERFORMANCE ====================

  @benchmark @compaction
  Scenario: Compaction performance with snapshot
    Given a Lithair server on port 21008 with persistence "/tmp/lithair-bench-compact"
    And 50000 events of which 40000 are obsolete
    When I trigger compaction with snapshot
    Then a snapshot must be created with consolidated state
    And the 40000 obsolete events must be archived
    And compaction must take less than 5 seconds
    And disk space used must decrease by at least 50%

  @benchmark @compaction @retention
  Scenario: Compaction with retention policy
    Given a Lithair server with retention policy "keep last 10 per entity"
    And 100 entities with 50 events each (5000 total)
    When I trigger compaction
    Then only 1000 events must remain (100 entities x 10 events)
    And a snapshot must capture state before compaction
    And hash chain must be preserved for remaining events

  # ==================== COMPARISON VS CRUD ====================

  @benchmark @comparison
  Scenario: Direct comparison append-only vs simulated UPDATE
    Given a Lithair server on port 21009 with persistence "/tmp/lithair-bench-compare"
    And 1000 existing articles
    When I measure time for 1000 modifications in append mode (events)
    And I simulate 1000 modifications in CRUD mode (read-write-rewrite)
    Then append mode must be at least 3x faster
    And append mode must use less CPU
    And both modes must produce the same final state

  @benchmark @comparison @audit
  Scenario: Added value of audit trail
    Given a Lithair server on port 21010 with persistence "/tmp/lithair-bench-audit"
    And 100 articles with multiple modifications
    When I ask "who modified what and when" for each article
    Then the response must be instant (< 10ms) thanks to history
    And information must include timestamps, event_types, and data
    And no separate audit table must be necessary
