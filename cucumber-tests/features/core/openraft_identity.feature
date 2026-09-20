Feature: Explicit identity and bootstrap of a three-node consensus group
  Scenario: A data directory belongs to exactly one node identity
    Given a provisioned consensus directory for node one
    When another node tries to recover that directory
    Then the identity mismatch is rejected
    And the original identity can still recover the directory

  Scenario: Bootstrap is an explicit single-use action
    Given three provisioned but uninitialized consensus processes
    Then no process has initialized the consensus group
    And followers cannot bootstrap the group
    When the designated node explicitly bootstraps the group
    Then the group accepts writes and survives a cold restart
    And no process can bootstrap the group again
