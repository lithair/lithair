# BddInvoice — decimal money handling through the declarative pipeline.
#
# Real applications store money as exact decimals, not f64. These scenarios
# verify that rust_decimal::Decimal fields survive the full event-sourced
# lifecycle (create -> persist -> restart-replay -> read) without losing
# precision, and that the production list pipeline (filter/sort/skip/take,
# the same code `GET /api/{model}` runs) behaves on realistic data.

@models @invoice
Feature: Invoice model with decimal money and nested line items

  Background:
    Given a fresh BDD invoice store

  @crud
  Scenario: Invoice CRUD round-trip preserves decimals and nested lines
    When I create invoice "inv-001" in "EUR" with total "1219.99", tax "203.33", due "2026-07-15T00:00:00Z" and 3 lines
    Then fetching invoice "inv-001" returns total "1219.99" and tax "203.33"
    And invoice "inv-001" has 3 lines with unit price "19.99"
    When I update invoice "inv-001" total to "1500.50"
    Then fetching invoice "inv-001" returns total "1500.50" and tax "203.33"
    When I delete invoice "inv-001"
    Then invoice "inv-001" no longer exists

  @list
  Scenario: Listing returns all invoices with an accurate total
    When I create 12 invoices alternating currencies "EUR" and "USD"
    Then listing invoices with query "" returns 12 results and total 12

  @filter
  Scenario: Filtering invoices by currency code
    When I create 12 invoices alternating currencies "EUR" and "USD"
    Then listing invoices with query "currency=EUR" returns 6 results and total 6
    And every listed invoice has "currency" equal to "EUR"

  @filter
  Scenario: Exact-equality filter on a decimal money field
    # Decimal serializes as a JSON string ("19.99"), so the Eq filter does an
    # exact string comparison — no float fuzziness.
    When I create invoice "inv-eq" in "EUR" with total "19.99", tax "0.01", due "2026-07-15T00:00:00Z" and 1 lines
    And I create invoice "inv-other" in "EUR" with total "20.00", tax "0.01", due "2026-07-15T00:00:00Z" and 1 lines
    Then listing invoices with query "total_amount=19.99" returns 1 results and total 1

  @paging
  Scenario: Paging invoices with sort, skip and take
    When I create 12 invoices alternating currencies "EUR" and "USD"
    Then listing invoices with query "sort=invoice_number&skip=4&take=4" returns 4 results and total 12
    And the listed invoice numbers are "INV-00004,INV-00005,INV-00006,INV-00007"
    And the invoice listing reports has_more "true"

  @pain-point @decimal-precision
  Scenario: Decimal precision survives persist, restart and replay
    When I create invoice "inv-prec" in "EUR" with total "19.99", tax "0.01", due "2026-07-15T00:00:00Z" and 1 lines
    And the invoice store is restarted
    Then fetching invoice "inv-prec" returns total "19.99" and tax "0.01"
    And the decimal sum of total and tax for invoice "inv-prec" is exactly "20.00"

  @pain-point @decimal-precision
  Scenario: Classic f64-breaking values stay exact across replay
    # 0.1 + 0.2 != 0.3 in f64 (0.30000000000000004). With Decimal it is exact.
    When I create invoice "inv-f64" in "EUR" with total "0.1", tax "0.2", due "2026-07-15T00:00:00Z" and 1 lines
    And the invoice store is restarted
    Then the decimal sum of total and tax for invoice "inv-f64" is exactly "0.3"
