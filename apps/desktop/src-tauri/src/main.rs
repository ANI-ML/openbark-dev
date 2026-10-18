// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The GPU self-test runs the app itself as a child (ADR-0014).
    if std::env::args().nth(1).as_deref() == Some(os_app::GPU_CHECK_ARG) {
        std::process::exit(os_app::gpu_check_main());
    }
    openbark_desktop_lib::run()
}
