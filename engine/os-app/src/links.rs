//! Web pages the desktop opens in the default browser when a link is pressed:
//! NVIDIA's licence for the GPU speech pack, a Hugging Face model's page and
//! Hugging Face's token page. The main window has no general opener
//! (apps/desktop/src-tauri/capabilities/default.json); it names a page and
//! the address is built here.

use crate::{AppError, Result};

/// NVIDIA's licence for the libraries in the GPU speech pack (ADR-0014).
pub const NVIDIA_LICENCE: &str = "https://docs.nvidia.com/deeplearning/cudnn/latest/reference/eula.html";

/// Where a Hugging Face account makes an access token.
pub const HF_TOKENS: &str = "https://huggingface.co/settings/tokens";

/// The address of `page`: "nvidia-licence", "hf-tokens", or "hf-repo" with
/// `repo` a Hugging Face model id ("owner/name"). Anything else is refused.
pub fn web_link(page: &str, repo: Option<&str>) -> Result<String> {
    match page {
        "nvidia-licence" => Ok(NVIDIA_LICENCE.to_string()),
        "hf-tokens" => Ok(HF_TOKENS.to_string()),
        "hf-repo" => match repo {
            Some(id) if is_repo_id(id) => Ok(format!("https://huggingface.co/{id}")),
            _ => Err(AppError("That isn't a Hugging Face model.".into())),
        },
        _ => Err(AppError("OpenBark doesn't open that page.".into())),
    }
}

/// "owner/name": two non-empty parts of letters, digits, '-', '_' and '.',
/// neither starting with '.', so the address stays a model page.
fn is_repo_id(id: &str) -> bool {
    let mut parts = id.split('/');
    let (Some(owner), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    id.len() <= 200
        && [owner, name].iter().all(|p| {
            !p.is_empty()
                && !p.starts_with('.')
                && p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_pages() {
        assert_eq!(web_link("nvidia-licence", None).unwrap(), NVIDIA_LICENCE);
        assert_eq!(web_link("hf-tokens", Some("ignored/repo")).unwrap(), HF_TOKENS);
    }

    #[test]
    fn a_model_page() {
        assert_eq!(
            web_link("hf-repo", Some("google/gemma-3n-E4B-it")).unwrap(),
            "https://huggingface.co/google/gemma-3n-E4B-it"
        );
        assert_eq!(
            web_link("hf-repo", Some("unsloth/Qwen3_4B.v2-GGUF")).unwrap(),
            "https://huggingface.co/unsloth/Qwen3_4B.v2-GGUF"
        );
    }

    #[test]
    fn refuses_anything_else() {
        for id in [
            "",
            "gemma",
            "/gemma",
            "google/",
            "a/b/c",
            "../settings",
            "google/..",
            "google/.hidden",
            "google/gemma?x=1",
            "google/gemma#top",
            "https://evil.example/x",
            "google/gemma name",
            "google\\gemma",
            "google/gémma",
        ] {
            assert!(web_link("hf-repo", Some(id)).is_err(), "{id:?}");
        }
        assert!(web_link("hf-repo", None).is_err());
        assert!(web_link("https://evil.example", None).is_err());
        assert!(web_link("", None).is_err());
    }
}
