use lithair_macros::DeclarativeModel;

#[derive(DeclarativeModel)]
#[storage(turso, version = 1, version = 2)]
struct Record {
    id: String,
}

fn main() {}
