Feature: Native HTTP commit acknowledgements
  Generated model routes keep validation, journal order and memory publication coherent.

  Scenario: A failed journal write leaves the published HTTP state unchanged
    Then failed POST PUT PATCH and DELETE operations preserve the previous state and stop further writes

  Scenario: An acknowledged mutation can be replayed immediately
    Then HTTP create edit and delete acknowledgements survive reopening without a timer flush

  Scenario: Concurrent partial edits preserve each accepted field
    Then concurrent HTTP patches retain every acknowledged field after reopening

  Scenario: Programmatic mutations propagate persistence errors
    Then failed replicated and admin mutations neither publish state nor emit notifications
