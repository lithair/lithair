Feature: Complete Web Server
  As a web developer
  I want Lithair to serve complete web applications
  In order to completely replace the traditional stack

  Background:
    Given a Lithair application with integrated frontend
    And assets are loaded in memory
    And REST APIs are exposed

  Scenario: HTML page serving
    When a client requests the home page
    Then the page should be served from memory
    And loading should take less than 10ms
    And contain all CSS/JS assets

  Scenario: Complete CRUD API
    When I make a GET request on "/api/articles"
    Then I should receive the list of articles
    When I make a POST request on "/api/articles"
    Then a new article should be created
    When I make a PUT request on "/api/articles/1"
    Then article 1 should be updated
    When I make a DELETE request on "/api/articles/1"
    Then article 1 should be deleted

  Scenario: CORS for external frontend
    When my Next.js frontend calls the Lithair API
    Then CORS headers should be correct
    And all HTTP methods should be authorized
    And approved origins should be configured

  Scenario: Real-time WebSockets
    When I connect via WebSocket
    Then the connection should be established instantly
    And events should be pushed in real-time
    And the connection should remain stable under load

  Scenario: Intelligent asset caching
    When a static asset is requested
    Then it should be served from SCC2 cache
    And the cache should have a hit rate > 95%
    And assets should be compressed automatically
