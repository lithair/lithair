use lithair_macros::DeclarativeModel;

#[derive(DeclarativeModel)]
#[retention(memroy = 100)] // typo: unknown key must fail the build
struct Email {
    #[db(primary_key)]
    id: String,
}

fn main() {}
