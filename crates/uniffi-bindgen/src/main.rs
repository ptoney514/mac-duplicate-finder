//! Binding generator entry point, invoked by scripts/build-xcframework.sh:
//! `cargo run -p uniffi-bindgen -- generate --library <dylib> --language swift`

fn main() {
    uniffi::uniffi_bindgen_main()
}
