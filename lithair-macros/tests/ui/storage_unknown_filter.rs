use lithair_macros::DeclarativeModel;

#[derive(DeclarativeModel)]
#[storage(turso, filters("missing"))]
struct Record {
    id: String,
}

fn main() {}
