fn main() {
    let protos = [
        "proto/zmk/studio.proto",
        "proto/zmk/meta.proto",
        "proto/zmk/core.proto",
        "proto/zmk/behaviors.proto",
        "proto/zmk/keymap.proto",
    ];

    for proto in protos {
        println!("cargo:rerun-if-changed={proto}");
    }

    let protoc = protoc_bin_vendored::protoc_bin_path().expect("failed to get protoc binary path");

    let mut config = prost_build::Config::new();
    config.include_file("proto_mod.rs");
    config.protoc_executable(protoc);

    config
        .compile_protos(&protos, &["proto/zmk"])
        .expect("failed to compile protobuf definitions");
}
