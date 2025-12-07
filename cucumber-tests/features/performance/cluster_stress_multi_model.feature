# Cluster Stress Test - Multi-Model with Random CRUD and Random Node Targeting
# This tests full data replication under heavy load across multiple models

Feature: CLUSTER STRESS TEST - Multi-Model Distributed Replication
  As a distributed systems engineer
  I want to stress test the cluster with random CRUD operations on multiple models
  In order to verify data consistency and replication under heavy load

  # ==================== QUICK VALIDATION (1K ops) ====================

  @real-cluster @stress @multi-model @quick
  Scenario: 1K random CRUD operations across 3 models - Quick validation
    Given a real LithairServer cluster of 3 nodes with multi-model support

    When I execute 1000 random CRUD operations across all models targeting random nodes
    And I wait for replication to complete

    Then all models should have consistent data across all nodes
    And the data files should be identical across all nodes
    And the operation count should match expected values
    And I can stop the real cluster cleanly

  # ==================== MEDIUM STRESS (10K ops) ====================

  @real-cluster @stress @multi-model @medium
  Scenario: 10K random CRUD operations - Medium stress test
    Given a real LithairServer cluster of 3 nodes with multi-model support

    When I execute 10000 random CRUD operations across all models targeting random nodes
    And I wait for replication to complete

    Then all models should have consistent data across all nodes
    And the data files should be identical across all nodes
    And the operation count should match expected values
    And I display stress test statistics
    And I can stop the real cluster cleanly

  # ==================== HIGH STRESS (100K ops) ====================

  @real-cluster @stress @multi-model @high
  Scenario: 100K random CRUD operations - High stress test
    Given a real LithairServer cluster of 3 nodes with multi-model support

    When I execute 100000 random CRUD operations across all models targeting random nodes
    And I wait for replication to complete

    Then all models should have consistent data across all nodes
    And the data files should be identical across all nodes
    And the operation count should match expected values
    And I display stress test statistics
    And I can stop the real cluster cleanly

  # ==================== ULTIMATE STRESS (1M ops) ====================

  @real-cluster @stress @multi-model @ultimate @slow
  Scenario: 1 MILLION random CRUD operations - Ultimate stress test
    Given a real LithairServer cluster of 3 nodes with multi-model support

    When I execute 1000000 random CRUD operations across all models targeting random nodes
    And I wait for replication to complete

    Then all models should have consistent data across all nodes
    And the data files should be identical across all nodes
    And the operation count should match expected values
    And I display stress test statistics
    And I can stop the real cluster cleanly

  # ==================== CONCURRENT STRESS ====================

  @real-cluster @stress @multi-model @concurrent
  Scenario: 10K concurrent random CRUD operations - Concurrency stress
    Given a real LithairServer cluster of 3 nodes with multi-model support

    When I execute 10000 concurrent random CRUD operations with 50 workers
    And I wait for replication to complete

    Then all models should have consistent data across all nodes
    And the data files should be identical across all nodes
    And no data should be lost or corrupted
    And I display stress test statistics
    And I can stop the real cluster cleanly

  # ==================== CHAOS MODE ====================

  @real-cluster @stress @multi-model @chaos
  Scenario: 5K operations with node failures - Chaos stress test
    Given a real LithairServer cluster of 3 nodes with multi-model support

    When I execute 2000 random CRUD operations across all models targeting random nodes
    And I kill a random follower node
    And I execute 2000 more random CRUD operations on remaining nodes
    And I wait for replication to complete

    Then the remaining nodes should have consistent data
    And operations should have succeeded on the majority
    And I display stress test statistics
    And I can stop the real cluster cleanly
