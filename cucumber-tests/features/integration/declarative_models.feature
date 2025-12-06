Feature: Lithair Declarative Models
  As an application developer
  I want to use declarative models to automatically generate CRUD APIs
  In order to reduce boilerplate code and ensure consistency

  Background:
    Given a Lithair server with declarative models enabled
    And permissions are configured automatically
    And CRUD routes are generated dynamically

  Scenario: Automatic CRUD route generation
    When I define an Article model with DeclarativeModel
    Then routes GET /articles, POST /articles, PUT /articles/{id}, DELETE /articles/{id} must be created
    And each route must have appropriate permissions
    And the JSON schema must be generated automatically

  Scenario: Permission validation per model
    When a "Contributor" user accesses POST /articles
    Then the request must be accepted with permission "ArticleWrite"
    When an "Anonymous" user accesses POST /articles
    Then the request must be rejected with 403 Forbidden error
    When a "Reporter" user accesses GET /articles
    Then the request must be accepted with permission "ArticleRead"

  Scenario: Automatic entity persistence
    When I create an article via POST /articles
    Then the article must be persisted in the state engine
    And a unique ID must be generated automatically
    And creation metadata must be added

  Scenario: Entity state workflow
    When I create an article with status "Draft"
    And I update it to "Published"
    Then the workflow must respect valid transitions
    And lifecycle hooks must be executed
    And state must be validated before saving

  Scenario: Relations between models
    When I define Article and Comment models
    And Comment references Article
    Then relational routes must be generated
    And /articles/{id}/comments must be accessible
    And reference consistency must be guaranteed

  Scenario: Declarative query performance
    When I perform 1000 GET /articles requests in parallel
    Then all requests must succeed
    And average response time must be less than 10ms
    And memory usage must remain stable
