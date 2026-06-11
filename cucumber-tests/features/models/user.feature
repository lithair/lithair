# BddUser — timezone-aware timestamps, optional fields, enum roles,
# validated unique emails.
#
# Email uniqueness is enforced by check_unique_constraints, which runs on the
# HTTP create/update paths and on submit_admin_edit. The replication-style
# insert (apply_replicated_item) intentionally bypasses validation, so the
# uniqueness scenario exercises the public submit_admin_edit path.

@models @user
Feature: User model with timestamps, optional bio, role enum and unique email

  Background:
    Given a fresh BDD user store

  @crud
  Scenario: User CRUD round-trip with enum role and optional bio
    When I create user "u-1" with email "alice@example.com", name "Alice", role "admin", bio "Rustacean" and created at "2026-03-01T12:34:56.789Z"
    Then fetching user "u-1" returns email "alice@example.com", role "admin" and bio "Rustacean"
    When I change user "u-1" role to "viewer"
    Then fetching user "u-1" returns email "alice@example.com", role "viewer" and bio "Rustacean"
    When I delete user "u-1"
    Then user "u-1" no longer exists

  @optional
  Scenario: An absent optional bio stays null through the round-trip
    When I create user "u-2" with email "bob@example.com", name "Bob" and role "viewer"
    Then fetching user "u-2" returns a null bio
    And fetching user "u-2" returns email "bob@example.com", role "viewer" and no bio

  @validation
  Scenario: Email validation rejects malformed addresses
    When I validate a user with email "not-an-email"
    Then the user validation fails mentioning "email"
    When I validate a user with email "carol@example.com"
    Then the user validation succeeds

  @pain-point @unique
  Scenario: Changing an email to an existing one violates the unique constraint
    When I create user "u-3" with email "dave@example.com", name "Dave" and role "editor"
    And I create user "u-4" with email "erin@example.com", name "Erin" and role "viewer"
    And I try to change user "u-4" email to "dave@example.com"
    Then the user edit is rejected with a unique constraint error on "email"
    And fetching user "u-4" returns email "erin@example.com", role "viewer" and no bio

  @pain-point @timestamp @replay
  Scenario: Timestamp fidelity survives restart and replay
    # DateTime<Utc> with sub-second precision must come back bit-identical
    # after the event store is replayed — no timezone drift, no precision loss.
    When I create user "u-5" with email "frank@example.com", name "Frank", role "admin", bio "Ops" and created at "2026-03-01T12:34:56.789Z"
    And the user store is restarted
    Then user "u-5" has created_at exactly "2026-03-01T12:34:56.789Z"
    And fetching user "u-5" returns email "frank@example.com", role "admin" and bio "Ops"

  @filter @paging
  Scenario: Filtering users by role enum and paging
    When I create 10 users with roles alternating "admin" and "viewer"
    Then listing users with query "role=admin" returns 5 results and total 5
    And listing users with query "role=editor" returns 0 results and total 0
    And listing users with query "sort=id&skip=2&take=3" returns 3 results and total 10
