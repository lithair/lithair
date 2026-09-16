use lithair_macros::DeclarativeModel;

#[derive(DeclarativeModel)]
#[storage(turso, filters("id"))]
struct Record {
    id: u64,
}

fn main() {}
