Feature: Recoverable native checkpoints
  Native model declarations remain unchanged when snapshots reclaim event history.

  Scenario: A checkpoint marks an exact replay boundary
    Then SCC recovery applies only increments after the snapshot

  Scenario: SCC compaction preserves acknowledged state
    Then the compacted SCC state survives reopening

  Scenario: Journal reclamation requires a checkpoint
    Then native truncation without a checkpoint preserves the journal and fails

  Scenario: HTTP state survives repeated maintenance
    Then repeated native HTTP compaction preserves updates and deletes and rejects snapshot corruption
