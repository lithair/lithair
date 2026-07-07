use lithair_macros::DeclarativeModel;

#[derive(DeclarativeModel)]
#[schema(verison = 2)] // typo: unknown key must fail the build
struct Order {
    #[db(primary_key)]
    id: String,
}

fn main() {}
