//! Server-Sent Events (SSE) for real-time model change subscriptions
//!
//! Provides live streaming of create/update/delete events for DeclarativeModel handlers.
//! Uses tokio broadcast channels with heartbeat and lag detection.

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::BodyExt;
use http_body_util::StreamBody;
use hyper::{Response, StatusCode};
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::sync::RwLock;

type RespBody = BoxBody<Bytes, Infallible>;
type Resp = Response<RespBody>;

/// Frame type for SSE streaming
type SseFrame = http_body::Frame<Bytes>;

/// A model change event broadcast to SSE subscribers
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelChangeEvent {
    pub model_name: String,
    pub operation: String,
    pub data: serde_json::Value,
}

/// Broadcasts model change events to SSE subscribers
///
/// Manages per-model broadcast channels with automatic creation on first access.
/// Each channel has a capacity of 1000 events; slow consumers receive lag warnings.
pub struct SseEventBroadcaster {
    channels: RwLock<HashMap<String, broadcast::Sender<ModelChangeEvent>>>,
}

impl SseEventBroadcaster {
    pub fn new() -> Self {
        Self { channels: RwLock::new(HashMap::new()) }
    }

    /// Get or create a broadcast channel for the given model name
    async fn get_channel(&self, model_name: &str) -> broadcast::Sender<ModelChangeEvent> {
        let channels = self.channels.read().await;
        if let Some(sender) = channels.get(model_name) {
            return sender.clone();
        }
        drop(channels);

        let mut channels = self.channels.write().await;
        // Double-check after acquiring write lock
        if let Some(sender) = channels.get(model_name) {
            return sender.clone();
        }
        let (sender, _) = broadcast::channel(1000);
        channels.insert(model_name.to_string(), sender.clone());
        sender
    }

    /// Broadcast a model change event
    pub async fn broadcast(&self, model_name: &str, operation: &str, data: serde_json::Value) {
        let sender = self.get_channel(model_name).await;
        let event = ModelChangeEvent {
            model_name: model_name.to_string(),
            operation: operation.to_string(),
            data,
        };
        // Ignore send errors (no subscribers)
        let _ = sender.send(event);
    }

    /// Create a streaming SSE response for model change events
    ///
    /// Returns a `Response<BoxBody>` compatible with the framework's `Resp` type.
    /// The stream includes:
    /// - An initial `connected` event
    /// - Model change events (`create`, `update`, `delete`)
    /// - Heartbeat comments every 30 seconds
    /// - Lag warnings if the client falls behind
    pub async fn create_sse_response(&self, model_name: &str) -> Resp {
        let sender = self.get_channel(model_name).await;
        let receiver = sender.subscribe();
        let channel_name = model_name.to_string();

        let stream = async_stream::stream! {
            // Initial connected event
            let initial = format!(
                "event: connected\ndata: {}\n\n",
                serde_json::json!({
                    "model": channel_name,
                    "message": "Connected to SSE stream"
                })
            );
            yield Ok::<SseFrame, Infallible>(http_body::Frame::data(Bytes::from(initial)));

            let mut receiver = receiver;
            let mut heartbeat = tokio::time::interval(Duration::from_secs(30));

            loop {
                tokio::select! {
                    _ = heartbeat.tick() => {
                        yield Ok(http_body::Frame::data(Bytes::from(": heartbeat\n\n")));
                    }
                    result = receiver.recv() => {
                        match result {
                            Ok(event) => {
                                let sse = format!(
                                    "event: {}\ndata: {}\n\n",
                                    event.operation,
                                    serde_json::json!({
                                        "model": event.model_name,
                                        "item": event.data
                                    })
                                );
                                yield Ok(http_body::Frame::data(Bytes::from(sse)));
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                let warning = format!(
                                    "event: warning\ndata: {{\"message\":\"Dropped {} events\"}}\n\n",
                                    n
                                );
                                yield Ok(http_body::Frame::data(Bytes::from(warning)));
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                break;
                            }
                        }
                    }
                }
            }
        };

        let stream_body = StreamBody::new(stream);

        Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .header("Connection", "keep-alive")
            .header("Access-Control-Allow-Origin", "*")
            .header("X-Accel-Buffering", "no")
            .body(stream_body.boxed())
            .expect("valid SSE response")
    }
}

impl Default for SseEventBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to create an `Arc<SseEventBroadcaster>` shared across model handlers
pub fn create_broadcaster() -> Arc<SseEventBroadcaster> {
    Arc::new(SseEventBroadcaster::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_broadcaster_creation() {
        let broadcaster = SseEventBroadcaster::new();
        // Broadcasting with no subscribers should not panic
        broadcaster
            .broadcast("todos", "create", serde_json::json!({"id": "1", "title": "Test"}))
            .await;
    }

    #[tokio::test]
    async fn test_broadcast_receives_event() {
        let broadcaster = SseEventBroadcaster::new();
        // Subscribe first
        let sender = broadcaster.get_channel("todos").await;
        let mut rx = sender.subscribe();

        // Broadcast
        broadcaster.broadcast("todos", "create", serde_json::json!({"id": "1"})).await;

        let event = rx.recv().await.unwrap();
        assert_eq!(event.model_name, "todos");
        assert_eq!(event.operation, "create");
    }
}
