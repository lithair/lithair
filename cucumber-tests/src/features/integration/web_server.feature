Feature: Complete Web Server
  As a web developer
  I want Lithair to serve complete web applications
  In order to completely replace the traditional stack

  Background:
    Given a Lithair application with integrated frontend
    And the assets are loaded in memory
    And the REST APIs are exposed

  Scenario: Serving HTML pages
    When a client requests the home page
    Then the page must be served from memory
    And loading must take less than 10ms
    And contain all CSS/JS assets

  Scenario: Complete CRUD API
    When I make a GET on "/api/articles"
    Then I must receive the list of articles
    When I make a POST on "/api/articles"
    Then a new article must be created
    When I make a PUT on "/api/articles/1"
    Then article 1 must be updated
    When I make a DELETE on "/api/articles/1"
    Then article 1 must be deleted

  Scenario: CORS for external frontend
    When my Next.js frontend calls the Lithair API
    Then the CORS headers must be correct
    And all HTTP methods must be allowed
    And approved origins must be configured

  Scenario: Real-time WebSockets
    When I connect via WebSocket
    Then the connection must be established instantly
    And events must be pushed in real-time
    And the connection must remain stable under load

  Scenario: Intelligent asset caching
    When a static asset is requested
    Then it must be served from the SCC2 cache
    And the cache must have a hit rate > 95%
    And assets must be compressed automatically
