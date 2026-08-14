use lithair_macros::DeclarativeModel;

// `main` is a bare flag: `main = false` would silently mean `main = true`
// (the value dropped), so it must fail the build instead.
#[derive(DeclarativeModel)]
#[server(main = false)]
struct Product {
    id: String,
}

fn main() {}
