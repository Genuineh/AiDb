fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Always set up protobuf compiler using protobuf-src
    // This provides a bundled protoc to avoid version issues on all platforms
    std::env::set_var("PROTOC", protobuf_src::protoc());

    // Only compile protobuf when building with the cluster feature
    #[cfg(feature = "cluster")]
    {
        tonic_build::compile_protos("proto/aidb.proto")?;
    }
    Ok(())
}
