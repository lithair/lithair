Feature: Lithair Distribution and Clustering
  As a distributed systems architect
  I want Lithair to support clustering and replication
  In order to ensure high availability and fault tolerance

  Background:
    Given a Lithair cluster of 3 nodes
    And the Raft protocol is enabled for consensus
    And data replication is configured
    And hash chain is enabled on all nodes

  # ==================== LEADER ELECTION ====================

  @core @raft @leader-election
  Scenario: Raft leader election
    When a 3-node cluster starts
    Then a leader must be elected automatically
    And the 2 other nodes must become followers
    And the leader must be able to accept writes
    And followers must redirect writes to the leader

  @core @raft @leader-election
  Scenario: Leader fault tolerance
    When the leader fails
    Then a new election must be triggered
    And a new leader must be elected among the followers
    And the cluster must continue to function
    And no data must be lost

  # ==================== DATA REPLICATION ====================

  @core @raft @replication
  Scenario: Synchronous data replication
    When a write is performed on the leader
    Then it must be replicated on all followers
    And confirmation must wait for majority (quorum)
    And strong consistency must be guaranteed
    And followers must have the same data

  @core @raft @replication
  Scenario: Bulk data replication
    When 100 writes are performed on the leader in quick succession
    Then all 100 items must be replicated to followers
    And bulk replication must use batching for efficiency
    And idempotence must prevent duplicate processing

  @core @raft @replication @http
  Scenario: HTTP replication endpoints
    Given a running 3-node cluster
    Then the leader should expose POST /internal/replicate
    And the leader should expose POST /internal/replicate_bulk
    And followers should accept replication requests from leader
    And unauthorized replication requests should be rejected

  # ==================== FAULT TOLERANCE ====================

  @core @raft @fault-tolerance
  Scenario: Network partition and split-brain
    When the network is partitioned into 2 groups
    Then only the group with majority must remain active
    And the minority group must refuse writes
    And split-brain must be avoided
    And data consistency must be preserved

  @core @raft @fault-tolerance
  Scenario: Node rejoin after failure
    When a node reconnects to the cluster
    Then it must synchronize its missing state
    And receive missing data via snapshot
    And rejoin the cluster as a follower
    And synchronization must not impact performance

  # ==================== SCALING ====================

  @core @raft @scaling
  Scenario: Horizontal scaling with node addition
    When a new node joins the cluster
    Then it must receive existing data
    And quorum must be updated
    And performance must improve
    And load must be distributed evenly

  # ==================== CONSISTENCY ====================

  @core @raft @consistency
  Scenario: Distributed operations consistency
    When concurrent writes are performed
    Then total order must be preserved
    And conflicts must be resolved by Raft
    And all nodes must see the same final state
    And operations must be ACID compliant

  # ==================== HASH CHAIN + REPLICATION ====================

  @core @raft @hash-chain
  Scenario: Hash chain maintained during replication
    When I create 50 articles on the leader
    And data is replicated to all followers
    Then each node should have its own hash chain
    And chain verification should pass on all nodes
    And event hashes should be computed locally on each node

  @security @raft @hash-chain @tamper
  Scenario: Tamper detection across replicated cluster
    Given 100 articles have been replicated across the cluster
    When someone tampers with events on follower node 2
    Then chain verification on node 2 should fail
    But chain verification on leader and node 3 should pass
    And the majority provides source of truth for recovery

  @security @raft @hash-chain @recovery
  Scenario: Automatic recovery from tampered node
    Given a follower node with detected chain corruption
    When the node requests resync from leader
    Then it should receive uncorrupted data
    And rebuild its local hash chain
    And chain verification should pass after recovery

  # ==================== CLUSTER STATUS ====================

  @api @raft @status
  Scenario: Cluster status endpoint
    Given a running 3-node cluster
    When I call GET /status on any node
    Then I should receive cluster information including:
      | field       | description                    |
      | is_leader   | true/false                     |
      | leader_port | port of current leader         |
      | peers       | list of known peer addresses   |
      | node_id     | unique identifier of this node |

  @api @raft @leader
  Scenario: Leader discovery endpoint
    Given a running 3-node cluster
    When I call GET /raft/leader on any node
    Then I should receive the current leader's address
    And the response should be consistent across all nodes
