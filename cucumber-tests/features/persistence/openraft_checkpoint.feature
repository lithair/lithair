Feature: Durable OpenRaft snapshots and compacted journals
  Scenario: A snapshot and retained suffix survive compaction and restart
    Given a durable OpenRaft log with committed commands
    When I publish a snapshot and purge its covered prefix
    And I compact and reopen the OpenRaft journal
    Then the snapshot and retained commands are intact
    And obsolete journal generations have been reclaimed

  Scenario: Corruption of the active snapshot stops recovery
    Given a durable OpenRaft log with committed commands
    When I publish a snapshot and purge its covered prefix
    And I damage the active snapshot generation
    Then OpenRaft checkpoint recovery fails
