//! One commit boundary for generated CRUD, admin edits and replicated apply.
use super::*;

pub(super) type MutationGuard = tokio::sync::OwnedMutexGuard<Option<String>>;

#[derive(Debug, thiserror::Error)]
pub(super) enum MutationError {
    #[error("Insufficient permissions")]
    Forbidden,
    #[error("Entity not found")]
    NotFound,
    #[error("{0}")]
    Invalid(String),
    #[error("Unique constraint: {0}")]
    Conflict(String),
    #[error("Native persistence failed: {0}")]
    Persistence(String),
}

impl<T> DeclarativeHttpHandler<T>
where
    T: HttpExposable + LifecycleAware + ReplicatedModel + RetentionAware,
{
    pub(super) async fn lock_mutations(&self) -> Result<MutationGuard, MutationError> {
        let guard = self.mutations.clone().lock_owned().await;
        if let Some(error) = &*guard {
            return Err(MutationError::Persistence(error.clone()));
        }
        Ok(guard)
    }

    pub(super) fn check_primary_key(id: &str, item: &T) -> Result<(), MutationError> {
        if item.get_primary_key() != id {
            return Err(MutationError::Invalid(
                "The primary key must match the route and cannot be changed".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn mutation_error_response(&self, error: MutationError) -> Resp {
        match error {
            MutationError::Forbidden => {
                self.json_error_response(StatusCode::FORBIDDEN, "Insufficient permissions")
            }
            MutationError::NotFound => self.not_found_response(),
            MutationError::Invalid(message) => self.bad_request_response(&message),
            MutationError::Conflict(message) => {
                self.json_error_response(StatusCode::CONFLICT, &message)
            }
            MutationError::Persistence(message) => {
                log::error!("Native commit failed: {message}");
                self.internal_error_response()
            }
        }
    }

    /// The owned task keeps the mutation permit until append/flush, publication
    /// and notification finish, even if the initiating request is dropped.
    /// Readers keep using the previous memory state while the journal is busy.
    pub(super) async fn commit_mutation(
        &self,
        mut mutation: MutationGuard,
        operation: &'static str,
        key: String,
        item: T,
        notification: Option<&'static str>,
    ) -> Result<(), MutationError> {
        let envelope = EventEnvelope {
            event_type: format!("{}{operation}", Self::event_model_name()),
            event_id: format!(
                "{}:{operation}:{key}:{}",
                Self::event_model_name(),
                uuid::Uuid::new_v4()
            ),
            timestamp: chrono::Utc::now().timestamp() as u64,
            payload: serde_json::to_string(&item)
                .map_err(|e| MutationError::Invalid(e.to_string()))?,
            aggregate_id: Some(key.clone()),
            event_hash: None,
            previous_hash: None,
        };
        let store = self.event_store.clone();
        let storage = self.storage.clone();
        let published_events = self.published_events.clone();
        let retention = self.retention.clone();
        let broadcaster = self.sse_broadcaster.get().cloned();
        tokio::spawn(async move {
            // Leave a failure behind even if the task panics during publication.
            *mutation =
                Some("An admitted commit did not finish; reopen and reconcile the journal".into());
            let result = tokio::task::spawn_blocking(move || {
                let mut store = store.blocking_write();
                store
                    .append_envelope(&envelope)
                    .and_then(|_| store.flush())
                    .map(|_| store.event_count())
                    .map_err(|error| error.to_string())
            })
            .await
            .map_err(|error| error.to_string())
            .and_then(|result| result);
            let count = match result {
                Ok(count) => count,
                Err(error) => {
                    *mutation = Some(error.clone());
                    return Err(MutationError::Persistence(error));
                }
            };
            {
                let mut storage = storage.write().await;
                if operation == "Deleted" {
                    storage.remove(&key);
                    if let Some(retention) = &retention {
                        retention.remove(&key);
                    }
                } else {
                    Self::insert_with_retention(
                        &mut storage,
                        retention.as_deref(),
                        key,
                        item.clone(),
                    );
                }
                published_events.store(count, std::sync::atomic::Ordering::Release);
            }
            *mutation = None;
            if let (Some(broadcaster), Some(operation)) = (broadcaster, notification) {
                if let Ok(data) = serde_json::to_value(&item) {
                    broadcaster.broadcast(T::http_base_path(), operation, data).await;
                }
            }
            Ok(())
        })
        .await
        .map_err(|error| MutationError::Persistence(error.to_string()))?
    }
}
