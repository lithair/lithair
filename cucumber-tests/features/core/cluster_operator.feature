Feature: Preparing and inspecting an identity-bound cluster
  Scenario: Explicit provisioning follows validated operator configuration
    Given an operator configuration for three authenticated voters
    When I check the configuration and provision its local store
    Then the store retains the configured identity without bootstrapping
    And offline inspection leaves the store unchanged

  Scenario: Bootstrap requires all three peers to agree
    Given three uninitialized peers with a shared bootstrap plan
    When one peer is unreachable during bootstrap preflight
    Then bootstrap is refused without consuming its permission
    When the peer returns and the operator retries bootstrap
    Then the three nodes accept writes and recover after restart

  Scenario: Certificate rotation preserves identity and rejects stale operator actions
    Given an operator configuration for three authenticated voters
    When I check the configuration and provision its local store
    And I stage and retire a peer certificate with explicit generations
    Then stale credential transitions leave the store untouched
    And the store retains the configured identity without bootstrapping
    And offline inspection leaves the store unchanged
