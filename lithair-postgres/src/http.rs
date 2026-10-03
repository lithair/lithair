//! Generated model routes backed by PostgreSQL, with Lithair's shared session
//! transport. The routes are lithair-core's shared document handler.
use crate::{Database, Equal, Error, Page, SqlModel, Store, MAX_DOCUMENT_BYTES};
use lithair_core::app::{
    document_handler, DocumentErrorKind, DocumentStore, ListedPage, ModelFactory,
};
use serde_json::Value;
use std::sync::Arc;

/// Factory used by `DeclarativeModel` for `#[storage(postgres)]`. Records live
/// in the database installed with [`crate::Database::install`], not in
/// `data_path`. The directory is only checked: native data found there means
/// the model was native before, and switching backends needs an explicit
/// migration (RFC 296, R2).
pub fn model_factory<T: SqlModel>() -> ModelFactory {
    Arc::new(|data_path| {
        Box::pin(async move {
            for native_file in ["events.raftlog", "state.raftsnap", "meta.raftmeta", "model.db"] {
                anyhow::ensure!(
                    !tokio::fs::try_exists(std::path::Path::new(&data_path).join(native_file)).await?,
                    "Local data exists in this model directory; migrate explicitly before selecting PostgreSQL"
                );
            }
            let store = Database::installed()?.store::<T>(T::NAMESPACE)?;
            store.prepare().await?;
            Ok(document_handler(store))
        })
    })
}

#[async_trait::async_trait]
impl<T: SqlModel> DocumentStore for Store<T> {
    type Model = T;
    type Error = Error;
    const BACKEND: &'static str = "PostgreSQL";
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
