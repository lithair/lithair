#[path = "../../lithair-core/tests/support/oidc_fixture.rs"]
#[allow(dead_code)]
mod fixture;
use cucumber::{then, World};
#[derive(Debug, Default, World)]
struct OidcWorld;
#[then("an OIDC login gives custom handlers a verified subject that no request can change, until logout revokes it")]
async fn verified_identity(_: &mut OidcWorld) {
    fixture::login_yields_a_verified_identity_until_logout().await;
}
#[tokio::main]
async fn main() {
    OidcWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit("features/core/oidc.feature")
        .await;
}
