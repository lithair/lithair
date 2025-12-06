Feature: HTTP Server Performance
  As a high-performance framework
  I want to guarantee high throughput and low latency
  To support production applications under load

  Background:
    Given a Lithair server starts on port "21500"
    And the server uses persistence in "/tmp/cucumber-perf-test"

  @performance @critical
  Scenario: Write throughput - Minimum 1000 req/s
    Given the server is ready to receive requests
    When I create 1000 articles in parallel with 10 workers
    Then the total time must be less than 1 second
    And the throughput must be greater than 1000 requests per second
    And all articles must be persisted
    And no error must be logged

  @performance @critical
  Scenario: Read throughput - Minimum 5000 req/s
    Given the server contains 100 pre-created articles
    When I read the article list 5000 times with 20 workers
    Then the total time must be less than 1 second
    And the throughput must be greater than 5000 requests per second
    And the p95 latency must be less than 50 milliseconds
    And no connection error must occur

  @performance
  Scenario: Mixed load 80/20 - Minimum 2000 req/s
    Given the server contains 50 pre-created articles
    When I run a mixed load for 10 seconds:
      | type  | percentage | workers |
      | read  | 80         | 16      |
      | write | 20         | 4       |
    Then the total throughput must be greater than 2000 requests per second
    And the error rate must be less than 0.1%
    And the p99 latency must be less than 100 milliseconds

  @performance @durability
  Scenario: Performance with fsync persistence
    Given the server has fsync enabled on each write
    When I create 500 articles sequentially
    Then the total time must be less than 2 seconds
    And all articles must be in the events.raftlog file
    And no article must be lost after a brutal restart

  @performance @http
  Scenario: HTTP/1.1 Keep-Alive
    Given the server supports HTTP/1.1 keep-alive
    When I make 100 requests with the same TCP connection
    Then all requests must succeed
    And no "Connection reset" error must occur
    And the number of TCP connections must be exactly 1

  @performance @concurrency
  Scenario: High concurrent load - 50 workers
    Given the server is ready
    When I launch 50 workers in parallel
    And each worker creates 20 articles
    Then 1000 articles must be created in total
    And the total time must be less than 5 seconds
    And all articles must have unique IDs
    And no data corruption must be detected

  @performance @latency
  Scenario: Latency under constant load
    Given the server is under constant load of 500 req/s
    When I measure latency for 30 seconds
    Then the p50 latency must be less than 10 milliseconds
    And the p95 latency must be less than 50 milliseconds
    And the p99 latency must be less than 100 milliseconds
    And no timeout must occur

  @performance @stress
  Scenario: Stress test - 10000 articles
    Given the server starts with an empty database
    When I create 10000 articles in batches of 100
    Then all 10000 articles must be created
    And the total time must be less than 30 seconds
    And the server memory must remain under 500 MB
    And the events.raftlog file must contain exactly 10000 events

  @performance @regression
  Scenario: Reference benchmark
    Given the server is in benchmark mode
    When I run the standard benchmark:
      | operation | number | workers |
      | POST      | 1000   | 10      |
      | GET       | 5000   | 20      |
      | PUT       | 500    | 5       |
    Then the metrics must be recorded
    And the performance report must be generated
    And the metrics must not regress by more than 10%
