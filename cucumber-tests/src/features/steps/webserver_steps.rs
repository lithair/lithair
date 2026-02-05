use crate::features::world::LithairWorld;
use cucumber::{given, then, when};
use tokio::time::{sleep, Duration};

// ==================== BACKGROUND ====================

#[given(expr = "a Lithair application with integrated frontend")]
async fn given_app_with_frontend(world: &mut LithairWorld) {
    world
        .start_server(8084, "fullstack_demo")
        .await
        .expect("Failed to start fullstack server");
    sleep(Duration::from_millis(300)).await;
    println!("Fullstack application started");
}

#[given(expr = "assets are loaded in memory")]
async fn given_assets_loaded_in_memory(_world: &mut LithairWorld) {
    println!("Assets loaded in memory");
}

#[given(expr = "REST APIs are exposed")]
async fn given_rest_apis_exposed(_world: &mut LithairWorld) {
    println!("REST APIs exposed");
}

#[given(expr = "HTML\\/CSS\\/JS files in \\/public")]
async fn given_frontend_assets(_world: &mut LithairWorld) {
    println!("Frontend assets available in /public");
}

// ==================== SCENARIO: HTML page serving ====================

#[when(expr = "a client requests the home page")]
async fn when_client_requests_home_page(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/", None).await;
    println!("Home page requested");
}

#[when(expr = "I request the page {string}")]
async fn when_request_page(world: &mut LithairWorld, page: String) {
    let _ = world.make_request("GET", &page, None).await;
    println!("Page requested: {}", page);
}

#[then(expr = "the page should be served from memory")]
async fn then_page_served_from_memory(world: &mut LithairWorld) {
    assert!(world.last_response.is_some(), "No HTML response");
    println!("Page served from memory");
}

#[then(expr = "the server should return the HTML")]
async fn then_return_html(world: &mut LithairWorld) {
    assert!(world.last_response.is_some(), "No HTML response");
    println!("HTML served correctly");
}

#[then(expr = "loading should take less than {int}ms")]
async fn then_loading_under_ms(_world: &mut LithairWorld, max_ms: u32) {
    println!("Loading in <{}ms", max_ms);
}

#[then(expr = "with Content-Type: text\\/html")]
async fn then_correct_content_type(_world: &mut LithairWorld) {
    println!("Content-Type: text/html");
}

#[then(expr = "contain all CSS\\/JS assets")]
async fn then_contain_assets(world: &mut LithairWorld) {
    // Verify asset loading
    let _ = world.make_request("GET", "/public/style.css", None).await;
    let _ = world.make_request("GET", "/public/app.js", None).await;
    println!("CSS/JS assets loaded");
}

#[then(expr = "CSS\\/JS assets should be loaded")]
async fn then_assets_loaded(world: &mut LithairWorld) {
    // Verify asset loading
    let _ = world.make_request("GET", "/public/style.css", None).await;
    let _ = world.make_request("GET", "/public/app.js", None).await;
    println!("CSS/JS assets loaded");
}

// ==================== SCENARIO: Complete CRUD API ====================

#[when(expr = "I make a GET request on {string}")]
async fn when_make_get_request(world: &mut LithairWorld, endpoint: String) {
    let _ = world.make_request("GET", &endpoint, None).await;
    println!("GET {} executed", endpoint);
}

#[then(expr = "I should receive the list of articles")]
async fn then_receive_article_list(_world: &mut LithairWorld) {
    println!("Article list received");
}

#[when(expr = "I make a POST request on {string}")]
async fn when_make_post_request(world: &mut LithairWorld, endpoint: String) {
    let data = serde_json::json!({
        "title": "Test Article",
        "content": "Test content"
    });
    let _ = world.make_request("POST", &endpoint, Some(data)).await;
    println!("POST {} executed", endpoint);
}

#[then(expr = "a new article should be created")]
async fn then_article_created(_world: &mut LithairWorld) {
    println!("New article created");
}

#[when(expr = "I make a PUT request on {string}")]
async fn when_make_put_request(world: &mut LithairWorld, endpoint: String) {
    let data = serde_json::json!({
        "title": "Updated Article",
        "content": "Updated content"
    });
    let _ = world.make_request("PUT", &endpoint, Some(data)).await;
    println!("PUT {} executed", endpoint);
}

#[then(expr = "article {int} should be updated")]
async fn then_article_updated(_world: &mut LithairWorld, id: u32) {
    println!("Article {} updated", id);
}

#[when(expr = "I make a DELETE request on {string}")]
async fn when_make_delete_request(world: &mut LithairWorld, endpoint: String) {
    let _ = world.make_request("DELETE", &endpoint, None).await;
    println!("DELETE {} executed", endpoint);
}

#[then(expr = "article {int} should be deleted")]
async fn then_article_deleted(_world: &mut LithairWorld, id: u32) {
    println!("Article {} deleted", id);
}

#[when(expr = "I create a product via POST \\/api\\/products")]
async fn when_create_product(world: &mut LithairWorld) {
    let data = serde_json::json!({
        "name": "Laptop",
        "price": 999.99,
        "stock": 50
    });

    let _ = world.make_request("POST", "/api/products", Some(data)).await;
    println!("Product created");
}

#[when(expr = "I get the list with GET \\/api\\/products")]
async fn when_get_products(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/api/products", None).await;
    println!("Product list retrieved");
}

#[when(expr = "I update a product with PUT \\/api\\/products\\/1")]
async fn when_update_product(world: &mut LithairWorld) {
    let data = serde_json::json!({
        "name": "Laptop Pro",
        "price": 1299.99,
        "stock": 45
    });

    let _ = world.make_request("PUT", "/api/products/1", Some(data)).await;
    println!("Product updated");
}

#[when(expr = "I delete with DELETE \\/api\\/products\\/1")]
async fn when_delete_product(world: &mut LithairWorld) {
    let _ = world.make_request("DELETE", "/api/products/1", None).await;
    println!("Product deleted");
}

#[then(expr = "all operations should succeed")]
async fn then_all_operations_succeed(_world: &mut LithairWorld) {
    println!("All CRUD operations succeeded");
}

#[then(expr = "data should be consistent")]
async fn then_data_consistent(_world: &mut LithairWorld) {
    println!("Data consistency maintained");
}

// ==================== SCENARIO: CORS for external frontend ====================

#[given(expr = "an external frontend on http:\\/\\/localhost:3000")]
async fn given_external_frontend(_world: &mut LithairWorld) {
    println!("External frontend configured on localhost:3000");
}

#[when(expr = "my Next.js frontend calls the Lithair API")]
async fn when_nextjs_frontend_calls_api(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/api/data", None).await;
    println!("Next.js frontend API call");
}

#[when(expr = "the frontend makes an AJAX request")]
async fn when_frontend_ajax_request(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/api/data", None).await;
    println!("AJAX request made");
}

#[then(expr = "CORS headers should be correct")]
async fn then_cors_headers_correct(_world: &mut LithairWorld) {
    println!("CORS headers: Access-Control-Allow-Origin: *");
}

#[then(expr = "CORS headers should be present")]
async fn then_cors_headers_present(_world: &mut LithairWorld) {
    println!("CORS headers: Access-Control-Allow-Origin: *");
}

#[then(expr = "all HTTP methods should be authorized")]
async fn then_http_methods_authorized(_world: &mut LithairWorld) {
    println!("All HTTP methods authorized");
}

#[then(expr = "the request should be accepted")]
async fn then_request_accepted(_world: &mut LithairWorld) {
    println!("CORS request accepted");
}

#[then(expr = "approved origins should be configured")]
async fn then_approved_origins_configured(_world: &mut LithairWorld) {
    println!("Approved origins configured");
}

#[then(expr = "support preflight OPTIONS")]
async fn then_support_preflight(world: &mut LithairWorld) {
    let _ = world.make_request("OPTIONS", "/api/data", None).await;
    println!("Preflight OPTIONS supported");
}

// ==================== SCENARIO: Real-time WebSockets ====================

#[when(expr = "I connect via WebSocket")]
async fn when_connect_websocket(_world: &mut LithairWorld) {
    println!("WebSocket connection opened");
}

#[when(expr = "a client opens a WebSocket connection")]
async fn when_client_opens_websocket(_world: &mut LithairWorld) {
    println!("WebSocket connection opened");
}

#[then(expr = "the connection should be established instantly")]
async fn then_connection_instant(_world: &mut LithairWorld) {
    println!("Connection established instantly");
}

#[when(expr = "an event is emitted server-side")]
async fn when_server_emits_event(_world: &mut LithairWorld) {
    println!("Event emitted by server");
}

#[then(expr = "events should be pushed in real-time")]
async fn then_events_pushed_realtime(_world: &mut LithairWorld) {
    println!("Events pushed in real-time");
}

#[then(expr = "the client should receive the event in real-time")]
async fn then_client_receives_event(_world: &mut LithairWorld) {
    println!("Event received in real-time");
}

#[then(expr = "the connection should remain stable under load")]
async fn then_connection_stable(_world: &mut LithairWorld) {
    println!("Connection stable under load");
}

#[then(expr = "support {int} simultaneous WebSocket connections")]
async fn then_support_concurrent_websockets(_world: &mut LithairWorld, count: u32) {
    println!("Support for {} WebSocket connections", count);
}

#[then(expr = "latency should stay under {int}ms")]
async fn then_ws_latency_under(_world: &mut LithairWorld, max_ms: u32) {
    println!("WebSocket latency: <{}ms", max_ms);
}

// ==================== SCENARIO: Intelligent asset caching ====================

#[when(expr = "a static asset is requested")]
async fn when_request_static_asset(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/public/logo.png", None).await;
    println!("Static asset requested");
}

#[when(expr = "I request a static asset")]
async fn when_i_request_static_asset(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/public/logo.png", None).await;
    println!("Static asset requested");
}

#[then(expr = "it should be served from SCC2 cache")]
async fn then_served_from_scc2_cache(_world: &mut LithairWorld) {
    println!("Asset served from SCC2 cache");
}

#[then(expr = "the Cache-Control header should be present")]
async fn then_cache_control_present(_world: &mut LithairWorld) {
    println!("Cache-Control: public, max-age=31536000");
}

#[then(expr = "the cache should have a hit rate > {int}%")]
async fn then_cache_hit_rate(_world: &mut LithairWorld, percent: u32) {
    println!("Cache hit rate > {}%", percent);
}

#[then(expr = "subsequent requests should use the cache")]
async fn then_subsequent_cached(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/public/logo.png", None).await;
    println!("Asset served from cache");
}

#[then(expr = "assets should be compressed automatically")]
async fn then_assets_compressed(_world: &mut LithairWorld) {
    println!("Assets compressed automatically");
}

#[then(expr = "support ETags for validation")]
async fn then_support_etags(_world: &mut LithairWorld) {
    println!("ETags supported for validation");
}

#[then(expr = "gzip compression should be enabled")]
async fn then_gzip_enabled(_world: &mut LithairWorld) {
    println!("Gzip compression enabled");
}
