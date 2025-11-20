fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Set up protobuf compiler for cluster and raft-cluster features
    // protobuf-src provides a bundled protoc to avoid version issues
    #[cfg(feature = "cluster")]
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
