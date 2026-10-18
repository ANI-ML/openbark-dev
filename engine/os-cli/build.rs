//! Linux links sherpa-onnx as shared libraries (ADR-0001, amendment of
//! 2026-10-18). `sherpa-onnx-sys` copies them next to the binary in `target/`,
//! and the install scripts put them next to the installed `openbark`. The
//! `$ORIGIN` run path makes `openbark` look there, so it starts without
//! `LD_LIBRARY_PATH`. Windows (DLLs beside the exe) and macOS (static) need
//! nothing here.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");
    }
}
