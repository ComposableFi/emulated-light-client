use std::io::Result;

fn main() -> Result<()> {
    let mut config = prost_build::Config::new();
    if std::env::var_os("CARGO_FEATURE_std").is_none() {
        config.btree_map(["."]);
    }
    config
        .enable_type_names()
        .type_name_domain(["."], "composable.finance")
        .include_file("messages.rs")
        .compile_protos(&["proto/guest.proto"], &["proto/"])
}
