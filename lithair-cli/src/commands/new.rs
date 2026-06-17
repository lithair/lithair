use std::fs;
use std::path::Path;

use crate::templates;

/// Validate that the project name is safe for use as a directory and Cargo package name.
fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Project name cannot be empty".into());
    }
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return Err(format!("invalid project name: \"{}\"", name));
    }
    if name.starts_with('.') || name.starts_with('-') {
        return Err(format!("invalid project name: \"{}\"", name));
    }
    // Cargo package name: alphanumeric, `-`, `_`
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(format!("invalid project name: \"{}\"", name));
    }
    Ok(())
}

/// Scaffold a new Lithair project at `<base>/<name>`.
pub fn run(name: &str, base: &Path, no_frontend: bool) -> Result<(), String> {
    validate_name(name)?;

    let project_dir = base.join(name);
    if project_dir.exists() {
        return Err(format!("\"{}\" already exists", name));
    }

    let files = templates::standard_project(name, !no_frontend);

    for file in &files {
        let dest = project_dir.join(file.path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create directory {}: {}", parent.display(), e))?;
        }
        fs::write(&dest, &file.content)
            .map_err(|e| format!("failed to write {}: {}", dest.display(), e))?;
    }

    println!("Created project \"{}\" with {} files.", name, files.len());
    println!();
    println!("  cd {}", name);
    println!("  cargo run");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_names() {
        assert!(validate_name("my-app").is_ok());
        assert!(validate_name("cool_project").is_ok());
        assert!(validate_name("app123").is_ok());
    }

    #[test]
    fn invalid_names() {
        assert!(validate_name("").is_err());
        assert!(validate_name("../escape").is_err());
        assert!(validate_name(".hidden").is_err());
        assert!(validate_name("-bad").is_err());
        assert!(validate_name("no spaces").is_err());
        assert!(validate_name("a/b").is_err());
    }

    #[test]
    fn scaffold_creates_files() {
        let tmp = tempfile::tempdir().unwrap();
        run("test-proj", tmp.path(), false).unwrap();

        assert!(tmp.path().join("test-proj/Cargo.toml").exists());
        assert!(tmp.path().join("test-proj/src/main.rs").exists());
        assert!(tmp.path().join("test-proj/src/models/mod.rs").exists());
        assert!(tmp.path().join("test-proj/src/models/item.rs").exists());
        assert!(tmp.path().join("test-proj/src/routes/mod.rs").exists());
        assert!(tmp.path().join("test-proj/src/routes/health.rs").exists());
        assert!(tmp.path().join("test-proj/src/middleware/mod.rs").exists());
        assert!(tmp.path().join("test-proj/frontend/index.html").exists());
        assert!(tmp.path().join("test-proj/data/.gitkeep").exists());
    }

    #[test]
    fn scaffold_no_frontend() {
        let tmp = tempfile::tempdir().unwrap();
        run("api-only", tmp.path(), true).unwrap();

        assert!(tmp.path().join("api-only/src/models/mod.rs").exists());
        assert!(!tmp.path().join("api-only/frontend").exists());
    }

    #[test]
    fn scaffold_rejects_existing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("exists")).unwrap();
        let result = run("exists", tmp.path(), false);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already exists"));
    }

    #[test]
    fn project_name_in_cargo_toml() {
        let tmp = tempfile::tempdir().unwrap();
        run("cool-project", tmp.path(), false).unwrap();

        let content = fs::read_to_string(tmp.path().join("cool-project/Cargo.toml")).unwrap();
        assert!(content.contains("name = \"cool-project\""));
    }

    #[test]
    fn scaffold_wires_model_and_routes() {
        let tmp = tempfile::tempdir().unwrap();
        run("wired", tmp.path(), false).unwrap();

        let main_rs = fs::read_to_string(tmp.path().join("wired/src/main.rs")).unwrap();
        assert!(main_rs.contains("with_model"), "main.rs should wire with_model");
        assert!(main_rs.contains("with_route"), "main.rs should wire with_route");
        assert!(
            main_rs.contains("with_frontend"),
            "main.rs should wire with_frontend when frontend is enabled"
        );

        let item_rs = fs::read_to_string(tmp.path().join("wired/src/models/item.rs")).unwrap();
        assert!(item_rs.contains("DeclarativeModel"), "item.rs should derive DeclarativeModel");
    }

    #[test]
    fn scaffold_no_frontend_omits_with_frontend() {
        let tmp = tempfile::tempdir().unwrap();
        run("no-fe", tmp.path(), true).unwrap();

        let main_rs = fs::read_to_string(tmp.path().join("no-fe/src/main.rs")).unwrap();
        assert!(main_rs.contains("with_model"), "main.rs should still wire with_model");
        assert!(
            !main_rs.contains("with_frontend"),
            "main.rs should NOT contain with_frontend when --no-frontend"
        );
    }

    #[test]
    fn env_uses_lt_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        run("env-test", tmp.path(), false).unwrap();

        let content = fs::read_to_string(tmp.path().join("env-test/.env")).unwrap();
        assert!(content.contains("LT_PORT"));
        assert!(content.contains("LT_HOST"));
        assert!(content.contains("LT_LOG_LEVEL"));
        assert!(content.contains("LT_DATA_DIR"));
        assert!(!content.contains("RS_"));
    }

    /// Guard against scaffold rot: a generated project must depend on the
    /// CURRENT `lithair-core` (not a stale pin) and use the CURRENT public API.
    /// Each assertion below maps to a real way `lithair new` was once broken —
    /// a stale `0.1` dep, an import of the (then-missing) `prelude`, and a route
    /// handler written against an obsolete signature / removed imports.
    #[test]
    fn scaffold_targets_current_version_and_api() {
        let tmp = tempfile::tempdir().unwrap();
        run("fresh", tmp.path(), false).unwrap();
        let root = tmp.path().join("fresh");

        // 1. Cargo.toml pins this CLI's own version (caret), never the old "0.1".
        let cargo = fs::read_to_string(root.join("Cargo.toml")).unwrap();
        let expected = format!("lithair-core = \"{}\"", env!("CARGO_PKG_VERSION"));
        assert!(cargo.contains(&expected), "Cargo.toml should pin {expected}; got:\n{cargo}");
        assert!(!cargo.contains("lithair-core = \"0.1\""), "stale 0.1 pin must be gone");
        assert!(
            !cargo.contains("{{lithair_version}}"),
            "version placeholder must be substituted"
        );

        // 2. Sources use the prelude (which now exists in lithair-core).
        let main_rs = fs::read_to_string(root.join("src/main.rs")).unwrap();
        let item_rs = fs::read_to_string(root.join("src/models/item.rs")).unwrap();
        assert!(main_rs.contains("use lithair_core::prelude::*;"), "main.rs uses the prelude");
        assert!(item_rs.contains("use lithair_core::prelude::*;"), "item.rs uses the prelude");

        // 3. The custom-route handler matches the current with_route signature
        //    (RouteRequest -> RouteResponse) and drops the removed imports that
        //    referenced crates no longer in the scaffold's dependencies.
        let health_rs = fs::read_to_string(root.join("src/routes/health.rs")).unwrap();
        assert!(health_rs.contains("RouteResponse"), "health handler returns RouteResponse");
        assert!(health_rs.contains("RouteRequest"), "health handler takes RouteRequest");
        assert!(!health_rs.contains("http::StatusCode"), "no bare `http` crate dependency");
        assert!(!health_rs.contains("Full<Bytes>"), "no obsolete Full<Bytes> return type");
    }
}
