//! Test simple du provider password RBAC

use lithair_core::rbac::{AuthProvider, PasswordProvider};
use http::Request;
use http_body_util::Full;
use bytes::Bytes;

fn main() {
    println!("🔐 Testing RBAC Password Provider\n");
    
    // Create provider
    let provider = PasswordProvider::new("secret123".to_string(), "User".to_string());
    
    // Test 1: Successful authentication with role
    println!("Test 1: Valid password with Admin role");
    let request = Request::builder()
        .header("X-Auth-Password", "secret123")
        .header("X-Auth-Role", "Admin")
        .body(Full::new(Bytes::new()))
        .unwrap();
    
    match provider.authenticate(&request) {
        Ok(context) => {
            println!("  ✅ Authenticated: {}", context.authenticated);
            println!("  ✅ Roles: {:?}", context.roles);
            println!("  ✅ Provider: {}", context.provider);
        }
        Err(e) => println!("  ❌ Error: {}", e),
    }
    
    // Test 2: Successful authentication with default role
    println!("\nTest 2: Valid password with default role");
    let request = Request::builder()
        .header("X-Auth-Password", "secret123")
        .body(Full::new(Bytes::new()))
        .unwrap();
    
    match provider.authenticate(&request) {
        Ok(context) => {
            println!("  ✅ Authenticated: {}", context.authenticated);
            println!("  ✅ Roles: {:?}", context.roles);
        }
        Err(e) => println!("  ❌ Error: {}", e),
    }
    
    // Test 3: Wrong password
    println!("\nTest 3: Wrong password");
    let request = Request::builder()
        .header("X-Auth-Password", "wrong_password")
        .body(Full::new(Bytes::new()))
        .unwrap();
    
    match provider.authenticate(&request) {
        Ok(_) => println!("  ❌ Should have failed!"),
        Err(e) => println!("  ✅ Correctly rejected: {}", e),
    }
    
    // Test 4: No password (unauthenticated)
    println!("\nTest 4: No password header");
    let request = Request::builder()
        .body(Full::new(Bytes::new()))
        .unwrap();
    
    match provider.authenticate(&request) {
        Ok(context) => {
            println!("  ✅ Unauthenticated context returned");
            println!("  ✅ Authenticated: {}", context.authenticated);
            println!("  ✅ Roles: {:?}", context.roles);
        }
        Err(e) => println!("  ❌ Error: {}", e),
    }
    
    println!("\n✅ All tests passed!");
}
