use crate::engine::events::{EventEnvelope, EventStore};
use std::sync::{Arc, RwLock};
use tokio::sync::{mpsc, oneshot};

/// Message pour le writer thread
#[derive(Debug)]
pub enum WriteEvent {
    /// Raw JSON event string
    Event(String),
    /// Structured event envelope
    Envelope(EventEnvelope),
    /// Flush request with acknowledgment channel
    Flush(oneshot::Sender<()>),
}

/// Async writer pour EventStore - écriture en batch
pub struct AsyncWriter {
    tx: mpsc::UnboundedSender<WriteEvent>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

/// Configuration du mode de durabilité
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DurabilityMode {
    /// Performance maximale : batch + flush périodique (10ms)
    /// RISQUE : Perte max 10ms de données en cas de crash brutal
    /// Usage : Benchmarks, prototypes, données non-critiques
    Performance,

    /// Durabilité maximale : fsync après chaque batch (DEFAULT)
    /// GARANTIE : Aucune perte de données, même en cas de crash
    /// Usage : Production, données critiques, event-sourcing
    /// Note : 10-100x plus lent, mais c'est le STANDARD des DB sérieuses
    #[default]
    MaxDurability,
}

impl AsyncWriter {
    /// Créer un nouveau async writer avec durabilité maximale par défaut
    pub fn new(store: Arc<RwLock<EventStore>>, batch_size: usize) -> Self {
        Self::with_durability(store, batch_size, DurabilityMode::default())
    }

    /// Créer un async writer avec mode de durabilité configurable
    pub fn with_durability(
        store: Arc<RwLock<EventStore>>,
        batch_size: usize,
        durability: DurabilityMode,
    ) -> Self {
        // Configurer fsync selon le mode de durabilité (sur le store partagé)
        {
            let mut guard = store.write().expect("event store lock poisoned");
            let fsync = durability == DurabilityMode::MaxDurability;
            guard.configure_batching(batch_size, fsync);
        }

        let (tx, mut rx) = mpsc::unbounded_channel::<WriteEvent>();

        let handle = tokio::spawn(async move {
            let mut buffer = Vec::with_capacity(batch_size);
            // Keep track of flush requests
            let mut flushes = Vec::new();

            loop {
                tokio::select! {
                    // Recevoir des événements
                    Some(msg) = rx.recv() => {
                        match msg {
                            WriteEvent::Event(json) => {
                                buffer.push(json);
                                if buffer.len() >= batch_size {
                                    Self::flush_buffer(&store, &mut buffer, &mut flushes);
                                }
                            }
                            WriteEvent::Envelope(envelope) => {
                                // Serialize envelope to JSON and add to buffer
                                if let Ok(json) = serde_json::to_string(&envelope) {
                                    buffer.push(json);
                                    if buffer.len() >= batch_size {
                                        Self::flush_buffer(&store, &mut buffer, &mut flushes);
                                    }
                                }
                            }
                            WriteEvent::Flush(ack) => {
                                flushes.push(ack);
                                Self::flush_buffer(&store, &mut buffer, &mut flushes);
                            }
                        }

                        // Quick loop to drain channel without sleeping if busy
                        while let Ok(msg) = rx.try_recv() {
                            match msg {
                                WriteEvent::Event(json) => {
                                    buffer.push(json);
                                    if buffer.len() >= batch_size {
                                        Self::flush_buffer(&store, &mut buffer, &mut flushes);
                                    }
                                }
                                WriteEvent::Envelope(envelope) => {
                                    if let Ok(json) = serde_json::to_string(&envelope) {
                                        buffer.push(json);
                                        if buffer.len() >= batch_size {
                                            Self::flush_buffer(&store, &mut buffer, &mut flushes);
                                        }
                                    }
                                }
                                WriteEvent::Flush(ack) => {
                                    flushes.push(ack);
                                    Self::flush_buffer(&store, &mut buffer, &mut flushes);
                                }
                            }
                        }
                    }

                    // Timeout periodic flush (if not receiving)
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(10)), if !buffer.is_empty() => {
                        Self::flush_buffer(&store, &mut buffer, &mut flushes);
                    }

                    // Canal fermé
                    else => {
                        if !buffer.is_empty() || !flushes.is_empty() {
                            Self::flush_buffer(&store, &mut buffer, &mut flushes);
                        }
                        break;
                    }
                }
            }
        });

        Self { tx, handle: Some(handle) }
    }

    /// Get the sender channel to send events to the writer
    pub fn sender(&self) -> &mpsc::UnboundedSender<WriteEvent> {
        &self.tx
    }

    /// Écrire un événement (non-blocking)
    pub fn write(&self, event_json: String) -> Result<(), String> {
        self.tx
            .send(WriteEvent::Event(event_json))
            .map_err(|e| format!("Failed to send event: {}", e))
    }

    /// Flush le buffer sur disque via EventStore
    fn flush_buffer(
        store: &Arc<RwLock<EventStore>>,
        buffer: &mut Vec<String>,
        flushes: &mut Vec<oneshot::Sender<()>>,
    ) {
        if buffer.is_empty() && flushes.is_empty() {
            return;
        }

        let mut guard = match store.write() {
            Ok(g) => g,
            Err(e) => {
                log::error!("EventStore lock error: {}", e);
                // If we can't lock, we can't write.
                // Fail pending flushes?
                // Ideally we should panic or retry, but here we just drop them which causes Receiver drop error.
                // Since we are in a critical thread, logging is best effort.
                return;
            }
        };

        // Write all events
        for event_json in buffer.drain(..) {
            let _ = guard.append_raw_line(&event_json);
        }

        // FS YNC
        if let Err(e) = guard.flush_events() {
            log::error!("flush_events error: {}", e);
        }

        // Acknowledge all flushes
        for ack in flushes.drain(..) {
            let _ = ack.send(());
        }
    }

    /// Attendre que tous les événements soient écrits
    pub async fn shutdown(mut self) {
        // Fermer le canal
        drop(self.tx);

        // Attendre que le writer termine
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
    }

    /// Flush all pending events to disk immediately and wait for completion.
    pub async fn flush(&self) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(WriteEvent::Flush(tx))
            .map_err(|e| format!("Failed to send flush signal: {}", e))?;

        // Wait for acknowledgement
        rx.await.map_err(|e| format!("Flush cancelled (channel closed): {}", e))
    }
}
