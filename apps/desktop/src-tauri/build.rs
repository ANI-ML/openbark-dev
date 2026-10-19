fn main() {
    // Linux links sherpa-onnx as shared libraries (ADR-0001, amendment of
    // 2026-10-18). They sit next to the binary in `target/`, and in
    // /usr/lib/OpenBark in the .deb, .rpm and AppImage (bundle.linux.*.files in
    // tauri.linux.conf.json). The run path covers both, so the app starts
    // without LD_LIBRARY_PATH.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN:$ORIGIN/../lib/OpenBark");
    }
    // macOS: ggml's Metal code uses `@available` checks, which call
    // __isPlatformVersionAtLeast from clang's runtime (libclang_rt.osx.a).
    // rustc links with -nodefaultlibs, so that library isn't added on its own
    // and the app failed to link. Link it explicitly from the toolchain's own
    // runtime folder.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let out = std::process::Command::new("xcrun")
            .args(["--sdk", "macosx", "clang", "--print-runtime-dir"])
            .output()
            .or_else(|_| std::process::Command::new("clang").arg("--print-runtime-dir").output());
        match out {
            Ok(o) if o.status.success() => {
                let dir = String::from_utf8_lossy(&o.stdout).trim().to_string();
                println!("cargo:rustc-link-search=native={dir}");
                println!("cargo:rustc-link-lib=static=clang_rt.osx");
            }
            _ => println!("cargo:warning=couldn't find clang's runtime folder; the Metal build may not link"),
        }
    }
    tauri_build::build()
}
