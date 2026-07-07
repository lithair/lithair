use lithair_macros::DeclarativeModel;

#[derive(DeclarativeModel)]
struct Email {
    #[db(primary_key)]
    id: String,
    // struct-level attribute in field position: used to be SILENTLY ignored
    // (no memory budget configured, no error) — must fail the build.
    #[retention(memory = 100)]
    body: String,
}

fn main() {}
