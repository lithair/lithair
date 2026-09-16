use lithair_macros::DeclarativeModel;

#[derive(DeclarativeModel)]
#[storage(turso)]
struct Record {
    #[rbac(owner_field)]
    id: String,
}

fn main() {}
