use lithair_macros::DeclarativeModel;

#[derive(DeclarativeModel)]
#[storage(native, collection = "bad")]
struct Record {
    id: String,
}

fn main() {}
