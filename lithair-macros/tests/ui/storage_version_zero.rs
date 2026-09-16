use lithair_macros::DeclarativeModel;

#[derive(DeclarativeModel)]
#[storage(turso, version = 0)]
struct Record {
    id: String,
}

fn main() {}
