//! Environment changes stay isolated in this single-test process.
use lithair_core::{http::DeclarativeHttpHandler, DeclarativeModel};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
struct Record {
    #[db(primary_key)]
    id: String,
}

#[test]
fn native_http_rejects_the_legacy_unacknowledged_writer() {
    struct Restore(Option<std::ffi::OsString>);
    impl Drop for Restore {
        fn drop(&mut self) {
            match &self.0 {
                Some(value) => std::env::set_var("LT_OPT_PERSIST", value),
                None => std::env::remove_var("LT_OPT_PERSIST"),
            }
        }
    }
    let _restore = Restore(std::env::var_os("LT_OPT_PERSIST"));
    let dir = tempfile::tempdir().unwrap();
    for value in ["1", "TRUE"] {
        std::env::set_var("LT_OPT_PERSIST", value);
        let error = DeclarativeHttpHandler::<Record>::new(dir.path().to_str().unwrap())
            .err()
            .unwrap();
        assert!(error.to_string().contains("LT_OPT_PERSIST=0"));
    }
    std::env::set_var("LT_OPT_PERSIST", "0");
    // No background timer means construction does not require a Tokio runtime.
    assert!(DeclarativeHttpHandler::<Record>::new(dir.path().to_str().unwrap()).is_ok());
}
