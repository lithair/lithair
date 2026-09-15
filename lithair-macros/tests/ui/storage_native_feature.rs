use lithair_macros::DeclarativeModel;

#[derive(DeclarativeModel)]
#[storage(turso)]
#[retention(memory = "1h")]
struct Record {
    id: String,
}

fn main() {}
