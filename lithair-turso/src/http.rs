//! Generated model routes backed by SQL, with Lithair's shared session transport.
use crate::{Database, Equal, Error, Page, SqlModel, Store, MAX_DOCUMENT_BYTES};
use lithair_core::app::{
    document_handler, DocumentErrorKind, DocumentStore, ListedPage, ModelFactory,
};
use serde_json::Value;
use std::sync::Arc;

/// Factory used by `DeclarativeModel`. `data_path` is a model directory, just
/// like native storage; it is created on startup and contains `model.db`.
/// Give each registration its own directory. Namespace/collection are stable
/// document keys, independent of the HTTP mount path.
pub fn model_factory<T: SqlModel>() -> ModelFactory {
    Arc::new(|data_path| {
        Box::pin(async move {
            tokio::fs::create_dir_all(&data_path).await?;
            for native_file in ["events.raftlog", "state.raftsnap", "meta.raftmeta"] {
                anyhow::ensure!(
                    !tokio::fs::try_exists(std::path::Path::new(&data_path).join(native_file)).await?,
                    "Native data exists in this model directory; migrate explicitly before selecting Turso"
                );
            }
            let path = std::path::Path::new(&data_path).join("model.db");
            let database = Database::open(
                path.to_str().ok_or_else(|| anyhow::anyhow!("database path must be UTF-8"))?,
            )
            .await?;
            let store = database.store::<T>(T::NAMESPACE)?;
            store.prepare().await?;
            Ok(document_handler(store))
        })
    })
}

#[async_trait::async_trait]
impl<T: SqlModel> DocumentStore for Store<T> {
    type Model = T;
    type Error = Error;
    const BACKEND: &'static str = "Turso";
    const MAX_DOCUMENT_BYTES: usize = MAX_DOCUMENT_BYTES;

    fn filter_fields(&self) -> &'static [&'static str] {
        T::FILTER_FIELDS
    }
    fn permissions(&self) -> &'static [&'static str] {
        T::PERMISSIONS
    }
    fn cache_stats(&self) -> Option<lithair_core::app::CacheStats> {
        Store::cache_stats(self)
    }
    fn error_kind(error: &Error) -> DocumentErrorKind {
        match error {
            Error::Forbidden => DocumentErrorKind::Forbidden,
            Error::NotFound => DocumentErrorKind::NotFound,
            Error::Conflict => DocumentErrorKind::Conflict,
            Error::InvalidInput(_) | Error::Validation(_) => DocumentErrorKind::InvalidInput,
            _ => DocumentErrorKind::Internal,
        }
    }
    async fn list_page(
        &self,
        limit: u32,
        offset: u32,
        filter: Option<(&str, &str)>,
        permissions: &[String],
    ) -> Result<ListedPage<T>, Error> {
        let filter = filter.map(|(field, value)| Equal { field, value });
        let page = Store::list_page(self, Page { limit, offset }, filter, permissions).await?;
        Ok(ListedPage { data: page.data, next_offset: page.next_offset })
    }
    async fn get(&self, id: &str, permissions: &[String]) -> Result<Option<T>, Error> {
        Store::get(self, id, permissions).await
    }
    async fn create(&self, value: T, permissions: &[String]) -> Result<(), Error> {
        Store::create(self, value, permissions).await
    }
    async fn update(&self, id: &str, value: T, permissions: &[String]) -> Result<(), Error> {
        Store::update(self, id, value, permissions).await
    }
    async fn patch(&self, id: &str, changes: Value, permissions: &[String]) -> Result<(), Error> {
        Store::patch(self, id, changes, permissions).await
    }
    async fn delete(&self, id: &str, permissions: &[String]) -> Result<(), Error> {
        Store::delete(self, id, permissions).await
    }
}
