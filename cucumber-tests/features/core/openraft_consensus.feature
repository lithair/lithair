Feature: Authenticated three-node OpenRaft foundation
  This test state machine retains all logs. It does not enable clustered models or sessions.

  Scenario: Acknowledged commands survive a leader crash and return
    Given three authenticated OpenRaft processes with durable logs
    When the leader acknowledges a command and is killed
    Then the remaining majority accepts writes
    When the old leader restarts from its existing log
    Then all three processes recover the acknowledged commands

  Scenario: A minority cannot acknowledge writes or consistent reads
    Given three authenticated OpenRaft processes with durable logs
    When the leader is isolated by cutting its TCP connections
    Then the isolated process rejects writes and read barriers
    And the remaining majority accepts writes
    When the TCP partition is healed
    Then all three processes recover the acknowledged commands

  Scenario: A complete cold restart recovers membership and committed state
    Given three authenticated OpenRaft processes with durable logs
    When all three processes are killed after an acknowledged command and restarted
    Then the remaining majority accepts writes
    And all three processes recover the acknowledged commands
