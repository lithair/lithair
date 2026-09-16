use lithair_macros::DeclarativeModel;

#[derive(DeclarativeModel)]
#[storage(turso, version = 3, migrations(v2))]
struct Record {
    id: String,
}

fn main() {}
