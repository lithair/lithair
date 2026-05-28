# Deferred Retention Modes — Not Yet Implemented
#
# These scenarios describe retention features beyond the currently
# implemented `#[retention(memory = N)]` count-based mode. They are
# kept here as specifications for future work, tracked in issue #97:
#
# - Duration-based retention (`memory = "30d"`)
# - Memory-budget retention (`max_mb = 512`)
# - Runtime env override (`LT_<MODEL>_MEMORY_RETENTION`)
#
# All scenarios are tagged @deferred so they are excluded from the
# default cucumber run until the underlying parser/runtime support
# lands. Do NOT enable these scenarios in CI until the implementation
# catches up — they will fail by design.

Feature: Deferred Retention Modes
  As a Lithair application developer
  I want richer retention controls (duration, memory budget, env override)
  So that retention can adapt to operational constraints beyond raw count

  Background:
    Given a Lithair engine with event sourcing enabled
    And the engine uses a temporary data directory

  @deferred @retention @config
  Scenario: Retention configurable by duration
    Given a model with retention configured as "memory = 30d"
    And items have been created over a span of 60 days
    When the retention policy is evaluated
    Then items older than 30 days should be evicted
    And items within 30 days should be fully in memory

  @deferred @retention @config
  Scenario: Retention configurable by memory budget
    Given a model with retention configured as "max_mb = 50"
    When I create items until projected memory exceeds 50MB
    Then items should be evicted oldest-first
    And projected memory should stay under 50MB

  @deferred @retention @config
  Scenario: Runtime override via environment variable
    Given a model with retention limit of 100
    And the environment variable "LT_EMAIL_MEMORY_RETENTION" is set to "50"
    When the engine is initialized
    Then the effective retention limit should be 50
    And environment config should take precedence over annotation
