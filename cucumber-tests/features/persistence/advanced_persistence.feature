@advanced @wip
Feature: Advanced Multi-File Persistence
  As a high-availability critical application developer
  I want robust persistence with multi-files and integrity verification
  In order to guarantee zero data loss even in case of crash

  Background:
    Given a Lithair engine with multi-file persistence enabled
    And strict verification mode is activated

  Scenario: Real-time Memory <-> File synchronization
    When I create 100 articles in memory
    Then each article must be written immediately to disk
    And reading the file must return exactly 100 articles
    And memory/file checksums must match
    And no data must be lost in case of immediate crash

  Scenario: Multi-tables with separate files
    Given a database with 3 tables: "articles", "users", "comments"
    When I insert data in each table
    Then 3 distinct files must be created: "articles.raft", "users.raft", "comments.raft"
    And each file must contain only its table's data
    And total file size must match inserted data
    And I can read each table independently

  Scenario: ACID transactions with WAL (Write-Ahead Log)
    When I start a multi-table transaction
    And I insert 10 articles, 5 users, 20 comments
    Then the WAL must contain all operations in order
    And no data must be visible before commit
    When I commit the transaction
    Then all data must appear atomically
    And the WAL must be emptied after confirmation
    And data files must be up to date

  Scenario: Rollback on transaction failure
    When I start a transaction
    And I insert 50 valid articles
    And I insert 1 invalid article that causes an error
    Then the transaction must be rolled back automatically
    And none of the 51 articles must be persisted
    And memory state must be restored
    And files must not be modified

  Scenario: Integrity verification with CRC32 checksums
    Given 500 articles persisted with checksums
    When I read each article from disk
    Then the CRC32 checksum must be verified for each read
    And any corruption must be detected immediately
    And an error log must be generated for corruptions
    And corrupted articles must be marked as invalid

  Scenario: File compaction and optimization
    Given a file of 10000 events with 3000 deletions
    When I launch manual compaction
    Then a new optimized file must be created
    And it must contain only the 7000 active events
    And the old file must be archived with timestamp
    And file size must be reduced by at least 30%
    And all data must remain accessible

  Scenario: Incremental backup with delta
    Given a database with 1000 articles
    When I modify 50 articles
    And I launch an incremental backup
    Then only the 50 modified articles must be backed up
    And a delta file "backup-TIMESTAMP.delta" must be created
    And restoration must reconstruct exact state
    And backup time must be less than 100ms

  Scenario: Asynchronous file replication
    Given 3 Lithair nodes in cluster
    When I write 200 articles on the leader
    Then files must be replicated on all followers
    And each node must have identical files
    And checksums must match between nodes
    And replication latency must be less than 50ms

  Scenario: Optimized read with memory cache
    Given 10000 articles persisted on disk
    And an LRU cache of 1000 entries
    When I read 100 frequently accessed articles
    Then 99% of reads must come from cache
    And only 1 article must be read from disk
    And average latency must be less than 0.1ms
    And cache hit rate must be greater than 95%

  Scenario: Managing multiple format versions
    Given files in format v1, v2, and v3
    When I load data with automatic migration
    Then all formats must be read correctly
    And data must be migrated to format v3
    And old files must be kept as backup
    And no data must be lost during migration

  Scenario: Batch write performance
    When I write 10000 articles in batch mode
    Then all writes must be grouped in batches of 1000
    And throughput must exceed 50000 writes/second
    And memory usage must remain stable
    And all articles must be persisted correctly
    And final verification must confirm 10000 articles

  Scenario: Recovery after crash during write
    Given a batch write of 5000 articles in progress
    When the server crashes in the middle (after 2500 articles)
    And I restart the server
    Then the first 2500 articles must be present
    And the next 2500 must be absent
    And the WAL must be replayed automatically
    And state must be consistent (no corruption)
    And I can continue writing normally

  Scenario: Disk space monitoring
    Given a disk quota of 1GB
    When usage reaches 90%
    Then a WARNING alert must be issued
    And automatic compaction must start
    When usage reaches 95%
    Then non-critical writes must be blocked
    And a CRITICAL alert must be sent
    And emergency cleanup must be triggered

  Scenario: Data-at-rest encryption (AES-256)
    Given AES-256-GCM encryption enabled
    When I write 1000 sensitive articles
    Then each file must be encrypted with a unique key
    And plaintext data must never touch disk
    And reading must decrypt automatically
    And performance must not degrade by more than 10%
    And files must be unreadable without the key

  Scenario: Complete persistence audit trail
    When I perform 100 varied operations (CRUD)
    Then each operation must be logged in the audit trail
    And each log must contain: timestamp, user_id, operation, data_hash
    And audit trail must be immutable (append-only)
    And I can reconstitute complete history
    And detect any modification attempt

  Scenario: Hot backup without service interruption
    Given a production server with continuous traffic
    When I launch a complete backup
    Then backup must occur without blocking writes
    And reads must continue normally
    And backup must be consistent (snapshot at time T)
    And performance must not degrade by more than 5%
    And backup file must be compressed (gzip)

  Scenario: Point-in-time restoration
    Given hourly backups for 7 days
    When I want to restore state from 3 days ago, 14:35
    Then the system must identify necessary snapshot + deltas
    And restore exact state at that timestamp
    And all later data must be ignored
    And restoration must take less than 2 minutes

  Scenario: Managing large files (>10GB)
    Given a table with 10 million articles (15GB of data)
    When I perform CRUD operations
    Then file must be fragmented in 1GB chunks
    And each chunk must have its own index
    And reads must target the right chunk directly
    And performance must remain constant
    And memory used must not exceed 500MB

  Scenario: Automatic corruption detection and repair
    Given a file with 5 corrupted blocks out of 1000
    When the system detects corruption at startup
    Then corrupted blocks must be identified precisely
    And system must attempt repair from WAL
    And if impossible, restore from last snapshot
    And irrecoverable blocks must be marked
    And a corruption report must be generated
