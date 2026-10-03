use lithair_macros::DeclarativeModel;

#[derive(DeclarativeModel)]
#[retention(memory = 100, ttl = "5m")]
struct Record {
    id: String,
}

fn main() {}
