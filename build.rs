fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Set up protobuf compiler for raft-cluster feature
    // protobuf-src provides a bundled protoc to avoid version issues
    #[cfg(feature = "raft-cluster")]
    {
        #[cfg(feature = "protobuf-src")]
        {
            std::env::set_var("PROTOC", protobuf_src::protoc());
        }
    }

    // Only compile protobuf when building with the cluster feature
    #[cfg(feature = "cluster")]
    {
        tonic_build::compile_protos("proto/aidb.proto")?;
    }
    Ok(())
}
