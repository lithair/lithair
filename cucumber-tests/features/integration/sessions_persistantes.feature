Feature: Lithair Persistent Sessions
  As a web application user
  I want my session to remain active between server restarts
  In order not to have to reconnect constantly

  Background:
    Given a Lithair server with persistent sessions enabled
    And the session store is configured for persistence
    And session cookies are secured

  Scenario: Session creation and persistence
    When a user logs in with valid credentials
    Then a session must be created with a unique ID
    And the session must be persisted in the store
    And a secure cookie must be returned
    And the cookie must have HttpOnly, Secure, SameSite attributes

  Scenario: Automatic reconnection after restart
    When a user has an active session
    And the server restarts
    Then the user must remain connected
    And their session must be reloaded from the persistent store
    And all session data must be intact

  Scenario: Session inactivity timeout
    When a user is inactive for 30 minutes
    Then their session must expire automatically
    And their next request must be treated as anonymous
    And session data must be cleaned up

  Scenario: Multi-user simultaneous management
    When 100 users connect simultaneously
    Then each user must receive a unique session
    And sessions must not conflict
    And the store must handle concurrency without corruption

  Scenario: Session security against hijacking
    When a session is created for an IP address
    And the same session is used from another IP
    Then the session must be invalidated for security
    And the user must be disconnected
    And a security event must be logged

  Scenario: Expired session cleanup
    When 1000 sessions expire
    Then the cleanup process must execute
    And expired sessions must be removed from the store
    And storage space must be freed
    And performance must remain stable
