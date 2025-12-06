Feature: Lithair Metrics and Monitoring
  As a system administrator
  I want to monitor Lithair performance and health status
  In order to anticipate problems and optimize performance

  Background:
    Given a Lithair server with monitoring enabled
    And metrics are collected automatically
    And the /metrics endpoint is exposed

  Scenario: HTTP performance metrics
    When the server processes HTTP requests
    Then the number of requests per second must be measured
    And average response times must be tracked
    And status codes must be counted
    And metrics must be available on /metrics

  Scenario: Memory usage monitoring
    When the server runs under load
    Then memory usage must be monitored in real-time
    And memory peaks must be detected
    And memory leaks must be identified
    And alerts must be triggered if necessary

  Scenario: Concurrency and throughput metrics
    When 1000 simultaneous requests are processed
    Then the number of active connections must be measured
    And throughput per thread must be calculated
    And P95, P99 latency must be tracked
    And bottlenecks must be identified

  Scenario: Automatic health checks
    When the /health endpoint is called
    Then the server status must be verified
    And external dependencies must be tested
    And a detailed health report must be returned
    And the status code must reflect the real state

  Scenario: Proactive alerts and notifications
    When CPU usage exceeds 80%
    And average latency exceeds 100ms
    And error rate exceeds 5%
    Then an alert must be generated automatically
    And administrators must be notified
    And corrective actions must be suggested

  Scenario: Metrics aggregation and retention
    When metrics are collected for 24h
    Then data must be aggregated by intervals
    And detailed metrics must be archived
    And long-term trends must be calculated
    And storage space must be optimized
