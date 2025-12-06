# Performance and Integrity of Lithair Database
# Critical tests to verify that under load, NO data is lost

Feature: Performance and Persistence Integrity of Lithair
  As a developer
  I want to verify that Lithair under load
  Persists ALL data without loss or truncation

  Background:
    Given persistence is enabled by default

  # ==================== INTEGRITY TESTS ====================

  Scenario: Create 1000 articles and verify they are ALL persisted
    Given a Lithair server on port 20000 with persistence "/tmp/lithair-integrity-1000"
    When I create 1000 articles quickly
    Then the events.raftlog file must exist
    And the events.raftlog file must contain exactly 1000 "ArticleCreated" events
    And no event must be missing
    And the event checksum must be valid

  Scenario: Complete CRUD - Create, Modify, Delete and verify persistence
    Given a Lithair server on port 20001 with persistence "/tmp/lithair-crud-test"
    When I create 100 articles quickly
    And I modify 50 existing articles
    And I delete 25 articles
    Then the events.raftlog file must exist
    And the events.raftlog file must contain exactly 100 "ArticleCreated" events
    And the events.raftlog file must contain exactly 50 "ArticleUpdated" events
    And the events.raftlog file must contain exactly 25 "ArticleDeleted" events
    And the final state must have 75 active articles
    And all events must be in chronological order

  Scenario: STRESS TEST - 100K articles with complete CRUD and performance measurement
    Given a Lithair server on port 20002 with persistence "/tmp/lithair-stress-100k"
    When I create 100000 articles quickly
    And I modify 10000 existing articles
    And I delete 5000 articles
    Then the events.raftlog file must exist
    And the events.raftlog file must contain exactly 100000 "ArticleCreated" events
    And the events.raftlog file must contain exactly 10000 "ArticleUpdated" events
    And the events.raftlog file must contain exactly 5000 "ArticleDeleted" events
    And the final state must have 95000 active articles
    And all events must be in chronological order
    And I stop the server cleanly

  Scenario: Create 10000 articles and verify complete integrity
    Given a Lithair server on port 20001 with persistence "/tmp/lithair-integrity-10k"
    When I create 10000 articles in parallel with 50 threads
    Then all 10000 articles must be persisted
    And the number of events in events.raftlog must be exactly 10000
    And no duplicate must exist
    And the ID sequence must be continuous from 0 to 9999

  Scenario: Load test with integrity verification
    Given a Lithair server on port 20002 with persistence "/tmp/lithair-load-test"
    When I launch 5000 concurrent POST requests with 100 threads
    And I wait for all writes to complete
    Then the server must have responded to all 5000 requests
    And the events.raftlog file must contain exactly 5000 events
    And no error must be present in the logs
    And the average response time must be less than 50ms

  # ==================== PERFORMANCE TESTS ====================

  Scenario: Write performance - 1000 req/s
    Given a Lithair server on port 20003 with persistence "/tmp/lithair-perf-write"
    When I measure write performance for 10 seconds
    Then the server must process at least 1000 requests per second
    And all requests must be persisted
    And the error rate must be 0%
    And the p95 latency must be less than 100ms

  Scenario: Read performance with persistence
    Given a Lithair server on port 20004 with persistence "/tmp/lithair-perf-read"
    And 5000 articles already created and persisted
    When I measure read performance for 10 seconds
    Then the server must process at least 5000 requests per second
    And all reads must return valid data
    And the error rate must be 0%
    And the p99 latency must be less than 20ms

  Scenario: Mixed read/write performance (80/20)
    Given a Lithair server on port 20005 with persistence "/tmp/lithair-perf-mixed"
    When I run a mixed test for 30 seconds with:
      | Type  | Percentage | Concurrency |
      | Read  | 80%        | 100         |
      | Write | 20%        | 20          |
    Then the total throughput must be greater than 2000 req/s
    And all writes must be persisted
    And the error rate must be less than 0.1%
    And the average latency must be less than 30ms

  # ==================== PERSISTENCE UNDER LOAD TESTS ====================

  Scenario: Continuous persistence under high load
    Given a Lithair server on port 20006 with persistence "/tmp/lithair-persist-load"
    When I run a constant load of 500 req/s for 60 seconds
    Then exactly 30000 events must be persisted
    And the events.raftlog file must have a consistent size
    And no event must be corrupted
    And the time sequence must be strictly increasing

  Scenario: Restart with persisted data
    Given a Lithair server on port 20007 with persistence "/tmp/lithair-restart-test"
    And 1000 articles created and persisted
    When I stop the server
    And I restart the server on the same port with the same persistence
    Then the 1000 articles must be present in memory
    And I can create 1000 additional articles
    And the events.raftlog file must contain exactly 2000 events
    And the IDs must be continuous from 0 to 1999

  # ==================== ADVANCED INTEGRITY TESTS ====================

  Scenario: Event order verification
    Given a Lithair server on port 20008 with persistence "/tmp/lithair-event-order"
    When I create events in this order:
      | Type           | ID   |
      | ArticleCreated | art1 |
      | UserCreated    | usr1 |
      | ArticleUpdated | art1 |
      | CommentAdded   | cmt1 |
      | ArticleDeleted | art1 |
    Then the events must be in the file in the same order
    And each event must have a strictly increasing timestamp
    And the relationships between events must be preserved

  Scenario: Data corruption detection
    Given a Lithair server on port 20009 with persistence "/tmp/lithair-corruption-test"
    When I create 100 articles with checksums
    Then each article must have a valid checksum in the database
    And the total sum of checksums must match
    And no article must have corrupted data
    And the CRC32 verification must pass

  # ==================== EXTREME LOAD TESTS ====================

  Scenario: Extreme load - 50000 articles
    Given a Lithair server on port 20010 with persistence "/tmp/lithair-extreme-load"
    When I create 50000 articles in 10 batches of 5000
    Then all 50000 articles must be persisted
    And the events.raftlog file must be at least 5MB
    And no article must be missing
    And the database must remain consistent
    And the server must remain responsive (< 100ms latency)

  Scenario: Extreme concurrency test
    Given a Lithair server on port 20011 with persistence "/tmp/lithair-concurrency"
    When I launch 1000 threads that each create 10 articles simultaneously
    Then exactly 10000 articles must be persisted
    And no concurrency conflict must be detected
    And all IDs must be unique
    And no event must be duplicated

  # ==================== DATABASE SIZE TESTS ====================

  Scenario: Large database
    Given a Lithair server on port 20012 with persistence "/tmp/lithair-large-db"
    When I create articles with 10KB content each
    And I create 1000 of these articles
    Then the events.raftlog file must be at least 10MB
    And all articles must be retrievable
    And the read time must not degrade (< 50ms)
    And the database must be reloadable in less than 5 seconds

  # ==================== SNAPSHOT TESTS ====================

  Scenario: Snapshot creation under load
    Given a Lithair server on port 20013 with persistence "/tmp/lithair-snapshot"
    And the snapshot configuration is enabled every 1000 events
    When I create 5000 articles
    Then at least 5 snapshots must be created
    And each snapshot must be valid and recoverable
    And I can restore from any snapshot
    And the data after snapshot must be identical

  # ==================== DURABILITY TESTS ====================

  Scenario: Fsync durability
    Given a Lithair server on port 20014 with fsync enabled
    When I create 100 articles
    And I brutally kill the server (SIGKILL)
    And I restart the server
    Then all 100 articles must be present
    And no corruption must be detected

  Scenario: Durability without fsync (performance mode)
    Given a Lithair server on port 20015 without fsync
    When I create 1000 articles quickly
    Then the throughput must be greater than 5000 req/s
    And at least 95% of articles must be recoverable after crash
