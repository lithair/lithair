Feature: Project Scaffolding
  As a developer
  I want to scaffold a new Lithair project
  So that I can start building quickly with the standard structure

  Scenario: Generate a standard project
    Given a clean temporary directory
    When I run lithair new "my-app"
    Then the command should succeed
    And the directory "my-app" should exist
    And the file "my-app/Cargo.toml" should exist
    And the file "my-app/Cargo.toml" should contain "lithair-core"
    And the file "my-app/Cargo.toml" should be valid TOML
    And the file "my-app/src/main.rs" should exist
    And the file "my-app/src/main.rs" should contain "lithair_core::prelude"
    And the file "my-app/src/models/mod.rs" should exist
    And the file "my-app/src/models/item.rs" should exist
    And the file "my-app/src/routes/mod.rs" should exist
    And the file "my-app/src/routes/health.rs" should exist
    And the file "my-app/src/middleware/mod.rs" should exist
    And the file "my-app/.env.example" should contain "LT_PORT"
    And the file "my-app/.gitignore" should contain "target/"
    And the file "my-app/frontend/index.html" should exist
    And the file "my-app/data/.gitkeep" should exist
    And the file "my-app/README.md" should exist

  Scenario: Generate project without frontend
    Given a clean temporary directory
    When I run lithair new "api-only" --no-frontend
    Then the command should succeed
    And the directory "api-only/src/models" should exist
    And the directory "api-only/frontend" should not exist

  Scenario: Reject invalid project name
    Given a clean temporary directory
    When I run lithair new "../escape"
    Then the command should fail
    And the output should contain "invalid"

  Scenario: Reject existing directory
    Given a clean temporary directory
    And a directory "existing-project" already exists
    When I run lithair new "existing-project"
    Then the command should fail
    And the output should contain "already exists"

  Scenario: Project name used in generated files
    Given a clean temporary directory
    When I run lithair new "cool-project"
    Then the command should succeed
    And the file "cool-project/Cargo.toml" should contain 'name = "cool-project"'
    And the file "cool-project/README.md" should contain "cool-project"

  Scenario: Generated project wires model and routes
    Given a clean temporary directory
    When I run lithair new "wired-app"
    Then the command should succeed
    And the file "wired-app/src/main.rs" should contain "with_model"
    And the file "wired-app/src/main.rs" should contain "with_route"
    And the file "wired-app/src/main.rs" should contain "with_frontend"
    And the file "wired-app/src/models/item.rs" should contain "DeclarativeModel"

  Scenario: Environment variables use LT_ prefix
    Given a clean temporary directory
    When I run lithair new "env-test"
    Then the command should succeed
    And the file "env-test/.env" should contain "LT_PORT"
    And the file "env-test/.env" should contain "LT_HOST"
    And the file "env-test/.env" should contain "LT_LOG_LEVEL"
    And the file "env-test/.env" should contain "LT_DATA_DIR"
    And the file "env-test/.env" should not contain "RS_"
