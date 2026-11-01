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

/// All numbers written with digits in `text`, canonicalised. A decimal with
/// no leading digit counts as one: speech recognition writes "point one" as
/// ".1", sometimes glued to the drug ("hydromorphone.1"), and that is 0.1,
/// never 1.
pub fn extract_numbers(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let bare_decimal = chars[i] == '.'
            && chars.get(i + 1).is_some_and(|c| c.is_ascii_digit())
            && (i == 0 || !chars[i - 1].is_ascii_digit());
        if bare_decimal {
            let start = i + 1;
            i += 1;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            let digits: String = chars[start..i].iter().collect();
            out.push(canonical(&format!("0.{digits}")));
        } else if chars[i].is_ascii_digit() {
            let mut raw = String::new();
            let mut decimal = false;
            while i < chars.len() {
                let c = chars[i];
                if c.is_ascii_digit() {
                    raw.push(c);
                    i += 1;
                    continue;
                }
                if c != ',' || decimal {
                    break;
                }
                // A comma: "1,250" groups thousands; "0,5" and "38,6" are
                // the decimal comma of French (and most of Europe).
                let after = chars[i + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
                let zero = raw.trim_start_matches('0').is_empty();
                if after == 0 {
                    break;
                }
                if (1..=2).contains(&after) || (after == 3 && zero) {
                    raw.push('.');
                    decimal = true;
                }
                i += 1;
            }
            if !decimal && i + 1 < chars.len() && chars[i] == '.' && chars[i + 1].is_ascii_digit() {
                raw.push('.');
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    raw.push(chars[i]);
                    i += 1;
                }
            }
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

/// "fourth premolar" is 4 ("4th"); only the small ordinals said in clinics.
fn ordinal(word: &str) -> Option<u64> {
    Some(match word {
        "first" | "1st" => 1,
        "second" | "2nd" => 2,
        "third" | "3rd" => 3,
        "fourth" | "4th" => 4,
        "fifth" | "5th" => 5,
        "sixth" | "6th" => 6,
        "seventh" | "7th" => 7,
        "eighth" | "8th" => 8,
        "ninth" | "9th" => 9,
        "tenth" | "10th" => 10,
        _ => return None,
    })
}

fn digit(word: &str) -> Option<u64> {
    unit(word).filter(|d| *d < 10)
}

/// French number words that are not English words: one of these starts a
/// French number. "six" and "zero" are English too, so they start one only
/// before "virgule" or one of these; "cent" only next to another number word
/// ("per cent" stays as it is); "un" and "une" are also the article.
fn french_unit(word: &str) -> Option<u64> {
    Some(match word {
        "zéro" => 0,
        "deux" => 2,
        "trois" => 3,
        "quatre" => 4,
        "cinq" => 5,
        "sept" => 7,
        "huit" => 8,
        "neuf" => 9,
        "dix" => 10,
        "onze" => 11,
        "douze" => 12,
        "treize" => 13,
        "quatorze" => 14,
        "quinze" => 15,
        "seize" => 16,
        "vingt" | "vingts" => 20,
        "trente" => 30,
        "quarante" => 40,
        "cinquante" => 50,
        "soixante" => 60,
        _ => return None,
    })
}

/// [`french_unit`] plus the words that are only French numbers inside one.
fn french_value(word: &str) -> Option<u64> {
    french_unit(word).or(match word {
        "zero" => Some(0),
        "un" | "une" => Some(1),
        "six" => Some(6),
        _ => None,
    })
}

/// A whole French number starting at `words[i]`: its digits ("38.6" for
/// "trente-huit virgule six", "180" for "cent quatre-vingts", "0.5" for
/// "zéro virgule cinq") and how many words it took, or None.
fn french_number(words: &[&str], i: usize) -> Option<(String, usize)> {
    let bare = |w: &str| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string();
    let ends = |k: usize| words[k].ends_with(|c: char| !c.is_alphanumeric());
    let first = bare(words[i]);
    let next = words.get(i + 1).map(|w| bare(w)).unwrap_or_default();
    let starts = french_unit(&first).is_some()
        || (french_value(&first).is_some()
            && !ends(i)
            && (next == "virgule" || french_unit(&next).is_some()))
        || (first == "cent" && !ends(i) && french_value(&next).is_some());
    if !starts {
        return None;
    }
    let (int, mut k) = french_integer(words, i)?;
    let mut number = int.to_string();
    // "virgule" and the decimals: "virgule cinq" (.5), "virgule zéro cinq" (.05).
    if k < words.len() && !ends(k - 1) && bare(words[k]) == "virgule" && k + 1 < words.len() {
        let w = bare(words[k + 1]);
        if matches!(w.as_str(), "zéro" | "zero") {
            let mut digits = String::new();
            let mut j = k + 1;
            while j < words.len() {
                match french_value(&bare(words[j])).filter(|d| *d < 10) {
                    Some(d) => digits.push_str(&d.to_string()),
                    None => break,
                }
                j += 1;
                if ends(j - 1) {
                    break;
                }
            }
            number = format!("{number}.{digits}");
            k = j;
        } else if french_value(&w).is_some()
            && let Some((frac, j)) = french_integer(words, k + 1)
        {
            number = format!("{number}.{frac}");
            k = j;
        }
    }
    Some((canonical(&number), k - i))
}

/// The integer part of a French number at `words[i]`, and the index after it.
fn french_integer(words: &[&str], i: usize) -> Option<(u64, usize)> {
    let bare = |w: &str| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string();
    let (mut total, mut current, mut k, mut any) = (0u64, 0u64, i, false);
    while k < words.len() {
        let w = bare(words[k]);
        let tens = current % 100;
        if let Some(v) = french_value(&w) {
            if !any {
                current = v;
            } else if v == 20 && tens == 4 {
                current += 76; // quatre-vingts
            } else if (tens == 60 || tens == 80) && (10..20).contains(&v) {
                current += v; // soixante-dix, quatre-vingt-douze
            } else if tens >= 20 && tens.is_multiple_of(10) && v < 10 {
                current += v; // vingt-deux, cent quatre-vingt-un
            } else if tens == 10 && (7..10).contains(&v) {
                current += v; // dix-sept
            } else if tens == 0 && (current > 0 || total > 0) && v < 100 {
                current += v; // cent deux, mille cinq
            } else {
                break;
            }
        } else if matches!(w.as_str(), "cent" | "cents") && (any || k == i) {
            current = current.max(1) * 100;
        } else if w == "mille" {
            total += current.max(1) * 1000;
            current = 0;
        } else if w == "et"
            && any
            && current.is_multiple_of(10)
            && current % 100 >= 20
            && words.get(k + 1).is_some_and(|n| matches!(bare(n).as_str(), "un" | "une" | "onze"))
        {
            // vingt et un, soixante et onze
        } else {
            break;
        }
        any = true;
        let ended = words[k].ends_with(|c: char| !c.is_alphanumeric());
        k += 1;
        if ended {
            break;
        }
    }
    any.then_some((total + current, k))
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
        // French: "trente-huit virgule six" is 38.6 (none of its words are English).
        if let Some((number, used)) = french_number(&words, i) {
            out.push(number);
            i += used;
            continue;
        }
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
            } else if let Some(n) = ordinal(&w) {
                out.push(n.to_string()); // "fourth premolar", "4th"
                i += 1;
            } else if w == "point" && i + 1 < words.len() && digit(&bare(words[i + 1])).is_some() {
                // "point seven milligrams" is 0.7
                let mut number = String::from("0.");
                i += 1;
                while i < words.len() {
                    match digit(&bare(words[i])) {
                        Some(d) => number.push_str(&d.to_string()),
                        None => break,
                    }
                    let ends_word = words[i].ends_with(|c: char| !c.is_alphanumeric());
                    i += 1;
                    if ends_word {
                        break;
                    }
                }
                out.push(canonical(&number));
            } else {
                out.push(words[i].to_string());
                i += 1;
            }
            continue;
        }
        // Said digit by digit: "one oh eight" (a tooth), "four oh four".
        let run: Vec<u64> = words[i..]
            .iter()
            .enumerate()
            .scan(false, |ended, (k, w)| {
                if *ended {
                    return None;
                }
                *ended = w.ends_with(|c: char| !c.is_alphanumeric());
                // Recognisers sometimes write the "oh" of "four oh four" as the letter o.
                if k > 0 && bare(w) == "o" { Some(0) } else { digit(&bare(w)) }
            })
            .collect();
        let has_oh =
            words[i..i + run.len()].iter().skip(1).any(|w| matches!(bare(w).as_str(), "oh" | "o" | "zero"));
        if run.len() >= 3 || (run.len() == 2 && has_oh) {
            out.push(run.iter().map(u64::to_string).collect::<String>());
            i += run.len();
            continue;
        }
        let mut total: u64 = 0;
        let mut current: u64 = 0;
        let mut consumed_any = false;
        while i < words.len() {
            let w = bare(words[i]);
            if let Some(v) = unit(&w) {
                // "thirty eight" and "one hundred eighty" combine; "three forty" (a time) doesn't.
                let combines = !consumed_any
                    || (current.is_multiple_of(10) && current % 100 >= 20 && v < 10)
                    || (current >= 100 && current.is_multiple_of(100) && v < 100);
                if !combines {
                    break;
                }
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
    fn a_decimal_without_its_leading_zero_is_still_a_decimal() {
        assert_eq!(extract_numbers("hydromorphone.1 mg per kilo"), ["0.1"]);
        assert_eq!(extract_numbers("with a sepromazine.02 mg, then .5 mL"), ["0.02", "0.5"]);
        assert_eq!(extract_numbers("Weight 24.5 kilos. 3 days."), ["24.5", "3"]);
    }

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
        assert_eq!(normalize_spoken("heart rate one hundred eighty"), "heart rate 180");
        assert_eq!(
            normalize_spoken("One oh eight has a fracture, four oh four looks fine"),
            "108 has a fracture, 404 looks fine"
        );
        assert_eq!(normalize_spoken("four o four The lower right canine"), "404 the lower right canine");
        assert_eq!(normalize_spoken("She passed at three forty."), "she passed at 3 40");
        assert_eq!(normalize_spoken("vincristine, point seven milligrams"), "vincristine, 0.7 milligrams");
        assert_eq!(normalize_spoken("twenty four point five kilos"), "24.5 kilos");
        assert_eq!(normalize_spoken("every eight hours for seven days"), "every 8 hours for 7 days");
        assert_eq!(normalize_spoken("twenty-two kilos"), "22 kilos");
        assert_eq!(normalize_spoken("give one, then two"), "give 1 then 2");
        assert_eq!(normalize_spoken("point blank"), "point blank");
        assert_eq!(normalize_spoken("a one and a half inch line"), "a 1.5 inch line");
        assert_eq!(normalize_spoken("lost about half a kilo"), "lost about 0.5 a kilo");
        assert_eq!(normalize_spoken("4 and a half weeks"), "4.5 weeks");
    }

    #[test]
    fn a_decimal_comma_is_a_decimal_and_a_thousands_comma_is_not() {
        assert_eq!(extract_numbers("Maxitrol 0,5 cm, T 38,6 °C, 5,4 mg"), ["0.5", "38.6", "5.4"]);
        assert_eq!(extract_numbers("0,125 mg"), ["0.125"]);
        // English thousands, as before.
        assert_eq!(extract_numbers("1,250 mg and 12,500 units, 1,000,000"), ["1250", "12500", "1000000"]);
        assert_eq!(extract_numbers("teeth 101,102 and 3, then 4"), ["101102", "3", "4"]);
        assert_eq!(extract_numbers("38,6.2"), ["38.6", "2"]);
    }

    #[test]
    fn normalises_french_spoken_numbers() {
        let n = |s: &str| normalize_spoken(s);
        assert_eq!(n("température trente-huit virgule six"), "température 38.6");
        assert_eq!(n("zéro virgule cinq centimètre"), "0.5 centimètre");
        assert_eq!(n("cinq virgule quatre milligrammes"), "5.4 milligrammes");
        assert_eq!(n("virgule zéro cinq"), "virgule 0 5");
        assert_eq!(n("un virgule zéro cinq"), "1.05");
        assert_eq!(n("cent quatre-vingts battements"), "180 battements");
        assert_eq!(n("quatre-vingt-dix-neuf"), "99");
        assert_eq!(n("soixante et onze, soixante-dix-sept"), "71 77");
        assert_eq!(n("vingt et un jours"), "21 jours");
        assert_eq!(n("deux cent cinquante grammes"), "250 grammes");
        assert_eq!(n("dix-sept ans"), "17 ans");
        assert_eq!(n("trois fois par jour pendant dix jours"), "3 fois par jour pendant 10 jours");
        assert_eq!(n("grade deux sur six"), "grade 2 sur 6");
        assert_eq!(n("deux mille cinq"), "2005");
        // "un" and "une" stay words unless they start a number.
        assert_eq!(n("une fois par jour, un comprimé"), "une fois par jour, un comprimé");
        assert_eq!(n("six ans"), "6 ans");
        // Two numbers side by side stay two numbers.
        assert_eq!(n("cinq ou six jours"), "5 ou 6 jours");
        assert_eq!(n("trois quatre"), "3 4");
    }

    #[test]
    fn french_number_words_leave_english_alone() {
        assert_eq!(normalize_spoken("ten per cent of six hundred"), "10 per cent of 600");
        assert_eq!(normalize_spoken("six cents"), "6 cents");
        assert_eq!(normalize_spoken("zero point five"), "0.5");
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
