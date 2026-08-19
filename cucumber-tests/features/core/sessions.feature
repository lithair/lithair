Feature: Browser sessions
  As a browser client of a Lithair server configured with with_rbac_config
  I want the login to hand me a session cookie
  So that the cookie alone drives the session-gated API until I log out

  Scenario: Login issues a cookie, the gate accepts it, logout revokes it
    Given a server with RBAC auth routes and session-gated models
    When I POST valid credentials to /auth/login
    Then the response status should be 200
    And the response should set the "session_token" cookie
    When I GET /api/accounts with the session cookie only
    Then the response status should be 200
    When I POST /auth/logout with the session cookie only
    Then the response status should be 200
    And the response should clear the "session_token" cookie
    When I GET /api/accounts with the session cookie only
    Then the response status should be 401

  Scenario: The gate rejects a request without a session
    Given a server with RBAC auth routes and session-gated models
    When I GET /api/accounts without credentials
    Then the response status should be 401

  Scenario: A cross-site logout is rejected and the session survives
    Given a server with RBAC auth routes and session-gated models
    When I POST valid credentials to /auth/login
    Then the response status should be 200
    When I POST /auth/logout with the session cookie from a cross-site page
    Then the response status should be 403
    And the response should not touch any cookie
    When I GET /api/accounts with the session cookie only
    Then the response status should be 200
