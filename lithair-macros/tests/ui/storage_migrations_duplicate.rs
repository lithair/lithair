use lithair_macros::DeclarativeModel;

#[derive(DeclarativeModel)]
#[storage(turso, version = 1, migrations(), migrations())]
struct Record {
    id: String,
}

fn main() {}
