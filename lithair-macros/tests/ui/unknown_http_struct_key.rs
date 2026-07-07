use lithair_macros::DeclarativeModel;

// struct-level #[http] typo: unknown key must fail the build (was silently
// ignored — the same G2 class as the position fixes in this PR).
#[derive(DeclarativeModel)]
#[http(base_pth = "shop")]
struct Product {
    #[db(primary_key)]
    id: String,
}

fn main() {}
