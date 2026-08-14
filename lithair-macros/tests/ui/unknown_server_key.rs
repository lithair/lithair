use lithair_macros::DeclarativeModel;

#[derive(DeclarativeModel)]
#[server(main, clii)] // typo: unknown key must fail the build
struct Product {
    id: String,
}

fn main() {}
