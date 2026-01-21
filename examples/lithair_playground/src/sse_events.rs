//! Server-Sent Events (SSE) for live updates
//!
//! Provides real-time event streaming for:
//! - Replication events
//! - Cluster state changes
//! - Benchmark progress

#![allow(dead_code)]

use bytes::Bytes;
use http::{Response, StatusCode};
use http_body_util::StreamBody;
use std::collections::HashMap;
use std::convert::Infallible;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::sync::RwLock;

/// Frame type for SSE streaming
type SseFrame = http_body::Frame<Bytes>;

/// Event types for SSE channels
#[derive(Debug, Clone)]
pub struct SseEvent {
    pub channel: String,
    pub data: serde_json::Value,
}

/// SSE Event broadcaster
pub struct SseEventBroadcaster {
    /// Broadcast channels by name
    channels: RwLock<HashMap<String, broadcast::Sender<SseEvent>>>,
}

impl SseEventBroadcaster {
    pub fn new() -> Self {
        Self { channels: RwLock::new(HashMap::new()) }
    }

    /// Get or create a channel
    async fn get_channel(&self, name: &str) -> broadcast::Sender<SseEvent> {
        let channels = self.channels.read().await;
        if let Some(sender) = channels.get(name) {
            return sender.clone();
        }
        drop(channels);

        // Create new channel
        let mut channels = self.channels.write().await;
        let (sender, _) = broadcast::channel(1000);
        channels.insert(name.to_string(), sender.clone());
        sender
    }

    /// Broadcast an event to a channel
    pub async fn broadcast(&self, channel: &str, data: serde_json::Value) {
        let sender = self.get_channel(channel).await;
        let event = SseEvent { channel: channel.to_string(), data };
        // Ignore send errors (no subscribers)
        let _ = sender.send(event);
    }

    /// Create a streaming SSE response for a channel
    pub async fn create_sse_response(
        &self,
        channel: &str,
    ) -> Response<StreamBody<impl futures::Stream<Item = Result<SseFrame, Infallible>>>> {
        let sender = self.get_channel(channel).await;
        let receiver = sender.subscribe();
        let channel_name = channel.to_string();

        // Create the async stream
        let stream = async_stream::stream! {
            // Initial connected event
            let initial = format!(
                "event: connected\ndata: {}\n\n",
                serde_json::json!({
                    "channel": channel_name,
                    "message": "Connected to SSE stream"
                })
            );
            yield Ok(http_body::Frame::data(Bytes::from(initial)));

            let mut receiver = receiver;
            let mut heartbeat = tokio::time::interval(Duration::from_secs(15));

            loop {
                tokio::select! {
                    _ = heartbeat.tick() => {
                        // SSE comment for keepalive
                        yield Ok(http_body::Frame::data(Bytes::from(": heartbeat\n\n")));
                    }
                    result = receiver.recv() => {
                        match result {
                            Ok(event) => {
                                let sse = format!(
                                    "event: {}\ndata: {}\n\n",
                                    event.channel,
                                    event.data
                                );
                                yield Ok(http_body::Frame::data(Bytes::from(sse)));
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                // Client is too slow, send warning
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

        // Build response with streaming body
        Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .header("Connection", "keep-alive")
            .header("Access-Control-Allow-Origin", "*")
            .header("X-Accel-Buffering", "no") // Disable nginx buffering
            .body(StreamBody::new(stream))
            .unwrap()
    }
}

impl Default for SseEventBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}
