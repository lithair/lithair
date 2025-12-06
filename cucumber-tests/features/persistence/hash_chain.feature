# Hash Chain Integrity - Tamper-Evident Event Storage
# Implements SHA256-based hash chain for tamper detection

Feature: Hash Chain Integrity
  As a security-conscious developer
  I want events to form a cryptographic hash chain
  In order to detect any tampering with the event log

  Background:
    Given a Lithair server with hash chain enabled
    And event persistence is configured

  # ==================== BASIC HASH CHAIN ====================

  @core @hash-chain
  Scenario: Event hash computation on creation
    When I create an article via the API
    Then the event envelope should contain an event_hash field
    And the event_hash should be a valid SHA256 hex string (64 characters)
    And the hash should be computed from event content

  @core @hash-chain
  Scenario: Hash chain linking between events
    Given I have created an initial article (genesis event)
    When I create a second article
    Then the second event should have a previous_hash field
    And the previous_hash should match the genesis event's event_hash
    And both events should form a valid chain

  @core @hash-chain
  Scenario: Chain continuation across multiple events
    When I create 10 articles sequentially
    Then each event should reference the hash of the previous event
    And the first event should have no previous_hash (genesis)
    And the chain should be verifiable from start to end

  # ==================== TAMPER DETECTION ====================

  @security @hash-chain @tamper-detection
  Scenario: Detect payload tampering
    Given I have created an article with title "Original Title"
    When someone manually modifies the event payload in the log file
    And I verify the chain integrity
    Then the verification should report an invalid hash
    And the tampered event should be identified by index and event_id
    And the chain should be marked as INVALID

  @security @hash-chain @tamper-detection
  Scenario: Detect event deletion
    Given I have created 5 articles forming a hash chain
    When someone deletes the 3rd event from the log file
    And I verify the chain integrity
    Then the verification should detect a broken chain link
    And the gap should be reported at the expected position
    And data integrity should be flagged as compromised

  @security @hash-chain @tamper-detection
  Scenario: Detect event insertion
    Given I have created 3 articles forming a hash chain
    When someone inserts a fake event between events 1 and 2
    And I verify the chain integrity
    Then the fake event's previous_hash should not match
    And the insertion should be detected
    And the chain should be marked as INVALID

  @security @hash-chain @tamper-detection
  Scenario: Detect event reordering
    Given I have created events A, B, C in sequence
    When someone reorders the events to A, C, B
    And I verify the chain integrity
    Then the chain links should be broken
    And the reordering should be detected

  # ==================== BACKWARD COMPATIBILITY ====================

  @compatibility @hash-chain
  Scenario: Load legacy events without hash chain
    Given an existing events.raftlog file with legacy events (no hashes)
    When I start the Lithair server
    Then the legacy events should be loaded successfully
    And chain verification should report them as legacy events
    And new events should start a fresh hash chain

  @compatibility @hash-chain
  Scenario: Mixed legacy and hashed events
    Given legacy events exist in the log
    When I create new events after enabling hash chain
    Then new events should have hash chain fields
    And legacy events should remain unchanged
    And verification should accept the mixed chain with warnings

  # ==================== CHAIN VERIFICATION API ====================

  @api @hash-chain
  Scenario: Chain verification endpoint
    Given I have created 100 events
    When I call the chain verification API
    Then I should receive a verification result containing:
      | field            | expected        |
      | total_events     | 100             |
      | verified_events  | 100             |
      | legacy_events    | 0               |
      | is_valid         | true            |
    And the response should include a human-readable summary

  @api @hash-chain
  Scenario: Chain verification with detailed error report
    Given I have a corrupted event log
    When I call the chain verification API
    Then the response should include:
      | field           | description                        |
      | invalid_hashes  | list of events with bad hashes     |
      | broken_links    | list of chain discontinuities      |
      | is_valid        | false                              |
    And each error should include event_index and event_id

  # ==================== PERFORMANCE ====================

  @benchmark @hash-chain
  Scenario: Hash chain performance overhead
    Given a Lithair server with hash chain enabled
    When I create 10000 events
    Then the hash computation overhead should be less than 10%
    And throughput should remain above 1000 events/sec
    And chain verification should complete in less than 5 seconds

  @benchmark @hash-chain
  Scenario: Large chain verification
    Given an events.raftlog with 100000 hashed events
    When I verify the entire chain
    Then verification should complete in less than 30 seconds
    And memory usage should remain bounded
    And all 100000 events should be verified

  # ==================== DISTRIBUTED HASH CHAIN ====================

  @distributed @hash-chain
  Scenario: Hash chain integrity per node
    Given a 3-node Lithair cluster with hash chain enabled
    When I create 100 articles on the leader
    And data is replicated to followers
    Then each node should have its own valid hash chain
    And chain verification should pass on all nodes

  @distributed @hash-chain @tamper
  Scenario: Cross-node tamper detection
    Given a 3-node Lithair cluster with hash chain enabled
    And 50 articles have been replicated to all nodes
    When someone tampers with events on follower node 2
    Then chain verification on node 2 should fail
    But chain verification on nodes 1 and 3 should pass
    And the leader can be used as source of truth

  @distributed @hash-chain @recovery
  Scenario: Recovery from tampered follower
    Given a 3-node cluster where node 2 has tampered data
    When node 2 detects chain corruption
    Then it should request full resync from leader
    And its hash chain should be rebuilt from leader's data
    And chain verification should pass after recovery
