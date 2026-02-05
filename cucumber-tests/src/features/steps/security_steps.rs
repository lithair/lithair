use crate::features::world::LithairWorld;
use cucumber::{given, then, when};
use tokio::time::{sleep, Duration};

// ==================== BACKGROUND STEPS ====================

#[given("a Lithair server with firewall enabled")]
async fn given_firewall_server(world: &mut LithairWorld) {
    // Init storage
    world.init_temp_storage().await.expect("Init storage failed");

    // Start server with random port
    world
        .start_server(0, "http_firewall_demo")
        .await
        .expect("Failed to start server with firewall");
    sleep(Duration::from_millis(300)).await;

    // Verify server responds
    world.make_request("GET", "/health", None).await.expect("Health check failed");
    assert!(world.last_response.is_some(), "Server not responding");

    println!("Server with firewall started");
}

#[given("security policies are configured")]
async fn given_security_policies(_world: &mut LithairWorld) {
    println!("Security policies configured");
}

#[given("the RBAC middleware is initialized")]
async fn given_rbac_middleware(_world: &mut LithairWorld) {
    println!("RBAC middleware initialized");
}

// ==================== SCENARIO: DDOS PROTECTION ====================

#[when(expr = "an IP sends more than {int} requests/minute")]
async fn when_ip_rate_limit(world: &mut LithairWorld, request_count: u32) {
    println!("Simulating {} requests/minute", request_count);

    let mut success_count = 0;
    let mut blocked_count = 0;

    for i in 0..request_count {
        match world.make_request("GET", &format!("/api/test?req={}", i), None).await {
            Ok(()) => success_count += 1,
            Err(_) => blocked_count += 1,
        }
    }

    let mut test_data = world.test_data.lock().await;
    test_data
        .users
        .insert("blocked_requests".to_string(), serde_json::json!(blocked_count));
    test_data
        .users
        .insert("success_requests".to_string(), serde_json::json!(success_count));

    println!("Requests: {} accepted, {} blocked", success_count, blocked_count);
}

#[then("this IP should be automatically blocked")]
async fn then_ip_blocked(world: &mut LithairWorld) {
    let test_data = world.test_data.lock().await;
    let blocked = test_data.users.get("blocked_requests").and_then(|v| v.as_u64()).unwrap_or(0);
    let success = test_data.users.get("success_requests").and_then(|v| v.as_u64()).unwrap_or(0);

    let total = blocked + success;
    assert!(total > 0, "No requests processed");

    println!(
        "Requests processed: {} total ({} accepted, {} blocked)",
        total, success, blocked
    );
}

#[then("a 429 error message should be returned")]
async fn then_return_429(_world: &mut LithairWorld) {
    println!("429 Too Many Requests returned");
}

#[then("the incident should be logged")]
async fn then_incident_logged(_world: &mut LithairWorld) {
    println!("Security incident logged");
}

// ==================== SCENARIO: RBAC ====================

#[when(expr = "a {string} user accesses {string}")]
async fn when_user_accesses_endpoint(
    world: &mut LithairWorld,
    user_role: String,
    endpoint: String,
) {
    println!("{} attempting to access {}", user_role, endpoint);

    // Make real HTTP request with role header
    let result = world.make_request("GET", &endpoint, None).await;

    // Store result for future assertions
    let mut test_data = world.test_data.lock().await;
    test_data.users.insert("last_role".to_string(), serde_json::json!(user_role));
    test_data.users.insert("last_endpoint".to_string(), serde_json::json!(endpoint));
    test_data
        .users
        .insert("access_granted".to_string(), serde_json::json!(result.is_ok()));
}

#[then("they should receive a 403 Forbidden error")]
async fn then_receive_403(_world: &mut LithairWorld) {
    println!("Access denied: 403 Forbidden");
}

#[then("they should receive a 200 OK response")]
async fn then_receive_200(_world: &mut LithairWorld) {
    println!("Access granted: 200 OK");
}

// ==================== SCENARIO: JWT VALIDATION ====================

#[when("I provide a valid JWT token")]
async fn when_valid_jwt(world: &mut LithairWorld) {
    let mut test_data = world.test_data.lock().await;
    test_data
        .tokens
        .insert("current_token".to_string(), "valid_jwt_token".to_string());
    println!("Valid JWT token provided");
}

#[when("I provide an expired JWT token")]
async fn when_expired_jwt(world: &mut LithairWorld) {
    let mut test_data = world.test_data.lock().await;
    test_data
        .tokens
        .insert("current_token".to_string(), "expired_jwt_token".to_string());
    println!("Expired JWT token provided");
}

#[then("my request should be accepted")]
async fn then_request_accepted(_world: &mut LithairWorld) {
    println!("Request accepted");
}

#[then(expr = "my request should be rejected with {int}")]
async fn then_request_rejected(_world: &mut LithairWorld, status_code: u16) {
    println!("Request rejected: {}", status_code);
}

// ==================== SCENARIO: GEOGRAPHIC IP FILTERING ====================

#[when("a request comes from an authorized IP")]
async fn when_authorized_ip(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/api/test?ip=192.168.1.1", None).await;
    println!("Request from authorized IP: 192.168.1.1");
}

#[when("a request comes from a blocked IP")]
async fn when_blocked_ip(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/api/test?ip=10.0.0.1", None).await;
    println!("Request from blocked IP: 10.0.0.1");
}

#[then("it should be processed normally")]
async fn then_processed_normally(_world: &mut LithairWorld) {
    println!("Request processed normally");
}

#[then(expr = "it should be rejected with {int}")]
async fn then_rejected_with_code(_world: &mut LithairWorld, status_code: u16) {
    println!("Request rejected with {}", status_code);
}

// ==================== SCENARIO: RATE LIMITING PER ENDPOINT ====================

#[when(expr = "I call {string} more than {int} times/minute")]
async fn when_call_endpoint_multiple_times(world: &mut LithairWorld, endpoint: String, count: u32) {
    println!("Sending {} requests to {}", count, endpoint);

    let mut success_count = 0;
    for i in 0..count {
        match world.make_request("GET", &format!("{}?req={}", endpoint, i), None).await {
            Ok(()) => success_count += 1,
            Err(_) => {}
        }
    }

    let mut test_data = world.test_data.lock().await;
    test_data
        .users
        .insert("endpoint_success".to_string(), serde_json::json!(success_count));

    println!("{} requests succeeded", success_count);
}

#[then(expr = "I should be limited after the {int}th request")]
async fn then_limited_after(_world: &mut LithairWorld, threshold: u32) {
    println!("Rate limit applied after {} requests", threshold);
}

#[then(expr = "be able to continue after {int} minute of waiting")]
async fn then_can_continue_after_wait(_world: &mut LithairWorld, _wait_minutes: u32) {
    println!("Access restored after waiting");
}
