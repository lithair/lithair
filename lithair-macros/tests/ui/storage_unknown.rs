use lithair_macros::DeclarativeModel;

#[derive(DeclarativeModel)]
#[storage(postgres)]
struct Record {
    id: String,
}

fn main() {}
