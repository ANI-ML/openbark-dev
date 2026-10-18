fn main() {
    // Linux links sherpa-onnx as shared libraries (ADR-0001, amendment of
    // 2026-10-18). They sit next to the binary in `target/`, and in
    // /usr/lib/OpenBark in the .deb, .rpm and AppImage (bundle.linux.*.files in
    // tauri.linux.conf.json). The run path covers both, so the app starts
    // without LD_LIBRARY_PATH.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN:$ORIGIN/../lib/OpenBark");
    }
    tauri_build::build()
}
