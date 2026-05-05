//! `DeclarativeServe` — convenience trait for ultra-simple model serving.
//!
//! Provides `MyModel::serve_on_port(8080).await?` shorthand, used by the
//! single-node branch of the `#[derive(DeclarativeModel)]` macro
//! (`lithair-macros/src/declarative_simple.rs:1396, 1407, 1433`).
//!
//! Phase 4/5 of issue #42: the trait surface is preserved, but the default
//! implementations now build on [`LithairServer`](crate::app::LithairServer)
//! instead of the retired `DeclarativeServer`. Behavior is preserved
//! (port + auto-generated event-store path + `with_declarative_model`).

use std::future::Future;
use std::pin::Pin;

use crate::app::LithairServer;
use crate::consensus::ReplicatedModel;
use crate::http::HttpExposable;
use crate::lifecycle::LifecycleAware;
use crate::schema::HasSchemaSpec;

/// Extension trait that provides `MyModel::serve_on_port(8080).await?`.
///
/// This is the ultra-simple Lithair entry point used by macro-generated
/// single-node `main()` functions. For richer configuration (TLS, RBAC,
/// raft clustering, multi-model servers, ops endpoints), use
/// [`LithairServer`](crate::app::LithairServer) directly.
///
/// The default implementations are behavior-preserving migrations of the
/// pre-0.2.0 `DeclarativeServer`-based bodies; the trait surface itself is
/// unchanged.
pub trait DeclarativeServe:
    HttpExposable + LifecycleAware + ReplicatedModel + HasSchemaSpec + Send + Sync + 'static
{
    /// Serve this model on the given port with an auto-generated EventStore path.
    ///
    /// The event store is placed under `./data/{model_name_lowercase}.events`,
    /// and the directory is created on demand. The base path under which the
    /// model is exposed is `/api/{HttpExposable::http_base_path()}`.
    fn serve_on_port(port: u16) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        let model_name = std::any::type_name::<Self>()
            .rsplit("::")
            .next()
            .unwrap_or("UnknownModel")
            .to_lowercase();

        let event_store_path = format!("./data/{}.events", model_name);
        std::fs::create_dir_all("./data").ok();

        let base_path = format!("/api/{}", <Self as HttpExposable>::http_base_path());

        Box::pin(async move {
            LithairServer::new()
                .with_port(port)
                .with_declarative_model::<Self>(event_store_path, base_path)
                .serve()
                .await
        })
    }

    /// Serve with an explicit EventStore path.
    ///
    /// Same as [`serve_on_port`](Self::serve_on_port) but lets the caller
    /// pin the event-store location instead of inferring it from the model
    /// name.
    fn serve_with_storage(
        port: u16,
        event_store_path: &str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        let path = event_store_path.to_string();
        let base_path = format!("/api/{}", <Self as HttpExposable>::http_base_path());

        Box::pin(async move {
            LithairServer::new()
                .with_port(port)
                .with_declarative_model::<Self>(path, base_path)
                .serve()
                .await
        })
    }
}

// Auto-implement for every type that satisfies the bounds.
impl<T> DeclarativeServe for T where
    T: HttpExposable + LifecycleAware + ReplicatedModel + HasSchemaSpec + Send + Sync + 'static
{
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time check that the trait is dyn-incompatible (it is, because
    /// of the `Sized`-implying `'static` bound and associated functions). We
    /// just need to make sure the bounds line up so users get a sensible
    /// error if they accidentally try to use it as a trait object.
    #[allow(dead_code)]
    fn _trait_bounds_compile<T: DeclarativeServe>() {
        let _ = T::serve_on_port;
        let _ = T::serve_with_storage;
    }
}
