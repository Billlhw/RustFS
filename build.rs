use std::env;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    // Get the path to the vendored protoc binary
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("protoc not found");
    let protoc_include =
        protoc_bin_vendored::include_path().expect("protoc include path not found");

    // Set the environment variables for protoc
    env::set_var("PROTOC", protoc);
    env::set_var("PROTOC_INCLUDE", protoc_include);

    // Configure tonic-build and compile the .proto files
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .type_attribute(
            "ChunkInfo",
            "#[derive(serde::Serialize, serde::Deserialize)]",
        )
        .compile(&["proto/master.proto", "proto/chunk.proto"], &["proto"])?;

    Ok(())
}
