Feature: Bounded frontend persistence
  Scenario: Deploying unchanged assets is idempotent
    Then unchanged frontend loads, concurrent reloads and restarts do not append copies

  Scenario: Existing frontend history is reclaimed safely
    Then legacy frontend history compacts while preserving content, MIME and deletions

  Scenario: Changes remain durable without accumulating history
    Then frontend updates stay bounded and offline removals survive reopening in isolation

  Scenario: Corrupted history must not become a partial checkpoint
    Then corrupt frontend journals are rejected without discarding their evidence
