# BddOrder — nested line items, status enum, and FK relations.
#
# FK AUTO-JOIN COVERAGE NOTE (verified against lithair-core, June 2026):
# the declarative HTTP handler (`DeclarativeHttpHandler::handle_get` /
# `handle_list`) does NOT expand #[db(fk = ...)] relations — relation
# expansion only exists at the engine level via AutoJoiner::expand +
# RelationRegistry (lithair-core/src/engine/relations.rs). The FK scenario
# below therefore tests what actually exists: the derive-generated
# ModelSpec fk policy driving the engine-level AutoJoiner, with the joined
# data sourced from the real BddUser handler.

@models @order
Feature: Order model with nested lines, status enum and FK relations

  Background:
    Given a fresh BDD order store

  @crud
  Scenario: Order CRUD round-trip with nested lines and status transitions
    When I create order "ord-1" for user "u-100" and product "p-200" with status "pending" and 2 lines at unit price "9.99"
    Then fetching order "ord-1" returns status "pending" and 2 lines
    And order "ord-1" line 1 has product "p-200" and unit price "9.99"
    When I change order "ord-1" status to "shipped"
    Then fetching order "ord-1" returns status "shipped" and 2 lines
    When I delete order "ord-1"
    Then order "ord-1" no longer exists

  @filter
  Scenario: Filtering orders by status enum value
    When I create 8 orders alternating statuses "pending" and "paid"
    Then listing orders with query "status=pending" returns 4 results and total 4
    And listing orders with query "status=shipped" returns 0 results and total 0

  @paging
  Scenario: Paging orders with sort, skip and take
    When I create 8 orders alternating statuses "pending" and "paid"
    Then listing orders with query "sort=id&skip=5&take=2" returns 2 results and total 8

  @replay
  Scenario: Nested decimal line items survive restart and replay
    When I create order "ord-3" for user "u-100" and product "p-1" with status "paid" and 3 lines at unit price "0.10"
    And the order store is restarted
    Then fetching order "ord-3" returns status "paid" and 3 lines
    And order "ord-3" line 1 has product "p-1" and unit price "0.10"

  @pain-point @fk
  Scenario: Engine-level FK auto-join expands user_id into a joined user object
    # user_id uses #[relation(foreign_key = "bdd_users")]. product_id carries
    # #[db(fk = "bdd_products")] (parsed since the issue #122 fix — see the
    # scenario below) but no "bdd_products" source is registered, so the
    # AutoJoiner must leave it unexpanded and pass the original field through
    # untouched.
    Given a fresh BDD user store
    When I create user "u-100" with email "grace@example.com", name "Grace" and role "admin"
    And I create order "ord-2" for user "u-100" and product "p-1" with status "paid" and 1 lines at unit price "5.00"
    And I expand order "ord-2" relations with the users source registered
    Then the order model reports field "user_id" as a foreign key to "bdd_users"
    And the expanded order has a joined "user" object with email "grace@example.com"
    And the expanded order still carries "user_id" equal to "u-100"
    And the expanded order has no joined "product" object

  @fk
  Scenario: The db(fk = ...) attribute marks the field as a foreign key
    # Regression coverage for issue #122 (found by this suite, June 2026):
    # the macro's #[db(...)] parser used to walk proc-macro TokenTrees one by
    # one, so the pair-shaped `fk = "table"` arrived as separate tokens and
    # the FK target was silently dropped (same bug class as issue #75 for
    # #[http(validate = ...)]). parse_db_attributes now splits the token
    # string on commas, same as parse_http_attributes; this scenario asserts
    # the documented behavior of #[db(fk = ...)] and must stay green.
    Given a fresh BDD order store
    Then the order model reports field "product_id" as a foreign key to "bdd_products"
