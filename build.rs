use std::path::Path;
use std::process::Command;

fn main() {
    if std::env::var("CARGO_FEATURE_CLUSTER").is_ok() {
        println!("cargo:rerun-if-changed=proto/raft.proto");

        let protoc_available = std::env::var("PROTOC")
            .ok()
            .filter(|p| !p.is_empty() && Path::new(p).exists())
            .is_some()
            || Command::new("protoc")
                .arg("--version")
                .output()
                .is_ok_and(|o| o.status.success());

        if !protoc_available {
            panic!(
                "cluster feature 需要 protoc (protobuf-compiler). \
                 安装后重试, 或设置 PROTOC 为编译器路径."
            );
        }

        if let Err(e) =
            tonic_prost_build::configure().compile_protos(&["proto/raft.proto"], &["proto"])
        {
            panic!("failed to compile proto: {e}");
        }
    }
}
