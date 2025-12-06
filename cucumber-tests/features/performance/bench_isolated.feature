Feature: Isolated Benchmarks - Memory vs Disk vs E2E

  Scenario: BENCH 1 - Pure in-memory read (StateEngine)
    Given a Lithair server on port 20010 with persistence "/tmp/lithair-bench-read"
    And 10000 articles pre-loaded in memory
    When I read 100000 random articles via GET
    Then the average read time must be less than 1ms
    And the read throughput must exceed 50000 req/sec

  Scenario: BENCH 2 - Pure disk write (FileStorage)
    Given a Lithair server on port 20011 with persistence "/tmp/lithair-bench-write"
    When I create 50000 articles in direct write mode
    Then the events.raftlog file must contain exactly 50000 "ArticleCreated" events
    And the write throughput must be measured

  Scenario: BENCH 3 - Complete E2E (HTTP + Memory + Disk in //)
    Given a Lithair server on port 20012 with persistence "/tmp/lithair-bench-e2e"
    When I create 50000 articles via HTTP POST
    Then all articles must be in memory
    And the events.raftlog file must contain exactly 50000 "ArticleCreated" events
    And the E2E throughput must be measured

  Scenario: BENCH 4 - Read/Write Mix (Production realistic)
    Given a Lithair server on port 20013 with persistence "/tmp/lithair-bench-mix"
    And 10000 articles pre-loaded in memory
    When I run 80% reads and 20% writes for 30 seconds
    Then the total throughput must be measured
    And the P50, P95, P99 latencies must be calculated
