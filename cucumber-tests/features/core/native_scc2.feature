Feature: Concurrent native state and persistence acknowledgements
  Native models keep their active state in memory. One mutation order coordinates
  validation, journal admission and publication; a durable acknowledgement waits
  for the journal. These scenarios qualify one process, not cluster consensus.

  Scenario Outline: Concurrent increments survive replay in both acknowledgement modes
    Given an isolated native engine with <mode> acknowledgements
    When eight clients increment the same record twenty times
    Then all 160 increments are present before and after reopening

    Examples:
      | mode    |
      | queued  |
      | durable |

  Scenario: A declared unique field has one winner under contention
    Given an isolated native engine with durable acknowledgements
    When sixteen clients claim the same unique name
    Then exactly one record owns the name after reopening

  Scenario: A storage failure cannot become a successful durable mutation
    Given an isolated native engine with durable acknowledgements
    When the journal cannot be opened for writing
    Then the mutation fails without publishing a record and subsequent flushes fail

  Scenario Outline: Updating an evicted record preserves its earlier increments
    Given an isolated native engine with <mode> acknowledgements
    And the native cache retains one full record
    When a second record evicts the first incremented record
    And the first record is incremented again
    Then both increments of the first record survive reopening

    Examples:
      | mode    |
      | queued  |
      | durable |

  Scenario: Uncheckpointed volatile state cannot be recovered from older events
    Given an isolated native engine with queued acknowledgements
    And the native cache retains one full record
    When a volatile change to the first record is evicted
    Then updating the first record is refused without using its older durable value
