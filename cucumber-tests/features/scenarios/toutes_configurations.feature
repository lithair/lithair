@doc
Feature: All Possible Lithair Configurations

  # This file contains ALL possible configuration scenarios
  # to validate that Lithair works in ALL use cases

  # ==================== CONFIGURATION 1: API ONLY + PERSISTENCE ====================

  Scenario: API only with disk persistence
    Given a Lithair server with configuration:
      | option              | value                   |
      | mode                | api_only                |
      | persistence         | enabled                 |
      | persistence_path    | /tmp/lithair-api        |
      | port                | 0                       |
      | frontend            | disabled                |
    When I create 10 articles via the API
    Then all articles should be in memory
    And all articles should be on disk in events.raftlog
    And I can restart the server
    And the 10 articles should be reloaded from disk

  # ==================== CONFIGURATION 2: API ONLY WITHOUT PERSISTENCE ====================

  Scenario: API only in in-memory mode (no persistence)
    Given a Lithair server with configuration:
      | option              | value                   |
      | mode                | api_only                |
      | persistence         | disabled                |
      | port                | 0                       |
      | frontend            | disabled                |
    When I create 100 articles via the API
    Then all articles should be in memory only
    And no file should be created on disk
    And performance should be maximum
    When I restart the server
    Then all data should be lost

  # ==================== CONFIGURATION 3: FRONTEND + API + PERSISTENCE ====================

  Scenario: Complete application with HTML/JS frontend
    Given a Lithair server with configuration:
      | option              | value                     |
      | mode                | full_stack                |
      | persistence         | enabled                   |
      | persistence_path    | /tmp/lithair-full         |
      | port                | 3000                      |
      | frontend            | enabled                   |
      | static_dir          | /tmp/lithair-static       |
      | enable_sessions     | true                      |
    When I load the page "/"
    Then I should see the HTML frontend
    And CSS should be loaded
    And JavaScript should work
    When I create an article from the frontend
    Then the article should be visible in the DOM
    And the article should be in the API
    And the article should be persisted on disk

  # ==================== CONFIGURATION 4: PRODUCTION MODE ====================

  Scenario: Production mode with all optimizations
    Given a Lithair server with configuration:
      | option              | value                     |
      | mode                | production                |
      | persistence         | enabled                   |
      | persistence_path    | /var/lib/lithair          |
      | port                | 80                        |
      | tls                 | enabled                   |
      | tls_cert            | /path/to/cert.pem         |
      | tls_key             | /path/to/key.pem          |
      | rate_limiting       | 1000                      |
      | cache               | enabled                   |
      | cache_ttl           | 3600                      |
      | max_connections     | 10000                     |
      | enable_metrics      | true                      |
      | metrics_port        | 9090                      |
    When I make 10000 concurrent requests
    Then all should succeed
    And average latency should be < 50ms
    And rate limiting should block after 1000 req/min
    And Prometheus metrics should be available

  # ==================== CONFIGURATION 5: DEVELOPMENT MODE ====================

  Scenario: Development mode with hot reload and debug
    Given a Lithair server with configuration:
      | option              | value                     |
      | mode                | development               |
      | persistence         | enabled                   |
      | port                | 3000                      |
      | debug               | true                      |
      | hot_reload          | true                      |
      | cors                | *                         |
    When I modify a source file
    Then the server should reload automatically
    And debug logs should be visible
    And CORS should allow all origins

  # ==================== CONFIGURATION 6: DISTRIBUTED CLUSTER ====================

  Scenario: Lithair cluster 5 nodes with replication
    Given a Lithair cluster with configuration:
      | option              | value                     |
      | mode                | cluster                   |
      | node_count          | 5                         |
      | persistence         | enabled                   |
      | replication_factor  | 3                         |
      | consensus           | raft                      |
    When I write 1000 articles on the leader node
    Then data should be replicated on at least 3 nodes
    When the leader fails
    Then a new leader should be elected in < 5 seconds
    And the cluster should continue to function
    And no data should be lost

  # ==================== CONFIGURATION 7: ADMIN PANEL ====================

  Scenario: Administration panel with authentication
    Given a Lithair server with configuration:
      | option              | value                     |
      | mode                | full_stack                |
      | admin_panel         | enabled                   |
      | admin_port          | 8080                      |
      | auth                | jwt                       |
      | admin_user          | admin                     |
      | admin_password      | secret123                 |
    When I log in to the admin panel with "admin" / "secret123"
    Then I should access the dashboard
    And I should see real-time metrics
    And I should be able to manage users
    And I should be able to view logs
    And I should be able to make a backup

  # ==================== CONFIGURATION 8: API + SSO ====================

  Scenario: API with Single Sign-On (Google OAuth)
    Given a Lithair server with configuration:
      | option              | value                           |
      | mode                | api_only                        |
      | auth                | oauth2                          |
      | oauth_provider      | google                          |
      | oauth_client_id     | xxx.apps.googleusercontent.com  |
      | oauth_client_secret | secret                          |
      | oauth_redirect_uri  | http://localhost:3000/callback  |
    When a user logs in with Google
    Then they should receive a JWT token
    And the token should be valid
    And the user should access the protected API

  # ==================== CONFIGURATION 9: MICROSERVICES ====================

  Scenario: Lithair in microservice mode
    Given multiple Lithair servers:
      | service         | port | config                    |
      | users-service   | 3001 | api_only, persistence     |
      | articles-service| 3002 | api_only, persistence     |
      | auth-service    | 3003 | api_only, in-memory       |
    When I make a request to users-service
    Then users-service should respond
    When users-service calls articles-service
    Then inter-service communication should work

  # ==================== CONFIGURATION 10: TEST MODE ====================

  Scenario: Configuration for automated tests
    Given a Lithair server with configuration:
      | option              | value                     |
      | mode                | test                      |
      | persistence         | disabled                  |
      | port                | 0                         |
      | fixtures            | enabled                   |
      | fixtures_path       | tests/fixtures            |
      | reset_on_restart    | true                      |
    When I run automated tests
    Then the server should start in < 100ms
    And fixtures should be loaded
    And after each test, the DB should be reset

  # ==================== CONFIGURATION 11: WEBSOCKETS + API ====================

  Scenario: REST API + real-time WebSockets
    Given a Lithair server with configuration:
      | option              | value                     |
      | mode                | full_stack                |
      | websockets          | enabled                   |
      | ws_port             | 3001                      |
    When a client connects via WebSocket
    Then the connection should be established
    When I create an article via the API
    Then all WebSocket clients should receive the notification in real-time

  # ==================== CONFIGURATION 12: MINIMAL CONFIGURATION ====================

  Scenario: Minimal configuration (defaults)
    Given a Lithair server without explicit configuration
    When I start the server
    Then it should use default values:
      | option              | default value             |
      | mode                | full_stack                |
      | persistence         | enabled                   |
      | port                | 8080                      |
      | frontend            | enabled                   |
    And the server should function normally

  # ==================== META-TEST: COMPLETE COMPILATION ====================

  Scenario: Compilation and startup of final user server
    Given the complete Lithair source code
    When a user runs "cargo build --release"
    Then compilation should succeed without errors
    And a "lithair" binary should be created
    When the user runs "./target/release/lithair --config prod.toml"
    Then the server should start successfully
    And all routes should be accessible
    And persistence should work
    And metrics should be available
    And the server should handle 1000+ req/s
