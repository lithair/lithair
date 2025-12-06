Feature: Observability and Monitoring
  As a DevOps engineer
  I want Lithair to expose detailed metrics
  In order to monitor system health and performance

  Background:
    Given a Lithair server with monitoring enabled
    And the Prometheus endpoints are configured
    And the health checks are implemented

  Scenario: Complete health checks
    When I call "/health"
    Then I must receive status "UP" or "DOWN"
    When I call "/ready"
    Then I must know if the server is ready for traffic
    When I call "/info"
    Then I must receive the version and system information

  Scenario: Prometheus metrics
    When I call "/observe/metrics"
    Then I must receive metrics in Prometheus format
    And the metrics must include: requests/sec, latency, memory
    And the metrics must be labeled by endpoint and status

  Scenario: Performance profiling
    When I call "/observe/perf/cpu"
    Then I must receive current CPU usage
    When I call "/observe/perf/memory"
    Then I must receive detailed memory usage
    When I call "/observe/perf/latency"
    Then I must receive latency percentiles

  Scenario: Structured logging
    When an error occurs
    Then it must be logged with ERROR level
    And contain timestamp, context and stack trace
    When a request is processed
    Then it must be logged with INFO level
    And contain method, URL, latency and status

  Scenario: Automatic alerts
    When latency exceeds 100ms
    Then an alert must be generated
    When memory exceeds 80%
    Then a critical alert must be issued
    When error rate exceeds 5%
    Then an alert must be triggered
