use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Set up protobuf compiler using protobuf-src for tonic-build
    std::env::set_var("PROTOC", protobuf_src::protoc());

    // Emit generated protobuf into src/cluster so it's available at compile time
    // This avoids relying on env!("OUT_DIR") at compile-time which can be fragile
    let out = Path::new("src").join("cluster");
    std::fs::create_dir_all(&out)?;

    // Use separate configure calls because `Builder::compile` takes self by value
    if std::env::var("CARGO_FEATURE_CLUSTER").is_ok() {
        tonic_build::configure()
            .out_dir(out.clone())
            .compile(&["proto/aidb.proto"], &["proto"])?;
        println!("cargo:rerun-if-changed=proto/aidb.proto");
    }

    if std::env::var("CARGO_FEATURE_RAFT_CLUSTER").is_ok() {
        tonic_build::configure()
            .out_dir(out.clone())
            .compile(&["proto/raft.proto"], &["proto"])?;
        println!("cargo:rerun-if-changed=proto/raft.proto");
    }

    Ok(())
}
