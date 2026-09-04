fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var_os("CARGO_FEATURE_NAPI_ADDON").is_some() {
        napi_build::setup();
    }
}
