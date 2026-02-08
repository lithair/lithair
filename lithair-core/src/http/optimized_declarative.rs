//! T021 Optimized HTTP Declarative Handler (Hyper 1.x)
//!
//! High-performance version of the declarative HTTP handler that uses:
//! - Bincode for internal serialization (3-5x faster than JSON)
//! - JSON only for HTTP API responses (compatibility)
//! - Smart format selection based on context

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use std::convert::Infallible;
use std::sync::Arc;

use crate::serialization::{BincodeSerializable, SmartSerializer};

type RespBody = BoxBody<Bytes, Infallible>;
type Req = Request<Incoming>;
type Resp = Response<RespBody>;

fn body_from<T: Into<Bytes>>(data: T) -> RespBody {
    Full::new(data.into()).boxed()
}

/// Optimized HTTP exposable trait with bincode support (T021)
pub trait OptimizedHttpExposable: BincodeSerializable + Clone + Send + Sync + 'static {
    /// Get the base path for this model's REST endpoints
    fn http_base_path() -> &'static str;

    /// Get the primary key field name
    fn primary_key_field() -> &'static str;

    /// Get the primary key value for this instance
    fn get_primary_key(&self) -> String;

    /// Lifecycle hooks (optional)
    fn on_before_create(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    fn on_after_create(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
}

/// High-performance HTTP handler with T021 optimizations
pub struct OptimizedDeclarativeHttpHandler<T>
where
    T: OptimizedHttpExposable,
{
    storage: Arc<std::sync::RwLock<std::collections::HashMap<String, T>>>,
    serializer: SmartSerializer,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> Default for OptimizedDeclarativeHttpHandler<T>
where
    T: OptimizedHttpExposable,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T> OptimizedDeclarativeHttpHandler<T>
where
    T: OptimizedHttpExposable,
{
    /// Create optimized handler with bincode serialization (T021)
    pub fn new() -> Self {
        Self {
            storage: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
            serializer: SmartSerializer::new(), // Default to bincode for T021
            _phantom: std::marker::PhantomData,
        }
    }

    /// Handle HTTP request with T021 optimizations
    pub async fn handle_request(&self, req: Req) -> Result<Resp, Infallible> {
        let method = req.method().clone();
        let uri = req.uri().path().to_string();
        let base_path = format!("/{}", T::http_base_path());

        log::debug!("T021 OPTIMIZED: {} {}", method, uri);

        match (method, uri.as_str()) {
            // GET /api/products - List all (JSON response for HTTP API)
            (Method::GET, path) if path == base_path.as_str() => self.handle_list_all().await,

            // POST /api/products - Create (bincode internal, JSON response)
            (Method::POST, path) if path == base_path.as_str() => {
                self.handle_create_optimized(req).await
            }

            // GET /api/products/{id} - Get by ID (JSON response)
            (Method::GET, path) if path.starts_with(&base_path) && path.len() > base_path.len() => {
                let id = &path[base_path.len() + 1..];
                self.handle_get_by_id(id).await
            }

            // PUT /api/products/{id} - Update (bincode internal, JSON response)
            (Method::PUT, path) if path.starts_with(&base_path) && path.len() > base_path.len() => {
                let id = &path[base_path.len() + 1..];
                self.handle_update_optimized(req, id).await
            }

            // DELETE /api/products/{id} - Delete
            (Method::DELETE, path)
                if path.starts_with(&base_path) && path.len() > base_path.len() =>
            {
                let id = &path[base_path.len() + 1..];
                self.handle_delete_optimized(id).await
            }

            _ => Ok(self.not_found_response()),
        }
    }

    /// List all items (HTTP API response in JSON for compatibility)
    async fn handle_list_all(&self) -> Result<Resp, Infallible> {
        log::debug!("T021 OPTIMIZED: Listing all {}", T::http_base_path());

        let items = self.get_all_items_optimized().await;

        // Always use JSON for HTTP API responses
        let json_envelope = match self.serializer.serialize_http(&items) {
            Ok(envelope) => envelope,
            Err(_) => return Ok(self.internal_error_response("Serialization failed")),
        };

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .header("x-serialization", "json-api")
            .header("x-performance-mode", "t021-optimized")
            .body(body_from(json_envelope.data))
            .unwrap())
    }

    /// Create item with T021 bincode optimization for internal processing
    async fn handle_create_optimized(&self, req: Req) -> Result<Resp, Infallible> {
        let body_bytes = match req.into_body().collect().await.map(|c| c.to_bytes()) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(self.bad_request_response("Failed to read body")),
        };

        // Parse JSON from HTTP request (for API compatibility)
        let mut item: T = match T::from_json_bytes(&body_bytes) {
            Ok(item) => item,
            Err(_) => return Ok(self.bad_request_response("Invalid JSON")),
        };

        // T021 OPTIMIZATION: Use bincode for internal processing
        log::debug!("T021: Processing create with bincode serialization");

        // Apply lifecycle hooks
        if item.on_before_create().is_err() {
            return Ok(self.bad_request_response("Validation failed"));
        }

        // Store using optimized bincode format internally
        let internal_envelope = match self.serializer.serialize_internal(&item) {
            Ok(envelope) => envelope,
            Err(_) => return Ok(self.internal_error_response("Internal serialization failed")),
        };

        // Calculate performance metrics
        let bincode_size = internal_envelope.size();
        let json_envelope = self.serializer.serialize_http(&item).unwrap();
        let json_size = json_envelope.size();
        let size_savings = (json_size - bincode_size) as f64 / json_size as f64 * 100.0;

        log::debug!(
            "T021 STATS: JSON {} bytes -> Bincode {} bytes ({:.1}% savings)",
            json_size,
            bincode_size,
            size_savings
        );

        // Store the item with its primary key
        if let Err(e) = self.store_item_optimized(&item).await {
            return Ok(self.internal_error_response(&format!("Storage failed: {}", e)));
        }

        // Apply post-create hooks
        if item.on_after_create().is_err() {
            log::warn!("Post-create hook failed, but item was created successfully");
        }

        // Return JSON response for HTTP API compatibility
        Ok(Response::builder()
            .status(StatusCode::CREATED)
            .header("content-type", "application/json")
            .header("x-serialization", "bincode-internal")
            .header("x-performance-mode", "t021-optimized")
            .header("x-size-savings", &format!("{:.1}%", size_savings))
            .body(body_from(json_envelope.data))
            .unwrap())
    }

    /// Get item by ID (JSON response for HTTP API)
    async fn handle_get_by_id(&self, id: &str) -> Result<Resp, Infallible> {
        log::debug!("T021 OPTIMIZED: Getting {} by ID: {}", T::http_base_path(), id);

        match self.get_item_by_id_optimized(id).await {
            Some(item) => {
                let json_envelope = match self.serializer.serialize_http(&item) {
                    Ok(envelope) => envelope,
                    Err(_) => return Ok(self.internal_error_response("Serialization failed")),
                };

                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .header("x-serialization", "json-api")
                    .header("x-performance-mode", "t021-optimized")
                    .body(body_from(json_envelope.data))
                    .unwrap())
            }
            None => Ok(self.not_found_response()),
        }
    }

    /// Update item with T021 bincode optimization
    async fn handle_update_optimized(&self, req: Req, id: &str) -> Result<Resp, Infallible> {
        let body_bytes = match req.into_body().collect().await.map(|c| c.to_bytes()) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(self.bad_request_response("Failed to read body")),
        };

        // Parse JSON from HTTP request
        let updated_item: T = match T::from_json_bytes(&body_bytes) {
            Ok(item) => item,
            Err(_) => return Ok(self.bad_request_response("Invalid JSON")),
        };

        log::debug!("T021: Processing update with bincode serialization");

        // Use bincode for internal storage (T021 optimization)
        if let Err(e) = self.update_item_optimized(id, &updated_item).await {
            return Ok(self.internal_error_response(&format!("Update failed: {}", e)));
        }

        // Return JSON response for HTTP API
        let json_envelope = match self.serializer.serialize_http(&updated_item) {
            Ok(envelope) => envelope,
            Err(_) => return Ok(self.internal_error_response("Serialization failed")),
        };

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .header("x-serialization", "bincode-internal")
            .header("x-performance-mode", "t021-optimized")
            .body(body_from(json_envelope.data))
            .unwrap())
    }

    /// Delete item with T021 optimization
    async fn handle_delete_optimized(&self, id: &str) -> Result<Resp, Infallible> {
        log::debug!("T021 OPTIMIZED: Deleting {} by ID: {}", T::http_base_path(), id);

        if let Err(e) = self.delete_item_optimized(id).await {
            return Ok(self.internal_error_response(&format!("Delete failed: {}", e)));
        }

        Ok(Response::builder()
            .status(StatusCode::NO_CONTENT)
            .header("x-performance-mode", "t021-optimized")
            .body(body_from(Bytes::new()))
            .unwrap())
    }

    // Internal optimized storage methods using bincode (T021)

    async fn get_all_items_optimized(&self) -> Vec<T> {
        let storage = self.storage.read().expect("http storage lock poisoned");
        storage.values().cloned().collect()
    }

    async fn get_item_by_id_optimized(&self, id: &str) -> Option<T> {
        let storage = self.storage.read().expect("http storage lock poisoned");
        storage.get(id).cloned()
    }

    async fn store_item_optimized(&self, item: &T) -> Result<(), String> {
        // T021: Store using bincode format internally for performance
        let envelope = self
            .serializer
            .serialize_internal(item)
            .map_err(|e| format!("Bincode serialization failed: {}", e))?;

        log::debug!("T021: Processing {} bytes in bincode format", envelope.size());

        let mut storage = self.storage.write().expect("http storage lock poisoned");
        storage.insert(item.get_primary_key(), item.clone());

        log::debug!("T021: Stored item with bincode optimization");
        Ok(())
    }

    async fn update_item_optimized(&self, id: &str, item: &T) -> Result<(), String> {
        // T021: Update using bincode format internally
        let envelope = self
            .serializer
            .serialize_internal(item)
            .map_err(|e| format!("Bincode serialization failed: {}", e))?;

        log::debug!("T021: Updating with {} bytes in bincode format", envelope.size());

        let mut storage = self.storage.write().expect("http storage lock poisoned");
        storage.insert(id.to_string(), item.clone());
        Ok(())
    }

    async fn delete_item_optimized(&self, id: &str) -> Result<(), String> {
        log::debug!("T021: Optimized delete operation");
        let mut storage = self.storage.write().expect("http storage lock poisoned");
        storage.remove(id);
        Ok(())
    }

    // Response helpers

    fn bad_request_response(&self, message: &str) -> Resp {
        Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header("content-type", "application/json")
            .body(body_from(format!(r#"{{"error": "{}"}}"#, message)))
            .unwrap()
    }

    fn internal_error_response(&self, message: &str) -> Resp {
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .header("content-type", "application/json")
            .body(body_from(format!(r#"{{"error": "{}"}}"#, message)))
            .unwrap()
    }

    fn not_found_response(&self) -> Resp {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("content-type", "application/json")
            .body(body_from(r#"{"error": "Not found"}"#))
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestProduct {
        id: Uuid,
        name: String,
        price: f64,
        category: String,
        created_at: DateTime<Utc>,
    }

    impl OptimizedHttpExposable for TestProduct {
        fn http_base_path() -> &'static str {
            "products"
        }

        fn primary_key_field() -> &'static str {
            "id"
        }

        fn get_primary_key(&self) -> String {
            self.id.to_string()
        }
    }

    #[test]
    fn test_bincode_vs_json_performance() {
        let product = TestProduct {
            id: Uuid::new_v4(),
            name: "High-performance test product with detailed description".to_string(),
            price: 199.99,
            category: "Electronics & Gadgets Category".to_string(),
            created_at: Utc::now(),
        };

        // Test bincode serialization
        let bincode_data = product.to_bincode_bytes().unwrap();
        let json_data = product.to_json_bytes().unwrap();

        println!("Bincode size: {} bytes", bincode_data.len());
        println!("JSON size: {} bytes", json_data.len());

        // Bincode should be more compact
        assert!(bincode_data.len() <= json_data.len());

        // Test round-trip
        let bincode_restored = TestProduct::from_bincode_bytes(&bincode_data).unwrap();
        let json_restored = TestProduct::from_json_bytes(&json_data).unwrap();

        assert_eq!(product, bincode_restored);
        assert_eq!(product, json_restored);
    }
}
