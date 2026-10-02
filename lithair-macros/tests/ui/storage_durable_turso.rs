use lithair_macros::DeclarativeModel;

#[derive(DeclarativeModel)]
#[storage(turso, durable)]
struct Record {
    id: String,
}

fn main() {}
