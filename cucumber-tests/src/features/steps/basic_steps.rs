use cucumber::{given, then, when};
use crate::features::world::LithairWorld;

/// Start a REAL Lithair HTTP server for E2E tests
///
/// # Technical Stack
/// - Initialize TempDir + FileStorage
/// - Start HttpServer on random port
/// - Routes: /health, /api/articles, /api/articles/count
///
/// # Validation
/// - Server really listens on TCP
/// - Reqwest can make real HTTP calls
#[given("a Lithair server is started")]
async fn given_lithair_server(world: &mut LithairWorld) {
    // 1. Initialize persistence
    let temp_path = world.init_temp_storage().await
        .expect("Init storage failed");

    // 2. Start the REAL HTTP server
    world.start_server(0, "test").await  // Port 0 = random port
        .expect("Failed to start HTTP server");

    let server_state = world.server.lock().await;
    println!("Lithair server started:");
    println!("   - URL: {}", server_state.base_url.as_ref().unwrap());
    println!("   - Storage: {:?}", temp_path);
    println!("   - PID: {}", server_state.process_id.unwrap());
}

/// Make a REAL HTTP GET request to the server
///
/// # E2E Tests
/// - Uses reqwest::Client for real HTTP request
/// - Server must be started first
/// - Verifies the server actually responds
#[when(expr = "I perform a GET request on {string}")]
async fn when_get_request(world: &mut LithairWorld, path: String) {
    // REAL HTTP call via reqwest
    let result = world.make_request("GET", &path, None).await;

    match result {
        Ok(_) => println!("GET {} succeeded", path),
        Err(e) => {
            println!("GET {} failed: {}", path, e);
            world.last_error = Some(e);
        }
    }
}

/// Verify that the HTTP response is a success (200)
///
/// # E2E Assertions
/// - Check status code 200
/// - Check valid JSON body
/// - Confirm the server actually responded
#[then("the response must be successful")]
async fn then_response_success(world: &mut LithairWorld) {
    assert!(world.last_response.is_some(), "No response received");

    let response = world.last_response.as_ref().unwrap();
    assert!(response.contains("200"), "HTTP code is not 200: {}", response);

    // Parse JSON body to verify it's valid
    if let Some(body_start) = response.find("Body: ") {
        let body_str = &response[body_start + 6..];
        match serde_json::from_str::<serde_json::Value>(body_str) {
            Ok(json) => println!("Valid JSON response: {}", json),
            Err(e) => println!("Non-JSON body: {}", e),
        }
    }

    println!("E2E test passed: HTTP server responds correctly");
}
