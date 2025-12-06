Feature: Real DeclarativeCluster Integration Tests
  As a distributed systems architect
  I want to test a real DeclarativeCluster with Raft and hash chains
  In order to verify the actual distributed behavior

  # ==================== BASIC CLUSTER STARTUP ====================

  @real-cluster @smoke
  Scenario: Start and stop a real 3-node cluster
    Given a real DeclarativeCluster of 3 nodes
    Then I should see the Raft leader information
    And I can stop the real cluster cleanly

  # ==================== DATA REPLICATION ====================

  @real-cluster @replication
  Scenario: Data replication across real cluster
    Given a real DeclarativeCluster of 3 nodes
    When I create a product on the leader
    Then the product should be visible on all nodes
    And I can stop the real cluster cleanly

  @real-cluster @replication @bulk
  Scenario: Bulk data replication across real cluster
    Given a real DeclarativeCluster of 3 nodes
    When I create 5 products on the leader
    Then the product should be visible on all nodes
    And I can stop the real cluster cleanly

  # ==================== LEADER ELECTION ====================

  @real-cluster @leader-election
  Scenario: Static leader election with lowest node ID
    Given a real DeclarativeCluster of 3 nodes
    Then I should see the Raft leader information
    And I can stop the real cluster cleanly

  # ==================== WRITE REDIRECTION ====================

  @real-cluster @write-redirect
  Scenario: Followers redirect writes to leader
    Given a real DeclarativeCluster of 3 nodes
    When I write to follower node 1
    Then the write should be redirected to the leader
    And I can stop the real cluster cleanly

  # ==================== HASH CHAIN ON CLUSTER ====================

  @real-cluster @hash-chain
  Scenario: Hash chain maintained on real cluster nodes
    Given a real DeclarativeCluster of 3 nodes
    When I create 5 products on the leader
    Then each real node should have its own hash chain
    And hash chain verification should pass on all real nodes
    And I can stop the real cluster cleanly

  # ==================== FAULT TOLERANCE ====================

  @real-cluster @fault-tolerance
  Scenario: Leader discovery endpoint works
    Given a real DeclarativeCluster of 3 nodes
    Then the leader discovery endpoint should return correct leader info
    And I can stop the real cluster cleanly

  @real-cluster @fault-tolerance @heartbeat
  Scenario: Leader sends heartbeats to followers
    Given a real DeclarativeCluster of 3 nodes
    When I wait for 3 seconds
    Then the followers should have received heartbeats
    And I can stop the real cluster cleanly

  @real-cluster @fault-tolerance @leader-failure
  Scenario: Follower detects leader failure and triggers election
    Given a real DeclarativeCluster of 3 nodes
    When I create a product on the leader
    And I kill the leader node
    And I wait for 12 seconds
    Then a new leader should be elected
    And the cluster should remain operational
    And I can stop the real cluster cleanly
