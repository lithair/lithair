use lithair_macros::DeclarativeModel;

// `distributed` needs node_id/data_dir from the clap-generated CLI; without
// `cli` it would silently generate a single-node binary, so it must fail.
#[derive(DeclarativeModel)]
#[server(main, distributed)]
struct Product {
    id: String,
}

fn main() {}
