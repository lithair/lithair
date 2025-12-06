Feature: Enterprise Security
  As a system administrator
  I want Lithair to provide advanced protections
  In order to secure my applications against threats

  Background:
    Given a Lithair server with firewall enabled
    And security policies are configured
    And the RBAC middleware is initialized

  Scenario: DDoS attack protection
    When an IP sends more than 100 requests/minute
    Then this IP should be automatically blocked
    And a 429 error message should be returned
    And the incident should be logged

  Scenario: Role-based access control (RBAC)
    When a "Customer" user accesses "/admin"
    Then they should receive a 403 Forbidden error
    When an "Admin" user accesses "/admin"
    Then they should receive a 200 OK response

  Scenario: JWT token validation
    When I provide a valid JWT token
    Then my request should be accepted
    When I provide an expired JWT token
    Then my request should be rejected with 401

  Scenario: Geographic IP filtering
    When a request comes from an authorized IP
    Then it should be processed normally
    When a request comes from a blocked IP
    Then it should be rejected with 403

  Scenario: Rate limiting per endpoint
    When I call "/api/sensitive" more than 10 times/minute
    Then I should be limited after the 10th request
    And be able to continue after 1 minute of waiting
