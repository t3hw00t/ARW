fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_GRPC");
    if std::env::var("CARGO_FEATURE_GRPC").is_err() {
        return Ok(());
    }
    println!("cargo:rerun-if-changed=proto/arw.proto");
    tonic_prost_build::configure()
        .build_server(true)
        .compile_protos(&["proto/arw.proto"], &["proto"])?;
    Ok(())
}
