//! Compile-fail tests for the declarative attribute surface (gate G2, #128):
//! an unknown key inside #[db]/#[retention]/#[server]/#[schema]/#[firewall]
//! must FAIL the build with a message naming the key and the valid set —
//! never be silently dropped. trybuild pins both the failure and the exact
//! diagnostic text (.stderr snapshots; refresh with TRYBUILD=overwrite).
//!
//! The pass path needs no case here: every model in the workspace compiles
//! through this derive on every build.

#[test]
fn unknown_attribute_keys_fail_the_build() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
