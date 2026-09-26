Feature: Declarative native data uses committed consensus
  Scenario: Ordered HTTP mutations and durable request results
    Then three native nodes preserve concurrent PATCH fields, uniqueness and retry results

  Scenario: Application recovery after a leader change and full restart
    Then acknowledged native records and request results survive failover and cold restart

  Scenario: External storage cannot silently join the native consensus group
    Then a declarative Turso model is refused by native consensus
