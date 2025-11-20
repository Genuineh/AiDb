fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Set up protobuf compiler using protobuf-src for tonic-build
    std::env::set_var("PROTOC", protobuf_src::protoc());

    // Only compile protobuf when building with the cluster feature
    #[cfg(feature = "cluster")]
    {
        tonic_build::compile_protos("proto/aidb.proto")?;
    }
    
    // Compile raft protobuf when building with the raft-cluster feature
    #[cfg(feature = "raft-cluster")]
    {
        tonic_build::compile_protos("proto/raft.proto")?;
    }
    
    Ok(())
}
