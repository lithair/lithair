Feature: Basic Test
  To verify that Cucumber infrastructure works

  Scenario: Basic server
    Given a Lithair server is started
    When I perform a GET request on "/health"
    Then the response must be successful
