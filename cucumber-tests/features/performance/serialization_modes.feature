# Dual-Mode Serialization - JSON (simd-json) + Binary (rkyv)
# Test of both serialization modes for Lithair

Feature: Dual-mode JSON and rkyv serialization
  As a developer
  I want to be able to use JSON or rkyv for serialization
  In order to optimize performance depending on the usage context

  Background:
    Given a test type "Article" with fields id, title, price

  # ==================== JSON MODE (simd-json) ====================

  @serialization @json
  Scenario Outline: JSON roundtrip serialization
    Given an article with id "<id>" title "<title>" and price <price>
    When I serialize the article in JSON mode
    And I deserialize the JSON data
    Then the deserialized article must have id "<id>"
    And the deserialized article must have title "<title>"
    And the deserialized article must have price <price>

    Examples:
      | id          | title                   | price  |
      | art-001     | First article           | 19.99  |
      | art-002     | Article with accents éè | 29.50  |
      | art-003     | Unicode article 日本語  | 99.00  |

  @serialization @json @benchmark
  Scenario: JSON serialization performance
    Given 1000 randomly generated articles
    When I measure the time to serialize the 1000 articles in JSON
    And I measure the time to deserialize the 1000 JSON articles
    Then JSON serialize throughput must be greater than 10 MB/s
    And JSON deserialize throughput must be greater than 100 MB/s

  @serialization @json @simd
  Scenario: Verification of simd-json usage for parsing
    Given valid JSON data representing an article
    When I deserialize with simd-json
    Then parsing must use SIMD instructions if available
    And the result must be identical to serde_json

  # ==================== BINARY MODE (rkyv) ====================

  @serialization @rkyv
  Scenario Outline: rkyv roundtrip serialization
    Given an article with id "<id>" title "<title>" and price <price>
    When I serialize the article in rkyv mode
    And I deserialize the rkyv data
    Then the deserialized article must have id "<id>"
    And the deserialized article must have title "<title>"
    And the deserialized article must have price <price>

    Examples:
      | id          | title                   | price  |
      | art-001     | First article           | 19.99  |
      | art-002     | Second test article     | 29.50  |
      | art-003     | Third test article      | 99.00  |

  @serialization @rkyv @benchmark
  Scenario: rkyv serialization performance
    Given 1000 randomly generated articles
    When I measure the time to serialize the 1000 articles in rkyv
    And I measure the time to deserialize the 1000 rkyv articles
    Then rkyv serialize throughput must be greater than 500 MB/s
    And rkyv deserialize throughput must be greater than 1000 MB/s

  @serialization @rkyv @zero-copy
  Scenario: Zero-copy access with rkyv
    Given an article serialized in rkyv
    When I access the data in zero-copy mode
    Then no memory allocation must be performed
    And I must be able to read the title without deserializing

  # ==================== COMPARISON JSON vs RKYV ====================

  @serialization @comparison
  Scenario: Data size comparison
    Given an article with id "test-size" title "Comparative size test" and price 42.50
    When I serialize in JSON
    And I serialize in rkyv
    Then rkyv size must be less than or equal to JSON size

  @serialization @comparison @benchmark
  Scenario: Comparative JSON vs rkyv benchmark
    Given 10000 randomly generated articles
    When I benchmark JSON serialization on 10000 articles
    And I benchmark rkyv serialization on 10000 articles
    Then rkyv serialize must be at least 5x faster than JSON serialize
    And rkyv deserialize must be at least 3x faster than JSON deserialize

  # ==================== MODE SELECTION ====================

  @serialization @mode-selection
  Scenario Outline: Mode selection via Accept header
    When I receive an Accept header "<accept>"
    Then the selected mode must be "<mode>"

    Examples:
      | accept                      | mode   |
      | application/json            | Json   |
      | application/octet-stream    | Binary |
      | application/x-rkyv          | Binary |
      | text/html                   | Json   |
      | */*                         | Json   |

  @serialization @content-type
  Scenario Outline: Content-Type according to mode
    Given the serialization mode "<mode>"
    Then the content-type must be "<content_type>"

    Examples:
      | mode   | content_type             |
      | Json   | application/json         |
      | Binary | application/octet-stream |

  # ==================== ERROR HANDLING ====================

  @serialization @errors @json
  Scenario: Invalid JSON error handling
    Given malformed JSON data "{ invalid json"
    When I attempt to deserialize in JSON
    Then a JsonDeserializeError must be returned
    And the message must indicate the error position

  @serialization @errors @rkyv
  Scenario: rkyv corrupted data error handling
    Given random binary data of 100 bytes
    When I attempt to deserialize in rkyv
    Then a RkyvDeserializeError or RkyvValidationError must be returned

  # ==================== HTTP INTEGRATION ====================

  @serialization @http @json
  Scenario: HTTP request with JSON
    Given a Lithair server on port 22000
    When I send a POST request with Content-Type "application/json"
    And the body contains an article in JSON
    Then the response must be in JSON
    And the response Content-Type must be "application/json"

  @serialization @http @rkyv
  Scenario: HTTP request with rkyv
    Given a Lithair server on port 22001
    When I send a POST request with Content-Type "application/octet-stream"
    And the body contains an article in rkyv
    And the Accept header is "application/octet-stream"
    Then the response must be in rkyv binary format
    And the response Content-Type must be "application/octet-stream"

  @serialization @http @negotiation
  Scenario: Automatic content negotiation
    Given a Lithair server on port 22002
    When I send a request with Accept "application/octet-stream, application/json;q=0.5"
    Then the server must respond in rkyv (higher priority)
