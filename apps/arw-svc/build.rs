fn main() {
    // Only build protobufs when the "grpc" feature is enabled.
    let grpc_enabled = std::env::var("CARGO_FEATURE_GRPC").is_ok();
    if !grpc_enabled {
        return;
    }

    println!("cargo:rerun-if-changed=proto/arw.proto");
    tonic_build::configure()
        .build_client(true)
        .build_server(true)
        .compile(&["proto/arw.proto"], &["proto"]) // inputs, includes
        .expect("failed to compile protos");
}
