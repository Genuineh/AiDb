use std::path::Path;
use std::process::Command;

fn main() {
    if std::env::var("CARGO_FEATURE_CLUSTER").is_ok() {
        println!("cargo:rerun-if-changed=proto/raft.proto");

        let out = Path::new("src").join("cluster").join("network");
        let _ = std::fs::create_dir_all(&out);

        // Only compile proto if protoc is available (CI may not have it).
        let protoc_available = std::env::var("PROTOC")
            .ok()
            .filter(|p| !p.is_empty() && Path::new(p).exists())
            .is_some()
            || Command::new("protoc")
                .arg("--version")
                .output()
                .is_ok_and(|o| o.status.success());

        if protoc_available {
            if let Err(e) = tonic_build::configure()
                .out_dir(&out)
                .compile(&["proto/raft.proto"], &["proto"])
            {
                panic!("failed to compile proto: {e}");
            }
        } else {
            println!("cargo:warning=protoc not found, using checked-in generated code");
        }
    }
}
