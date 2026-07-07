use lithair_macros::DeclarativeModel;

#[derive(DeclarativeModel)]
struct Product {
    #[db(primary_kye)] // typo: unknown key must fail the build
    id: String,
}

fn main() {}
