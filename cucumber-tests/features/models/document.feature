# BddDocument — large content blobs interacting with byte-budget retention.
#
# Documents carry >100KB content strings. The model pins id/title/version and
# declares #[retention(memory = 999999)]; budget scenarios tighten the byte
# budget via the LT_BDDDOCUMENT_MEMORY_MAX_MB env override. Content integrity
# is verified with CRC32 checksums recorded at creation time.

@models @document
Feature: Document model with large blobs, versioning and tags

  Background:
    Given a fresh BDD document store

  @crud
  Scenario: Document CRUD round-trip with a blob over 100KB
    When I create document "doc-1" titled "Spec" version 1 with 150 KB of content and tags "rust,framework"
    Then fetching document "doc-1" returns title "Spec" and version 1
    And the content of document "doc-1" is intact and 153600 bytes long
    When I update document "doc-1" to version 2
    Then fetching document "doc-1" returns title "Spec" and version 2
    When I delete document "doc-1"
    Then document "doc-1" no longer exists

  @filter
  Scenario: Filtering documents by tag with the contains operator
    When I create document "doc-a" titled "Alpha" version 1 with 1 KB of content and tags "rust,backend"
    And I create document "doc-b" titled "Beta" version 1 with 1 KB of content and tags "python,backend"
    Then listing documents with query "tags=~rust" returns 1 results and total 1
    And listing documents with query "tags=~backend" returns 2 results and total 2
    And listing documents with query "title=Alpha" returns 1 results and total 1

  @paging
  Scenario: Paging documents with sort, skip and take
    When I create 10 documents of 1 KB each
    Then listing documents with query "sort=id&skip=6&take=2" returns 2 results and total 10

  @pain-point @retention @blob
  Scenario: Large blobs evict under a byte budget and reload intact
    # 15 x 110KB ~= 1.65MB of hot data against a 1MB budget: the oldest
    # documents must be evicted to warm (pinned fields only), and reading an
    # evicted document must reload the full blob from the event store intact.
    Given a fresh BDD document store with a memory budget of 1 MB
    When I create 15 documents of 110 KB each
    Then fewer than 15 documents are fully in memory
    And at least 5 documents are evicted
    And every evicted document reloads intact from the event store

  @pain-point @retention @blob @replay
  Scenario: Evicted large blobs survive restart and replay intact
    Given a fresh BDD document store with a memory budget of 1 MB
    When I create 15 documents of 110 KB each
    And the document store is restarted
    Then all 15 documents are accessible with intact content
    And fewer than 15 documents are fully in memory

  @retention
  Scenario: A max_mb-only retention annotation enforces the byte budget
    # Regression test for issue #121: DeclarativeHttpHandler::new used to
    # construct the RetentionLayer only when retention_config.memory_count was
    # Some, so a model annotated with ONLY #[retention(max_mb = 1)] got no
    # retention at all. The gate now uses RetentionConfig::is_configured()
    # (any of count/duration/budget), so the budget-only annotation below
    # must evict.
    Given a fresh BDD budget-only document store
    When I create 5 budget-only documents of 500 KB each
    Then at most 2 budget-only documents are fully in memory
