use lithair_macros::DeclarativeModel;

#[derive(DeclarativeModel)]
#[storage(turso)]
struct Record {
    #[lifecycle(audited)]
    id: String,
}

fn main() {}
