Feature: Durable OpenRaft storage foundation
  The private log store preserves acknowledged consensus metadata across reopen.
  These scenarios do not qualify the current HTTP cluster or an application state machine.

  Scenario: Votes and log entries survive reopening
    Given an isolated OpenRaft log store
    When I persist a vote and three log entries
    And I reopen the OpenRaft log store
    Then the vote and three log entries are intact

  Scenario: A conflicting suffix stays replaced after reopening
    Given an isolated OpenRaft log store
    When I persist a vote and three log entries
    And I replace the conflicting suffix and purge the prefix
    And I reopen the OpenRaft log store
    Then only the replacement suffix and purge watermark remain

  Scenario: Damaged durable data is rejected
    Given an isolated OpenRaft log store
    When I persist a vote and three log entries
    And I damage the durable journal
    Then reopening the OpenRaft log store fails

  Scenario: Missing storage is not silently recreated
    Given an isolated OpenRaft log store
    When I persist a vote and three log entries
    And the stored journal and metadata disappear
    Then reopening the OpenRaft log store fails
