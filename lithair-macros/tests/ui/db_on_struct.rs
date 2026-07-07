use lithair_macros::DeclarativeModel;

// field-level attribute in struct position: must fail the build.
#[derive(DeclarativeModel)]
#[db(primary_key)]
struct Order {
    id: String,
}

fn main() {}
