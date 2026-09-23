use crate::engine::events::{EventEnvelope, EventStore};
use std::sync::{mpsc, Arc, Mutex, RwLock};
use tokio::sync::oneshot;

/// Message for the writer worker. Queue admission is not a persistence acknowledgement.
#[derive(Debug)]
pub enum WriteEvent {
    Event(String),
    Envelope(EventEnvelope),
    Flush(oneshot::Sender<Result<(), String>>),
}

/// Ordered, batched writes. After an I/O failure this writer remains failed:
/// retrying a partly written batch could duplicate events. Reopen/recover first.
pub struct AsyncWriter {
    tx: mpsc::Sender<WriteEvent>,
    handle: Option<std::thread::JoinHandle<()>>,
    failure: Arc<Mutex<Option<String>>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DurabilityMode {
    /// Flush batches without fsync. Neither queue admission nor the timer
    /// guarantees persistence across a machine failure.
    Performance,
    /// Fsync completed batches. Call `flush` to await acknowledgement;
    /// merely enqueueing an event does not establish durability.
    #[default]
    MaxDurability,
}

impl AsyncWriter {
    pub fn new(store: Arc<RwLock<EventStore>>, batch_size: usize) -> Self {
        Self::with_durability(store, batch_size, DurabilityMode::default())
    }

    pub fn with_durability(
        store: Arc<RwLock<EventStore>>,
        batch_size: usize,
        durability: DurabilityMode,
    ) -> Self {
        let batch_size = batch_size.max(1);
        let initial_error = match store.write() {
            Ok(mut guard) => {
                guard.configure_batching(batch_size, durability == DurabilityMode::MaxDurability);
                None
            }
            Err(error) => Some(format!("event store lock poisoned: {error}")),
        };
        let failure = Arc::new(Mutex::new(initial_error));
        let worker_failure = failure.clone();
        let (tx, rx) = mpsc::channel::<WriteEvent>();
        // Blocking file I/O has its own worker. Native mutation callers may
        // hold their ordering gate while waiting for a flush; this worker must
        // progress even when those callers occupy Tokio/blocking-pool threads.
        let handle =
            std::thread::Builder::new().name("lithair-event-writer".into()).spawn(move || {
                let mut buffer = Vec::with_capacity(batch_size);
                let period = std::time::Duration::from_millis(10);
                let mut deadline = std::time::Instant::now() + period;
                loop {
                    match rx
                        .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                    {
                        Ok(WriteEvent::Flush(ack)) => {
                            let result = Self::flush_buffer(&store, &mut buffer, &worker_failure);
                            let _ = ack.send(result);
                            deadline = std::time::Instant::now() + period;
                        }
                        Ok(event) => {
                            if Self::check_failure(&worker_failure).is_ok() {
                                buffer.push(event);
                            }
                            if buffer.len() >= batch_size || std::time::Instant::now() >= deadline {
                                let _ = Self::flush_buffer(&store, &mut buffer, &worker_failure);
                                deadline = std::time::Instant::now() + period;
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            if !buffer.is_empty() {
                                let _ = Self::flush_buffer(&store, &mut buffer, &worker_failure);
                            }
                            deadline = std::time::Instant::now() + period;
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            let _ = Self::flush_buffer(&store, &mut buffer, &worker_failure);
                            break;
                        }
                    }
                }
            });
        let handle = match handle {
            Ok(handle) => Some(handle),
            Err(error) => {
                if let Ok(mut failed) = failure.lock() {
                    *failed = Some(format!("failed to start event writer: {error}"));
                }
                None
            }
        };
        Self { tx, handle, failure }
    }

    fn check_failure(failure: &Mutex<Option<String>>) -> Result<(), String> {
        match &*failure.lock().map_err(|e| format!("writer failure lock poisoned: {e}"))? {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    pub fn sender(&self) -> &mpsc::Sender<WriteEvent> {
        &self.tx
    }

    /// Queue a raw event. Use `flush` to learn whether persistence succeeded.
    pub fn write(&self, event_json: String) -> Result<(), String> {
        Self::check_failure(&self.failure)?;
        self.tx
            .send(WriteEvent::Event(event_json))
            .map_err(|e| format!("Failed to queue event: {e}"))
    }

    pub(crate) fn write_envelope(&self, envelope: EventEnvelope) -> Result<(), String> {
        Self::check_failure(&self.failure)?;
        self.tx
            .send(WriteEvent::Envelope(envelope))
            .map_err(|e| format!("Failed to queue event: {e}"))
    }

    fn flush_buffer(
        store: &RwLock<EventStore>,
        buffer: &mut Vec<WriteEvent>,
        failure: &Mutex<Option<String>>,
    ) -> Result<(), String> {
        Self::check_failure(failure)?;
        let result = (|| {
            let mut guard = store.write().map_err(|e| format!("event store lock poisoned: {e}"))?;
            for event in buffer.drain(..) {
                let result = match event {
                    WriteEvent::Event(json) => guard.append_raw_line(&json),
                    WriteEvent::Envelope(envelope) => guard.append_envelope(&envelope),
                    WriteEvent::Flush(_) => unreachable!("flush messages are never buffered"),
                };
                result.map_err(|e| e.to_string())?;
            }
            guard.flush_events().map_err(|e| e.to_string())
        })();
        if let Err(error) = &result {
            log::error!("event writer stopped after persistence failure: {error}");
            *failure.lock().map_err(|e| format!("writer failure lock poisoned: {e}"))? =
                Some(error.clone());
            buffer.clear();
        }
        result
    }

    /// Close this sender and wait for the worker. Call `flush` first when the
    /// caller needs a persistence result. Other cloned senders must be dropped.
    pub async fn shutdown(mut self) {
        drop(self.tx);
        if let Some(handle) = self.handle.take() {
            match tokio::task::spawn_blocking(move || handle.join()).await {
                Ok(Ok(())) => {}
                _ => log::error!("event writer thread failed during shutdown"),
            }
        }
    }

    /// Only call from a blocking thread, never from a Tokio executor callback.
    pub(crate) fn flush_blocking(&self) -> Result<(), String> {
        let rx = self.flush_barrier()?;
        rx.blocking_recv()
            .map_err(|e| format!("Flush cancelled (channel closed): {e}"))?
    }

    fn flush_barrier(&self) -> Result<oneshot::Receiver<Result<(), String>>, String> {
        Self::check_failure(&self.failure)?;
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(WriteEvent::Flush(tx))
            .map_err(|e| format!("Failed to queue flush: {e}"))?;
        Ok(rx)
    }

    /// Acknowledge all events queued before this barrier, or return the first
    /// persistence failure. A failed writer never reports a later success.
    pub async fn flush(&self) -> Result<(), String> {
        self.flush_barrier()?
            .await
            .map_err(|e| format!("Flush cancelled (channel closed): {e}"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writer_progress_does_not_depend_on_a_tokio_executor() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(RwLock::new(EventStore::new(dir.path().to_str().unwrap()).unwrap()));
        let writer = AsyncWriter::new(store.clone(), 1000);
        writer.write(r#"{"value":1}"#.into()).unwrap();
        writer.write(r#"{"value":2}"#.into()).unwrap();
        writer.flush_blocking().unwrap();
        assert_eq!(store.read().unwrap().get_all_events().unwrap().len(), 2);
        // Join without a runtime too, so the temporary directory outlives the worker.
        let AsyncWriter { tx, handle, .. } = writer;
        drop(tx);
        handle.unwrap().join().unwrap();
    }
}
