//! Make two tone visits for the playback e2e (apps/desktop/e2e/audio-playback.mjs):
//! a three-minute mono recording and a forty-second two-channel call,
//! encrypted with OPENBARK_DATA_KEY. Prints them as JSON.
//!
//! ```text
//! OPENBARK_DATA_KEY=<scratch key> audio_visits <scratch visits folder>
//! ```
//!
//! Refuses OpenBark's own visits folder: point it at a scratch folder only.

fn main() {
    let Some(root) = std::env::args().nth(1).map(std::path::PathBuf::from) else {
        eprintln!("usage: audio_visits <scratch visits folder>");
        std::process::exit(2);
    };
    let own = os_engine::session::default_root();
    if std::env::var_os("OPENBARK_SESSIONS_DIR").is_none()
        && std::path::absolute(&root).ok() == std::path::absolute(&own).ok()
    {
        eprintln!("refusing: {} is OpenBark's own visits folder", root.display());
        std::process::exit(2);
    }
    if std::env::var_os("OPENBARK_DATA_KEY").is_none() {
        eprintln!("refusing: set OPENBARK_DATA_KEY to a scratch key");
        std::process::exit(2);
    }
    if let Err(e) = os_engine::vault::init() {
        eprintln!("encryption couldn't start: {e}");
        std::process::exit(1);
    }
    match os_app::demo::audio_visits(&root) {
        Ok(visits) => println!("{}", serde_json::to_string_pretty(&visits).expect("visits serialise")),
        Err(e) => {
            eprintln!("{}", e.0);
            std::process::exit(1);
        }
    }
}
