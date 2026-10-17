//! Chat formatting from the model's own Jinja template.
//!
//! llama.cpp's built-in `llama_chat_apply_template` recognises templates by
//! heuristics and only knows the Gemma 1-3 `<start_of_turn>` format. Rendering
//! the GGUF's `tokenizer.chat_template` with minijinja works for any model
//! family, Gemma 4 included, without hard-coding turn markers.

use minijinja::value::Serde;
use minijinja::{Environment, Error, ErrorKind, Value, context};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: "system".into(), content: content.into() }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".into(), content: content.into() }
    }
}

/// Render `messages` with a Hugging Face–style Jinja chat template.
pub fn render(
    template: &str,
    messages: &[ChatMessage],
    bos_token: &str,
    eos_token: &str,
) -> Result<String, Error> {
    let mut env = Environment::new();
    minijinja_contrib::add_to_environment(&mut env);
    env.set_unknown_method_callback(minijinja_contrib::pycompat::unknown_method_callback);
    env.add_function("raise_exception", |msg: String| -> Result<String, Error> {
        Err(Error::new(ErrorKind::InvalidOperation, msg))
    });
    env.add_function("strftime_now", |_fmt: String| -> String { String::new() });
    env.add_template("chat", template)?;
    env.get_template("chat")?.render(context! {
        messages => Value::from(Serde(messages)),
        add_generation_prompt => true,
        bos_token => bos_token,
        eos_token => eos_token,
        // Gemma 4 and other reasoning models: notes are generated directly.
        enable_thinking => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Abridged Gemma 1-3 style template, enough to exercise system merging,
    // role mapping and the generation prompt.
    const GEMMA_STYLE: &str = "{{ bos_token }}{% if messages[0]['role'] == 'system' %}{% set first = messages[0]['content'] + '\n\n' %}{% set loop_messages = messages[1:] %}{% else %}{% set first = '' %}{% set loop_messages = messages %}{% endif %}{% for message in loop_messages %}{% set role = 'model' if message['role'] == 'assistant' else message['role'] %}<start_of_turn>{{ role }}\n{{ (first if loop.first else '') + message['content'] | trim }}<end_of_turn>\n{% endfor %}{% if add_generation_prompt %}<start_of_turn>model\n{% endif %}";

    #[test]
    fn renders_system_merged_into_first_user_turn() {
        let out = render(
            GEMMA_STYLE,
            &[ChatMessage::system("Be terse."), ChatMessage::user("Hello")],
            "<bos>",
            "<eos>",
        )
        .unwrap();
        assert_eq!(out, "<bos><start_of_turn>user\nBe terse.\n\nHello<end_of_turn>\n<start_of_turn>model\n");
    }

    #[test]
    fn raise_exception_surfaces_as_error() {
        let err = render("{{ raise_exception('nope') }}", &[], "", "").unwrap_err();
        assert!(err.to_string().contains("nope"));
    }

    #[test]
    fn python_string_methods_work() {
        let out =
            render("{{ messages[0].content.strip() }}", &[ChatMessage::user("  hi  ")], "", "").unwrap();
        assert_eq!(out, "hi");
    }
}
