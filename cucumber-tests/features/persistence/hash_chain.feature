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
