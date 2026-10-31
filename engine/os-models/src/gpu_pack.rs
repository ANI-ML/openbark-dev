//! The GPU speech pack (ADR-0014): what speech recognition needs to run on an
//! NVIDIA graphics card on Windows. Downloaded only when the clinic asks for it.
//!
//! - ONNX Runtime 1.28.2 built with CUDA, from the sherpa-onnx v1.13.8
//!   release (the same build sherpa-onnx is tested with): `onnxruntime.dll`
//!   and its CUDA provider.
//! - NVIDIA's CUDA runtime, cuBLAS and cuDNN 9, from NVIDIA's own packages on
//!   PyPI. They're NVIDIA's software under NVIDIA's licence; OpenBark fetches
//!   them for the clinic rather than shipping them.
//!
//! The archives download like any model (resumable, size and SHA-256
//! checked) into `<models>/gpu-speech-pack`, then the libraries are unpacked
//! beside the program, where ONNX Runtime looks for them. A running program
//! keeps the processor-only `onnxruntime.dll` loaded, so that one file is
//! swapped by renaming the old one aside (Windows allows renaming a file in
//! use); the GPU is used from the next start.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::catalog::{CatalogEntry, FileSpec, Layout, Slot, Source};
use crate::store::ModelStore;

pub const PACK_ID: &str = "gpu-speech-pack";

/// One archive, and the files to take from it: file names, or `prefix*.dll`
/// (cuDNN's set of sub-libraries changes between 9.x releases).
struct Archive {
    name: &'static str,
    url: &'static str,
    size: u64,
    sha256: &'static str,
    take: &'static [&'static str],
}

const ARCHIVES: [Archive; 4] = [
    Archive {
        name: "sherpa-onnx-v1.13.8-cuda-12.x-cudnn-9.x-onnxruntime1.28.2-win-x64-cuda.tar.bz2",
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/sherpa-onnx-v1.13.8-cuda-12.x-cudnn-9.x-onnxruntime1.28.2-win-x64-cuda.tar.bz2",
        size: 595_017_373,
        sha256: "066c5b54dbafaa1388001a9c9837ac1374dbba6d6678f193ca06aa0d8e94d8c3",
        take: &["onnxruntime.dll", "onnxruntime_providers_shared.dll", "onnxruntime_providers_cuda.dll"],
    },
    Archive {
        name: "nvidia_cuda_runtime_cu12-12.9.79-py3-none-win_amd64.whl",
        url: "https://files.pythonhosted.org/packages/59/df/e7c3a360be4f7b93cee39271b792669baeb3846c58a4df6dfcf187a7ffab/nvidia_cuda_runtime_cu12-12.9.79-py3-none-win_amd64.whl",
        size: 3_591_604,
        sha256: "8e018af8fa02363876860388bd10ccb89eb9ab8fb0aa749aaf58430a9f7c4891",
        take: &["cudart64_12.dll"],
    },
    Archive {
        name: "nvidia_cublas_cu12-12.9.2.10-py3-none-win_amd64.whl",
        url: "https://files.pythonhosted.org/packages/20/e2/fc9a0e985249d873150276d5afb02e39a66817fedbf1a385724393e505ed/nvidia_cublas_cu12-12.9.2.10-py3-none-win_amd64.whl",
        size: 553_162_896,
        sha256: "623f43027d40d44ceadf0043f002bd25cf353e8f13ce90b9a87057019f560661",
        take: &["cublas64_12.dll", "cublasLt64_12.dll"],
    },
    Archive {
        name: "nvidia_cudnn_cu12-9.27.0.42-py3-none-win_amd64.whl",
        url: "https://files.pythonhosted.org/packages/aa/38/f856579877f7c1c5066e61182e7de7bc27bf35a78c8d1b0fa592e6985bc4/nvidia_cudnn_cu12-9.27.0.42-py3-none-win_amd64.whl",
        size: 743_068_852,
        sha256: "06e9b0026f3bad97d2b58666330fabec04fe1672f776661ecb0ce0029c27f142",
        // All of cuDNN: Parakeet's LSTM decoder needs cudnn_adv, and 9.27
        // added cudnn_engines_tensor_ir.
        take: &["cudnn*.dll"],
    },
];

fn matches(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        Some((pre, post)) => {
            name.starts_with(pre) && name.ends_with(post) && name.len() >= pre.len() + post.len()
        }
        None => pattern == name,
    }
}

/// The list of libraries the pack put beside the program.
fn manifest() -> Option<PathBuf> {
    program_dir().map(|d| d.join("gpu-pack-files.txt"))
}

fn manifest_lines() -> Vec<String> {
    manifest()
        .and_then(|m| fs::read_to_string(m).ok())
        .map(|s| s.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
        .unwrap_or_default()
}

/// Every library the pack put beside the program (from its manifest).
pub fn libraries() -> Vec<String> {
    manifest_lines().into_iter().filter(|l| !l.starts_with('#')).collect()
}

/// The size of the pack's own `onnxruntime.dll`, recorded in the manifest:
/// reinstalling or updating OpenBark puts the processor-only one back.
fn recorded_runtime_size() -> Option<u64> {
    manifest_lines()
        .iter()
        .find_map(|l| l.strip_prefix("# onnxruntime.dll size=").and_then(|n| n.parse().ok()))
}

/// Whether the pack was in place when this process started, so the libraries
/// loaded are its own. Worked out once, at the first call (App start-up).
pub fn loaded_at_start() -> bool {
    static AT_START: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AT_START.get_or_init(|| {
        tidy();
        installed()
    })
}

/// The pack as a downloadable entry (not listed among the models).
pub fn entry() -> CatalogEntry {
    CatalogEntry {
        id: PACK_ID.into(),
        display_name: "GPU speech pack (NVIDIA)".into(),
        slot: Slot::Stt,
        license: "ONNX Runtime: MIT. CUDA runtime, cuBLAS, cuDNN: NVIDIA licence".into(),
        attribution: Some("ONNX Runtime by Microsoft (CUDA build from k2-fsa/sherpa-onnx); CUDA, cuBLAS and cuDNN by NVIDIA".into()),
        source: Source::Url,
        files: ARCHIVES
            .iter()
            .map(|a| FileSpec {
                path: a.name.into(),
                size: a.size,
                sha256: a.sha256.into(),
                git_sha1: None,
                url: Some(a.url.into()),
            })
            .collect(),
        layout: Layout::Archives,
        gated: false,
        mobile: false,
    }
}

/// Whether this computer could use the pack: Windows on x64.
pub fn supported() -> bool {
    cfg!(all(windows, target_arch = "x86_64"))
}

/// The NVIDIA graphics card, if the NVIDIA driver (with CUDA) is installed:
/// its name from `nvidia-smi`, or a generic name. Independent of how the
/// note model was built.
pub fn nvidia_gpu() -> Option<String> {
    static FOUND: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    FOUND.get_or_init(find_nvidia_gpu).clone()
}

fn find_nvidia_gpu() -> Option<String> {
    if !cfg!(windows) {
        return None;
    }
    let system =
        std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    if !system.join("System32").join("nvcuda.dll").is_file() {
        return None;
    }
    let mut cmd = std::process::Command::new(system.join("System32").join("nvidia-smi.exe"));
    cmd.args(["--query-gpu=name", "--format=csv,noheader"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashing up
    }
    let name = cmd
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.lines().next().map(|l| l.trim().to_string()))
        .filter(|s| !s.is_empty());
    Some(name.unwrap_or_else(|| "NVIDIA graphics card".to_string()))
}

/// The folder the program runs from, where the libraries go.
pub fn program_dir() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf))
}

/// Where the result of the GPU self-test is kept, beside the libraries.
pub fn check_file() -> Option<PathBuf> {
    program_dir().map(|d| d.join("gpu-check.txt"))
}

/// All of the pack's libraries are beside the program.
pub fn installed() -> bool {
    let libs = libraries();
    program_dir().is_some_and(|dir| {
        let runtime_ok = recorded_runtime_size()
            .is_some_and(|size| fs::metadata(dir.join("onnxruntime.dll")).is_ok_and(|m| m.len() == size));
        runtime_ok
            && libs.iter().any(|l| l == "onnxruntime_providers_cuda.dll")
            && libs.iter().all(|name| dir.join(name).is_file())
    })
}

/// Unpack the downloaded archives beside the program, write the manifest,
/// then delete the archives.
pub fn unpack(store: &ModelStore, mut progress: impl FnMut(usize, usize)) -> io::Result<()> {
    // `progress(done, _)` counts libraries placed.
    let dir = program_dir().ok_or_else(|| io::Error::other("can't tell where OpenBark is installed"))?;
    let pack = entry();
    let mut done = 0;
    let mut placed: Vec<String> = Vec::new();
    // Files the pack put here before may be replaced; anything else of the
    // same name is someone else's, kept as `<name>.orig` and put back by remove().
    let ours_before = libraries();
    // Tidy up a previous swap.
    let _ = fs::remove_file(dir.join("onnxruntime.dll.old"));
    for archive in &ARCHIVES {
        let path = store.file_path(&pack, archive.name);
        let mut found = vec![false; archive.take.len()];
        let mut wanted = |name: &str| -> bool {
            match archive.take.iter().position(|p| matches(p, name)) {
                Some(i) => {
                    found[i] = true;
                    true
                }
                None => false,
            }
        };
        let mut put = |name: &str, reader: &mut dyn Read| -> io::Result<()> {
            let tmp = dir.join(format!("{name}.new"));
            io::copy(reader, &mut fs::File::create(&tmp)?)?;
            let dest = dir.join(name);
            let shipped = matches!(name, "onnxruntime.dll" | "onnxruntime_providers_shared.dll");
            if dest.exists() && !shipped && !ours_before.iter().any(|n| n == name) {
                let orig = dir.join(format!("{name}.orig"));
                if !orig.exists() {
                    fs::rename(&dest, &orig)?;
                }
            }
            if dest.exists() && fs::remove_file(&dest).is_err() {
                // In use (onnxruntime.dll in the running program): rename it aside.
                let old = dir.join(format!("{name}.old"));
                let _ = fs::remove_file(&old);
                fs::rename(&dest, &old)?;
            }
            fs::rename(&tmp, &dest)?;
            done += 1;
            progress(done, 0);
            placed.push(name.to_string());
            Ok(())
        };
        if archive.name.ends_with(".tar.bz2") {
            // The GPU speech pack is Windows-only; bzip2 isn't linked on iOS.
            #[cfg(target_os = "ios")]
            return Err(io::Error::other("the GPU speech pack isn't available on this device"));
            #[cfg(not(target_os = "ios"))]
            {
                let file = fs::File::open(&path)?;
                let mut tar = tar::Archive::new(bzip2::read::BzDecoder::new(io::BufReader::new(file)));
                for item in tar.entries()? {
                    let mut item = item?;
                    let name = item
                        .path()?
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    // The archive has each library twice (bin/ and lib/); take lib/.
                    let in_lib = item.path()?.components().any(|c| c.as_os_str() == "lib");
                    if in_lib && name.ends_with(".dll") && wanted(&name) {
                        put(&name, &mut item)?;
                    }
                }
            }
        } else {
            let mut zip = zip::ZipArchive::new(fs::File::open(&path)?).map_err(io::Error::other)?;
            // Every local header must match its central directory entry (no
            // duplicate or overlapping entries), so no reader could see a
            // different file under the same entry.
            zip.validate_header_consistency()
                .map_err(|e| io::Error::other(format!("{} is not a consistent zip: {e}", archive.name)))?;
            for i in 0..zip.len() {
                let mut item = zip.by_index(i).map_err(io::Error::other)?;
                // Only the last component is used, and only when it is one of
                // the libraries listed above: nothing lands outside `dir`.
                let name = item
                    .name()
                    .map_err(io::Error::other)?
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or_default()
                    .to_string();
                if name.ends_with(".dll") && wanted(&name) {
                    put(&name, &mut item)?;
                }
            }
        }
        let missing: Vec<&str> =
            archive.take.iter().zip(&found).filter(|(_, f)| !**f).map(|(p, _)| *p).collect();
        if !missing.is_empty() {
            return Err(io::Error::other(format!("{} is missing {}", archive.name, missing.join(", "))));
        }
    }
    placed.sort();
    placed.dedup();
    let runtime = fs::metadata(dir.join("onnxruntime.dll"))?.len();
    if let Some(m) = manifest() {
        let mut text = format!("# onnxruntime.dll size={runtime}\n");
        for name in &placed {
            text.push_str(name);
            text.push('\n');
        }
        fs::write(m, text)?;
    }
    if let Some(f) = check_file() {
        let _ = fs::remove_file(f);
    }
    // The archives aren't needed once unpacked.
    let _ = fs::remove_dir_all(store.model_dir(&pack));
    Ok(())
}

/// Remove the pack's libraries. ONNX Runtime goes back to the processor-only
/// build only when the app is reinstalled; with the CUDA provider gone it
/// runs on the processor anyway.
pub fn remove() -> io::Result<()> {
    let dir = program_dir().ok_or_else(|| io::Error::other("can't tell where OpenBark is installed"))?;
    if let Some(f) = check_file() {
        let _ = fs::remove_file(f);
    }
    for name in
        libraries().iter().filter(|n| *n != "onnxruntime.dll" && *n != "onnxruntime_providers_shared.dll")
    {
        let path = dir.join(name);
        if path.exists() && fs::remove_file(&path).is_err() {
            // Loaded right now: rename it aside so it's gone after a restart.
            fs::rename(&path, dir.join(format!("{name}.old")))?;
        }
        // A file of the same name that was here before the pack goes back.
        let orig = dir.join(format!("{name}.orig"));
        if orig.exists() && !path.exists() {
            let _ = fs::rename(&orig, &path);
        }
    }
    if let Some(m) = manifest() {
        let _ = fs::remove_file(m);
    }
    Ok(())
}

/// Delete libraries renamed aside by an earlier swap or removal (start-up):
/// `<name>.old` where the name is one the pack places (the manifest may be
/// gone after a removal, so this goes by the pack's own names and patterns).
pub fn tidy() {
    let Some(dir) = program_dir() else { return };
    let Ok(entries) = fs::read_dir(&dir) else { return };
    for entry in entries.flatten() {
        let file = entry.file_name().to_string_lossy().into_owned();
        if let Some(name) = file.strip_suffix(".old")
            && is_pack_library(name)
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// A library name the pack places: one of its archives' names or patterns.
fn is_pack_library(name: &str) -> bool {
    ARCHIVES.iter().any(|a| a.take.iter().any(|p| matches(p, name)))
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn patterns_pick_cudnn_libraries() {
        assert!(matches("cudnn*.dll", "cudnn_engines_tensor_ir64_9.dll"));
        assert!(matches("cudnn*.dll", "cudnn64_9.dll"));
        assert!(!matches("cudnn*.dll", "cudnn64_9.lib"));
        assert!(matches("cudart64_12.dll", "cudart64_12.dll"));
        assert!(!matches("cudart64_12.dll", "cudart32_12.dll"));
        assert!(super::is_pack_library("cudnn_adv64_9.dll") && super::is_pack_library("onnxruntime.dll"));
        assert!(!super::is_pack_library("someone-elses.dll"));
    }
}
