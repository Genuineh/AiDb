fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Only compile protobuf when building with the cluster feature
    #[cfg(feature = "cluster")]
    {
        tonic_build::compile_protos("proto/aidb.proto")?;
    }
    Ok(())
}
