//! `LT_SESSION_CROSS_SITE_CHECK` (issue #225) reaches the sessions config
//! and only accepts the two documented values.
//!
//! One process, one test: env vars are process-wide.

use lithair_core::config::SessionsConfig;
use lithair_core::session::{CookieConfig, CrossSiteCheck};

#[test]
fn env_var_drives_the_cross_site_check() {
    std::env::set_var("LT_SESSION_CROSS_SITE_CHECK", "Off");
    let mut sessions = SessionsConfig::default();
    sessions.apply_env_vars();
    assert_eq!(sessions.cross_site_check, "Off");
    assert!(sessions.validate().is_ok());
    assert_eq!(CookieConfig::from(&sessions).cross_site_check, CrossSiteCheck::Off);

    std::env::set_var("LT_SESSION_CROSS_SITE_CHECK", "sideways");
    let mut sessions = SessionsConfig::default();
    sessions.apply_env_vars();
    let err = sessions.validate().expect_err("only Enforce/Off are valid");
    assert!(err.to_string().contains("cross_site_check"), "{err}");

    std::env::remove_var("LT_SESSION_CROSS_SITE_CHECK");
}
