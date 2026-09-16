use lithair_macros::DeclarativeModel;

#[derive(DeclarativeModel)]
#[storage(turso)]
struct Record {
    #[db(indexed)]
    id: String,
}

fn main() {}
