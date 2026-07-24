use cucumber::{given, then, when, World as CucumberWorld};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Default, CucumberWorld)]
pub struct ScaffoldingWorld {
    pub temp_dir: Option<tempfile::TempDir>,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub app: Option<Child>,
    pub app_binary: Option<PathBuf>,
    pub app_dir: Option<PathBuf>,
    pub port: Option<u16>,
}

impl Drop for ScaffoldingWorld {
    fn drop(&mut self) {
        if let Some(child) = self.app.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl ScaffoldingWorld {
    fn base_dir(&self) -> PathBuf {
        self.temp_dir.as_ref().expect("temp_dir not initialized").path().to_path_buf()
    }
}

fn workspace_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

fn lithair_binary() -> PathBuf {
    let target_dir = workspace_root().join("target");

    // The per-PR gate explicitly builds the current debug CLI. Prefer it so a
    // stale release artifact from an earlier version cannot scaffold the app.
    for profile in &["debug", "ci", "release"] {
        let bin_path = target_dir.join(profile).join("lithair");
        if bin_path.exists() {
            return bin_path;
        }
    }

    target_dir.join("debug").join("lithair")
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("build http client")
}

fn stop_app(world: &mut ScaffoldingWorld) {
    let Some(mut child) = world.app.take() else {
        return;
    };

    // Match the user's Ctrl-C journey so Lithair can flush and shut down
    // gracefully. Fall back to Child::kill if SIGINT is unavailable.
    let interrupted = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .is_ok_and(|status| status.success());
    let mut force_killed = false;
    if !interrupted {
        let _ = child.kill();
        force_killed = true;
    }

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                #[cfg(unix)]
                let expected_signal = {
                    use std::os::unix::process::ExitStatusExt;
                    interrupted && status.signal() == Some(2)
                };
                #[cfg(not(unix))]
                let expected_signal = false;

                assert!(
                    status.success() || expected_signal || force_killed,
                    "generated app exited unsuccessfully: {status}"
                );
                break;
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("generated app did not stop after SIGINT");
            }
            Err(error) => panic!("failed waiting for generated app: {error}"),
        }
    }
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

#[when(expr = "I build generated app {string} against the current workspace")]
async fn build_generated_app(world: &mut ScaffoldingWorld, name: String) {
    let app_dir = world.base_dir().join(&name);
    let manifest = app_dir.join("Cargo.toml");
    let mut cargo_toml = std::fs::read_to_string(&manifest).expect("read generated Cargo.toml");
    cargo_toml.push_str(&format!(
        "\n[patch.crates-io]\nlithair-core = {{ path = {:?} }}\n",
        workspace_root().join("lithair-core")
    ));
    std::fs::write(&manifest, cargo_toml).expect("patch generated app to current workspace");

    // A nested Cargo invocation cannot share the outer test process's target
    // directory: Cargo keeps its artifact lock while this BDD binary runs.
    let target_dir = workspace_root().join("target/golden-path-e2e");
    let output = Command::new("cargo")
        .arg("build")
        .arg("--quiet")
        .arg("--offline")
        .current_dir(&app_dir)
        .env("CARGO_TARGET_DIR", &target_dir)
        .output()
        .expect("build generated application");

    world.exit_code = Some(output.status.code().unwrap_or(-1));
    world.stdout = String::from_utf8_lossy(&output.stdout).to_string();
    world.stderr = String::from_utf8_lossy(&output.stderr).to_string();
    world.app_binary = Some(target_dir.join("debug").join(&name));
    world.app_dir = Some(app_dir);
}

#[when(expr = "I start the generated app on an isolated port")]
async fn start_generated_app(world: &mut ScaffoldingWorld) {
    const START_ATTEMPTS: usize = 3;
    const START_TIMEOUT: Duration = Duration::from_secs(10);

    let app_dir = world.app_dir.as_ref().expect("generated app was not built");
    let app_binary = world.app_binary.as_ref().expect("generated binary path");

    let client = http_client();
    for _ in 1..=START_ATTEMPTS {
        let port = portpicker::pick_unused_port().expect("reserve isolated port");

        // An explicit process variable takes precedence over the generated
        // .env file (dotenvy never overwrites an existing variable).
        let mut child = Command::new(app_binary)
            .current_dir(app_dir)
            .env("LT_PORT", port.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start generated application");

        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            let health = client.get(format!("http://127.0.0.1:{port}/health")).send().await;
            if health.is_ok_and(|response| response.status().is_success()) {
                world.app = Some(child);
                world.port = Some(port);
                return;
            }

            if child.try_wait().expect("poll generated application").is_some() {
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    panic!("generated app did not become healthy after {START_ATTEMPTS} attempts");
}

#[when(expr = "I create item {string} named {string}")]
async fn create_item(world: &mut ScaffoldingWorld, id: String, name: String) {
    let port = world.port.expect("app port");
    let response = http_client()
        .post(format!("http://127.0.0.1:{port}/api/items"))
        .json(&serde_json::json!({"id": id, "name": name, "description": "golden path"}))
        .send()
        .await
        .expect("POST generated model");
    assert!(response.status().is_success(), "item creation failed: {}", response.status());
}

#[when("I stop the generated app")]
async fn stop_generated_app(world: &mut ScaffoldingWorld) {
    stop_app(world);
}

#[then(expr = "fetching item {string} returns name {string}")]
async fn fetch_item(world: &mut ScaffoldingWorld, id: String, name: String) {
    let port = world.port.expect("app port");
    let response = http_client()
        .get(format!("http://127.0.0.1:{port}/api/items/{id}"))
        .send()
        .await
        .expect("GET generated model");
    assert!(response.status().is_success(), "item fetch failed: {}", response.status());
    let item: serde_json::Value = response.json().await.expect("parse fetched item");
    assert_eq!(item["name"], name.as_str(), "fetched item has the wrong name:\n{item}");
}

#[then("the generated item store passes offline verification")]
async fn verify_generated_store(world: &mut ScaffoldingWorld) {
    assert!(world.app.is_none(), "offline verification requires the server to be stopped");
    let store = world.app_dir.as_ref().expect("generated app dir").join("data/items");
    let output = Command::new(lithair_binary())
        .arg("verify")
        .arg(&store)
        .output()
        .expect("run offline verifier");
    assert!(
        output.status.success(),
        "offline verification failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
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
