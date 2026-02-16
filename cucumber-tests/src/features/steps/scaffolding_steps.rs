use cucumber::{given, then, when, World as CucumberWorld};
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Default, CucumberWorld)]
pub struct ScaffoldingWorld {
    pub temp_dir: Option<tempfile::TempDir>,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl ScaffoldingWorld {
    fn base_dir(&self) -> PathBuf {
        self.temp_dir.as_ref().expect("temp_dir not initialized").path().to_path_buf()
    }
}

fn lithair_binary() -> PathBuf {
    // Look for the binary relative to the workspace root
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // cucumber-tests -> workspace root
    path.push("target");
    path.push("debug");
    path.push("lithair");
    path
}

#[given("a clean temporary directory")]
async fn clean_temp_dir(world: &mut ScaffoldingWorld) {
    world.temp_dir = Some(tempfile::tempdir().expect("failed to create temp dir"));
}

#[given(expr = "a directory {string} already exists")]
async fn dir_already_exists(world: &mut ScaffoldingWorld, name: String) {
    let dir = world.base_dir().join(&name);
    std::fs::create_dir_all(&dir).expect("failed to create existing directory");
}

#[when(regex = r#"^I run lithair new "([^"]*)"$"#)]
async fn run_new(world: &mut ScaffoldingWorld, name: String) {
    let output = Command::new(lithair_binary())
        .arg("new")
        .arg(&name)
        .current_dir(world.base_dir())
        .output()
        .expect("failed to execute lithair binary");

    world.exit_code = Some(output.status.code().unwrap_or(-1));
    world.stdout = String::from_utf8_lossy(&output.stdout).to_string();
    world.stderr = String::from_utf8_lossy(&output.stderr).to_string();
}

#[when(regex = r#"^I run lithair new "([^"]*)" --no-frontend$"#)]
async fn run_new_no_frontend(world: &mut ScaffoldingWorld, name: String) {
    let output = Command::new(lithair_binary())
        .arg("new")
        .arg(&name)
        .arg("--no-frontend")
        .current_dir(world.base_dir())
        .output()
        .expect("failed to execute lithair binary");

    world.exit_code = Some(output.status.code().unwrap_or(-1));
    world.stdout = String::from_utf8_lossy(&output.stdout).to_string();
    world.stderr = String::from_utf8_lossy(&output.stderr).to_string();
}

#[then("the command should succeed")]
async fn command_succeeds(world: &mut ScaffoldingWorld) {
    let code = world.exit_code.expect("no exit code");
    assert_eq!(
        code, 0,
        "Expected exit code 0, got {}.\nstdout: {}\nstderr: {}",
        code, world.stdout, world.stderr
    );
}

#[then("the command should fail")]
async fn command_fails(world: &mut ScaffoldingWorld) {
    let code = world.exit_code.expect("no exit code");
    assert_ne!(code, 0, "Expected non-zero exit code, got 0");
}

#[then(expr = "the directory {string} should exist")]
async fn dir_exists(world: &mut ScaffoldingWorld, path: String) {
    let full = world.base_dir().join(&path);
    assert!(full.exists(), "Directory does not exist: {}", full.display());
    assert!(full.is_dir(), "Path is not a directory: {}", full.display());
}

#[then(expr = "the directory {string} should not exist")]
async fn dir_not_exists(world: &mut ScaffoldingWorld, path: String) {
    let full = world.base_dir().join(&path);
    assert!(!full.exists(), "Directory should not exist: {}", full.display());
}

#[then(expr = "the file {string} should exist")]
async fn file_exists(world: &mut ScaffoldingWorld, path: String) {
    let full = world.base_dir().join(&path);
    assert!(full.exists(), "File does not exist: {}", full.display());
    assert!(full.is_file(), "Path is not a file: {}", full.display());
}

#[then(regex = r#"^the file "([^"]*)" should contain "([^"]*)"$"#)]
async fn file_contains(world: &mut ScaffoldingWorld, path: String, needle: String) {
    let full = world.base_dir().join(&path);
    let content =
        std::fs::read_to_string(&full).unwrap_or_else(|_| panic!("Cannot read {}", full.display()));
    assert!(
        content.contains(&needle),
        "File {} does not contain \"{}\". Content:\n{}",
        full.display(),
        needle,
        content
    );
}

#[then(regex = r#"^the file "([^"]*)" should contain '([^']*)'$"#)]
async fn file_contains_single_quote(world: &mut ScaffoldingWorld, path: String, needle: String) {
    let full = world.base_dir().join(&path);
    let content =
        std::fs::read_to_string(&full).unwrap_or_else(|_| panic!("Cannot read {}", full.display()));
    assert!(
        content.contains(&needle),
        "File {} does not contain '{}'. Content:\n{}",
        full.display(),
        needle,
        content
    );
}

#[then(regex = r#"^the file "([^"]*)" should not contain "([^"]*)"$"#)]
async fn file_not_contains(world: &mut ScaffoldingWorld, path: String, needle: String) {
    let full = world.base_dir().join(&path);
    let content =
        std::fs::read_to_string(&full).unwrap_or_else(|_| panic!("Cannot read {}", full.display()));
    assert!(
        !content.contains(&needle),
        "File {} should not contain \"{}\"",
        full.display(),
        needle
    );
}

#[then(expr = "the file {string} should be valid TOML")]
async fn file_valid_toml(world: &mut ScaffoldingWorld, path: String) {
    let full = world.base_dir().join(&path);
    let content =
        std::fs::read_to_string(&full).unwrap_or_else(|_| panic!("Cannot read {}", full.display()));
    content
        .parse::<toml::Table>()
        .unwrap_or_else(|e| panic!("File {} is not valid TOML: {}", full.display(), e));
}

#[then(expr = "the output should contain {string}")]
async fn output_contains(world: &mut ScaffoldingWorld, needle: String) {
    let combined = format!("{}{}", world.stdout, world.stderr);
    assert!(
        combined.contains(&needle),
        "Output does not contain \"{}\". stdout: {}\nstderr: {}",
        needle,
        world.stdout,
        world.stderr
    );
}
