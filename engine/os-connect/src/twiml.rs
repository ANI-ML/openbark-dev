//! The text of the clinic's two TwiML Bins (ADR-0023 section 3).
//!
//! Twilio has no documented API for Bins, so OpenBark shows this text with a
//! copy button, and the clinic pastes each into a Bin in its console and
//! copies the Bin's URL back (owner decision 3: Bins, no code deployed into
//! the clinic's account).
//!
//! Neither text asks Twilio to record: `<Dial>` records nothing unless told
//! to, and OpenBark records on the computer (ADR-0023 principle 5).
//!
//! `{{To}}`, `{{#e164}}` and `{{CallSid}}` are Twilio's Bin templates, filled
//! in by Twilio for each call. Clinic-typed text is escaped for XML and may
//! not contain `{{` or `}}`, so it can't become a template.

use crate::ConnectError;
use crate::settings::{check_computer_name, check_e164, identity_for};

/// What callers hear when no computer answers within 25 seconds.
pub const DEFAULT_MISSED_CALL: &str =
    "Sorry, we couldn't take your call. Please try the clinic's main number.";

/// One `<Dial>` rings at most ten `<Client>`s (https://www.twilio.com/docs/voice/twiml/client).
pub const MAX_COMPUTERS: usize = 10;

/// Longest missed-call sentence accepted.
pub const MAX_MISSED_CALL: usize = 500;

/// The "OpenBark outgoing" Bin: dials the number the phone window passes as
/// `To`, from the clinic's caller ID. `answerOnBridge` keeps the clinic
/// hearing ringing until the client answers; calls stop after an hour.
pub fn outgoing_bin(caller_id: &str) -> Result<String, ConnectError> {
    check_e164(caller_id)?;
    Ok(format!(
        r#"<Response>
  <Dial callerId="{caller_id}" answerOnBridge="true" timeLimit="3600">
    <Number>{{{{#e164}}}}{{{{To}}}}{{{{/e164}}}}</Number>
  </Dial>
</Response>"#
    ))
}

/// The "OpenBark incoming" Bin: rings every listed computer for 25 seconds,
/// passing the caller's call SID as `parent` (so OpenBark can end that leg
/// when it hangs up), then says the missed-call sentence. Not used until
/// incoming calls ship (ADR-0023 section 15, decision 2).
pub fn incoming_bin(computer_names: &[&str], missed_call: &str) -> Result<String, ConnectError> {
    if computer_names.is_empty() || computer_names.len() > MAX_COMPUTERS {
        return Err(ConnectError::Invalid(format!(
            "List between 1 and {MAX_COMPUTERS} computers to ring. Twilio rings at most {MAX_COMPUTERS} at once."
        )));
    }
    let mut identities: Vec<String> = Vec::with_capacity(computer_names.len());
    for name in computer_names {
        check_computer_name(name)?;
        let id = identity_for(name);
        if identities.contains(&id) {
            return Err(ConnectError::Invalid(format!("{name} is listed twice.")));
        }
        identities.push(id);
    }
    let sentence = missed_call.trim();
    if sentence.is_empty() || sentence.chars().count() > MAX_MISSED_CALL {
        return Err(ConnectError::Invalid(format!(
            "Write the missed-call sentence in up to {MAX_MISSED_CALL} characters."
        )));
    }
    if sentence.contains("{{") || sentence.contains("}}") || sentence.chars().any(char::is_control) {
        return Err(ConnectError::Invalid(
            "The missed-call sentence can't contain {{, }} or line breaks.".into(),
        ));
    }

    let mut out = String::from("<Response>\n  <Dial timeout=\"25\" answerOnBridge=\"true\">\n");
    for id in &identities {
        out.push_str(&format!(
            "    <Client>\n      <Identity>{id}</Identity>\n      <Parameter name=\"parent\" value=\"{{{{CallSid}}}}\"/>\n    </Client>\n"
        ));
    }
    out.push_str(&format!("  </Dial>\n  <Say>{}</Say>\n</Response>", escape_text(sentence)));
    Ok(out)
}

/// Escapes element text for XML. Apostrophes and quotes are fine in text.
fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outgoing_bin_is_the_text_in_adr_0023() {
        assert_eq!(
            outgoing_bin("+15555550100").unwrap(),
            r#"<Response>
  <Dial callerId="+15555550100" answerOnBridge="true" timeLimit="3600">
    <Number>{{#e164}}{{To}}{{/e164}}</Number>
  </Dial>
</Response>"#
        );
    }

    #[test]
    fn incoming_bin_is_the_text_in_adr_0023() {
        // The ADR's example identity is "openbark-front-desk"; Twilio allows
        // only letters, digits and underscores in Voice identities.
        assert_eq!(
            incoming_bin(&["front-desk"], DEFAULT_MISSED_CALL).unwrap(),
            r#"<Response>
  <Dial timeout="25" answerOnBridge="true">
    <Client>
      <Identity>openbark_front_desk</Identity>
      <Parameter name="parent" value="{{CallSid}}"/>
    </Client>
  </Dial>
  <Say>Sorry, we couldn't take your call. Please try the clinic's main number.</Say>
</Response>"#
        );
    }

    #[test]
    fn incoming_bin_rings_each_computer() {
        let text = incoming_bin(&["front-desk", "exam-2"], "Please call back later.").unwrap();
        assert_eq!(text.matches("<Client>").count(), 2);
        assert!(text.contains("<Identity>openbark_exam_2</Identity>"));
        assert_eq!(text.matches(r#"<Parameter name="parent" value="{{CallSid}}"/>"#).count(), 2);
        let ten: Vec<String> = (0..10).map(|i| format!("pc{i}")).collect();
        let ten: Vec<&str> = ten.iter().map(String::as_str).collect();
        incoming_bin(&ten, DEFAULT_MISSED_CALL).unwrap();
        let eleven = [ten.as_slice(), &["pc10"]].concat();
        assert!(incoming_bin(&eleven, DEFAULT_MISSED_CALL).is_err(), "Twilio rings at most ten");
        assert!(incoming_bin(&[], DEFAULT_MISSED_CALL).is_err());
        assert!(incoming_bin(&["front-desk", "front_desk"], DEFAULT_MISSED_CALL).is_err(), "same identity");
        assert!(incoming_bin(&["Front Desk"], DEFAULT_MISSED_CALL).is_err());
    }

    #[test]
    fn typed_text_cannot_break_the_xml_or_become_a_template() {
        let text = incoming_bin(&["lab"], "Cats & dogs <3 \"welcome\"").unwrap();
        assert!(text.contains("<Say>Cats &amp; dogs &lt;3 \"welcome\"</Say>"));
        assert!(incoming_bin(&["lab"], "Call {{From}} back").is_err());
        assert!(incoming_bin(&["lab"], "two\nlines").is_err());
        assert!(incoming_bin(&["lab"], "   ").is_err());
        assert!(outgoing_bin("+1555\"/><Number>+19005550100").is_err());
        assert!(outgoing_bin("15555550100").is_err());
    }

    #[test]
    fn neither_bin_asks_twilio_to_record() {
        let out = outgoing_bin("+15555550100").unwrap();
        let inc = incoming_bin(&["front-desk"], DEFAULT_MISSED_CALL).unwrap();
        for text in [out, inc] {
            assert!(!text.to_lowercase().contains("record"), "{text}");
        }
    }
}
