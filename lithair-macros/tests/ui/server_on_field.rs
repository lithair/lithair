use lithair_macros::DeclarativeModel;

// struct-level attribute in field position: must fail the build.
#[derive(DeclarativeModel)]
struct Product {
    #[server(main, cli)]
    id: String,
}

fn main() {}
