Feature: Lithair Blog with Frontend

  # Server configuration
  Background:
    Given a Lithair server with options:
      | option              | value                     |
      | port                | 0                         |
      | static_dir          | /tmp/blog-static          |
      | enable_sessions     | true                      |
      | session_store_path  | /tmp/blog-sessions        |
      | enable_admin        | true                      |

  Scenario: HTML frontend is accessible
    When I load the page "/"
    Then I should see HTML
    And the title should be "My Lithair Blog"
    And CSS should be loaded
    And JavaScript should be active

  Scenario: Create an article via the API
    When I POST to "/api/articles" with:
      """json
      {
        "title": "First Article",
        "content": "Content of my article",
        "author": "John Doe"
      }
      """
    Then the response should be 201 Created
    And a unique ID should be generated
    And the article should be persisted in events.raftlog

  Scenario: Frontend displays articles
    Given 3 articles created via the API
    When I load the page "/"
    Then I should see 3 articles in the DOM
    And each article should have a title
    And each article should have a "Read more" link

  Scenario: User session
    When I log in with username "admin" and password "secret"
    Then I should receive a session cookie
    And the cookie should be HttpOnly
    When I load "/admin/dashboard"
    Then I should see the admin dashboard
    And I should NOT see "Login"

  Scenario: Frontend + Backend integrated
    When I load the page "/"
    And I click on "Create an article" (JavaScript)
    And I fill the form with:
      | field   | value               |
      | title   | Article via Frontend|
      | content | Frontend content    |
    And I submit the form
    Then a POST request should be sent to "/api/articles"
    And the article should appear in the list
    And the article should be in memory (StateEngine)
    And the article should be on disk (FileStorage)
