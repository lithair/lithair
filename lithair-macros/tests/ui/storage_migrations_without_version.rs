use lithair_macros::DeclarativeModel;

#[derive(DeclarativeModel)]
#[storage(turso, migrations(v2))]
struct Record {
    id: String,
}

fn main() {}
