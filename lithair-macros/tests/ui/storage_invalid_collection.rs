use lithair_macros::DeclarativeModel;

#[derive(DeclarativeModel)]
#[storage(turso, collection = "")]
struct Record {
    id: String,
}

fn main() {}
