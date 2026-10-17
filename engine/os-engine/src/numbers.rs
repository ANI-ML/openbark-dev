//! Number extraction and spoken-number normalisation for faithfulness checks.
//!
//! ASR may write "thirty nine point two" where the note says "39.2"; both
//! sides are reduced to canonical digit strings before comparing.

/// Canonical form: no leading zeros on the integer part, no trailing zeros
/// after a decimal point ("039.20" → "39.2", "5.0" → "5").
pub fn canonical(n: &str) -> String {
    let (int, frac) = n.split_once('.').unwrap_or((n, ""));
    let int = int.trim_start_matches('0');
    let int = if int.is_empty() { "0" } else { int };
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() { int.to_string() } else { format!("{int}.{frac}") }
}

/// All numbers written with digits in `text`, canonicalised.
pub fn extract_numbers(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == ',') {
                i += 1;
            }
            if i + 1 < chars.len() && chars[i] == '.' && chars[i + 1].is_ascii_digit() {
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let raw: String = chars[start..i].iter().filter(|c| **c != ',').collect();
            out.push(canonical(&raw));
        } else {
            i += 1;
        }
    }
    out
}

fn unit(word: &str) -> Option<u64> {
    Some(match word {
        "zero" | "oh" => 0,
        "one" => 1,
        "two" => 2,
        "three" => 3,
        "four" => 4,
        "five" => 5,
        "six" => 6,
        "seven" => 7,
        "eight" => 8,
        "nine" => 9,
        "ten" => 10,
        "eleven" => 11,
        "twelve" => 12,
        "thirteen" => 13,
        "fourteen" => 14,
        "fifteen" => 15,
        "sixteen" => 16,
        "seventeen" => 17,
        "eighteen" => 18,
        "nineteen" => 19,
        "twenty" => 20,
        "thirty" => 30,
        "forty" => 40,
        "fifty" => 50,
        "sixty" => 60,
        "seventy" => 70,
        "eighty" => 80,
        "ninety" => 90,
        _ => return None,
    })
}

fn digit(word: &str) -> Option<u64> {
    unit(word).filter(|d| *d < 10)
}

/// Replace runs of spelled-out numbers with digits:
/// "thirty nine point two degrees" → "39.2 degrees",
/// "one hundred and twenty" → "120", "a half" stays as words.
pub fn normalize_spoken(text: &str) -> String {
    let lower = text.to_lowercase();
    let words: Vec<&str> =
        lower.split(|c: char| c.is_whitespace() || c == '-').filter(|w| !w.is_empty()).collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let bare = |w: &str| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string();
        let w = bare(words[i]);
        let and_a_half = |at: usize| {
            at + 2 < words.len()
                && bare(words[at]) == "and"
                && bare(words[at + 1]) == "a"
                && bare(words[at + 2]) == "half"
        };
        if unit(&w).is_none() {
            if !w.is_empty() && w.chars().all(|c| c.is_ascii_digit()) && and_a_half(i + 1) {
                out.push(format!("{w}.5")); // "4 and a half"
                i += 4;
            } else if w == "half" {
                out.push("0.5".into()); // "half a kilo", "a half"
                i += 1;
            } else {
                out.push(words[i].to_string());
                i += 1;
            }
            continue;
        }
        let mut total: u64 = 0;
        let mut current: u64 = 0;
        let mut consumed_any = false;
        while i < words.len() {
            let w = bare(words[i]);
            if let Some(v) = unit(&w) {
                current += v;
            } else if w == "hundred" && consumed_any {
                current = current.max(1) * 100;
            } else if w == "thousand" && consumed_any {
                total += current.max(1) * 1000;
                current = 0;
            } else if w == "and" && consumed_any && i + 1 < words.len() && unit(&bare(words[i + 1])).is_some()
            {
                // "one hundred and twenty"
            } else {
                break;
            }
            consumed_any = true;
            let ends_word = words[i].ends_with(|c: char| !c.is_alphanumeric());
            i += 1;
            if ends_word {
                break;
            }
        }
        let mut number = (total + current).to_string();
        // Decimal part: "point" followed by single digits.
        if i + 1 < words.len() && bare(words[i]) == "point" && digit(&bare(words[i + 1])).is_some() {
            i += 1;
            number.push('.');
            while i < words.len() {
                match digit(&bare(words[i])) {
                    Some(d) => {
                        number.push_str(&d.to_string());
                        i += 1;
                    }
                    None => break,
                }
            }
        } else if and_a_half(i) {
            number.push_str(".5");
            i += 3;
        }
        out.push(number);
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalises() {
        assert_eq!(canonical("039.20"), "39.2");
        assert_eq!(canonical("5.0"), "5");
        assert_eq!(canonical("0.5"), "0.5");
        assert_eq!(canonical("000"), "0");
    }

    #[test]
    fn extracts_digit_numbers() {
        assert_eq!(extract_numbers("T 39.2°C, HR 120 bpm, 4.20 kg"), vec!["39.2", "120", "4.2"]);
        assert_eq!(extract_numbers("q8h for 7 days; 1,250 mg"), vec!["8", "7", "1250"]);
        assert_eq!(extract_numbers("BID PO, end of sentence 3."), vec!["3"]);
        assert!(extract_numbers("no numbers here").is_empty());
    }

    #[test]
    fn normalises_spoken_numbers() {
        assert_eq!(normalize_spoken("thirty nine point two degrees"), "39.2 degrees");
        assert_eq!(normalize_spoken("Heart rate one hundred and twenty."), "heart rate 120");
        assert_eq!(normalize_spoken("every eight hours for seven days"), "every 8 hours for 7 days");
        assert_eq!(normalize_spoken("twenty-two kilos"), "22 kilos");
        assert_eq!(normalize_spoken("give one, then two"), "give 1 then 2");
        assert_eq!(normalize_spoken("point blank"), "point blank");
        assert_eq!(normalize_spoken("a one and a half inch line"), "a 1.5 inch line");
        assert_eq!(normalize_spoken("lost about half a kilo"), "lost about 0.5 a kilo");
        assert_eq!(normalize_spoken("4 and a half weeks"), "4.5 weeks");
    }

    #[test]
    fn spoken_then_extracted_matches_note() {
        let transcript = normalize_spoken(
            "Her temperature is thirty-eight point nine and she weighs four point two kilos.",
        );
        let found = extract_numbers(&transcript);
        assert!(found.contains(&"38.9".to_string()));
        assert!(found.contains(&"4.2".to_string()));
    }
}
