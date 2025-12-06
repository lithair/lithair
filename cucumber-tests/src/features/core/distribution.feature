Feature: Distribution and Consensus
  As a distributed systems architect
  I want Lithair to support multi-node clustering
  In order to ensure high availability and consistency

  Background:
    Given a Raft cluster of 3 nodes
    And node 1 is the leader
    And nodes 2 and 3 are followers

  Scenario: Leader election
    When the current leader fails
    Then a new leader must be elected in less than 5 seconds
    And the cluster must continue to function

  Scenario: Data replication
    When I write data on the leader
    Then this data must be replicated on all followers
    And consistency must be guaranteed
    And the operation must be confirmed only after majority replication

  Scenario: Network partition and split-brain
    When the cluster is partitioned into 2 groups
    Then only the majority group must accept writes
    And the minority group must refuse writes
    And consistency must be preserved

  Scenario: Joining an existing cluster
    When a new node joins the cluster
    Then it must synchronize all existing data
    And participate in consensus
    And not disrupt the service

  Scenario: Horizontal scalability
    When I add nodes to the cluster
    Then processing capacity must increase
    And latency must remain stable
    And availability must be maintained
