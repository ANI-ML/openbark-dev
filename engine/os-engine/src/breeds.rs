//! Dog and cat breeds, so that a breed said alone counts as saying the
//! species ("Coco is a seven month old Yorkshire terrier" says she is a dog).
//!
//! The model drafts the Patient section as said, and small models put a
//! breed said alone into Species ("Species: Yorkshire terrier"). After
//! drafting, [`species_from_breed`] writes Species "Dog" or "Cat" from this
//! table when Species is empty or holds a breed, and the breed's cited line
//! names a breed of that species; the breed moves to Breed when that is
//! empty. Nothing else is filled: an unknown breed or another animal's
//! breed leaves Species as drafted. The faithfulness pass accepts Species
//! dog or cat only when a cited line says the species or names one of its
//! breeds here, and flags a breed written as the species. Breeds that are also
//! the names of other animals' breeds (Abyssinian and Himalayan guinea pigs
//! and rabbits, Rex and Angora rabbits, Shetland ponies, Arabian horses) are
//! left out or listed only in a form that is unambiguous ("devon rex",
//! "shetland sheepdog"), so they imply nothing. Matching is on whole words,
//! case-insensitive, and a trailing plural "s" is allowed.

use serde_json::{Value, json};

use crate::transcript::Transcript;

/// The species a breed implies, with the breeds as said (lower case).
pub const BREEDS: &[(&str, &[&str])] = &[
    (
        "dog",
        &[
            "affenpinscher",
            "afghan hound",
            "airedale",
            "akita",
            "alaskan malamute",
            "malamute",
            "australian cattle dog",
            "australian shepherd",
            "basenji",
            "basset hound",
            "beagle",
            "bearded collie",
            "bernese mountain dog",
            "bichon frise",
            "bichon",
            "bloodhound",
            "border collie",
            "border terrier",
            "borzoi",
            "boston terrier",
            "boxer",
            "boykin spaniel",
            "brittany spaniel",
            "bull terrier",
            "bulldog",
            "bullmastiff",
            "cairn terrier",
            "cane corso",
            "cavalier king charles spaniel",
            "cavapoo",
            "chesapeake bay retriever",
            "chihuahua",
            "chinese crested",
            "chow chow",
            "cockapoo",
            "cocker spaniel",
            "collie",
            "corgi",
            "dachshund",
            "dalmatian",
            "doberman",
            "dobermann",
            "english setter",
            "english springer spaniel",
            "french bulldog",
            "frenchie",
            "german shepherd",
            "german shorthaired pointer",
            "giant schnauzer",
            "golden retriever",
            "goldendoodle",
            "great dane",
            "great pyrenees",
            "greyhound",
            "havanese",
            "husky",
            "irish setter",
            "irish wolfhound",
            "italian greyhound",
            "jack russell",
            "jack russell terrier",
            "japanese chin",
            "keeshond",
            "kelpie",
            "labradoodle",
            "labrador",
            "labrador retriever",
            "lhasa apso",
            "lurcher",
            "maltese",
            "maltipoo",
            "mastiff",
            "miniature pinscher",
            "miniature schnauzer",
            "newfoundland",
            "norfolk terrier",
            "norwich terrier",
            "old english sheepdog",
            "papillon",
            "pekingese",
            "pembroke welsh corgi",
            "pit bull",
            "pitbull",
            "pomeranian",
            "poodle",
            "portuguese water dog",
            "pug",
            "puggle",
            "rhodesian ridgeback",
            "rottweiler",
            "saint bernard",
            "st bernard",
            "saluki",
            "samoyed",
            "schipperke",
            "schnauzer",
            "scottish terrier",
            "shar pei",
            "sheltie",
            "shetland sheepdog",
            "shiba inu",
            "shih tzu",
            "siberian husky",
            "springer spaniel",
            "staffordshire bull terrier",
            "staffie",
            "staffy",
            "standard poodle",
            "toy poodle",
            "vizsla",
            "weimaraner",
            "welsh corgi",
            "west highland white terrier",
            "westie",
            "whippet",
            "wire fox terrier",
            "yorkie",
            "yorkshire terrier",
        ],
    ),
    (
        "cat",
        &[
            "balinese",
            "bengal",
            "birman",
            "british blue",
            "british shorthair",
            "burmese",
            "chartreux",
            "cornish rex",
            "devon rex",
            "domestic longhair",
            "domestic medium hair",
            "domestic mediumhair",
            "domestic shorthair",
            "egyptian mau",
            "exotic shorthair",
            "maine coon",
            "manx",
            "norwegian forest cat",
            "ocicat",
            "oriental shorthair",
            "persian",
            "ragdoll",
            "russian blue",
            "savannah cat",
            "scottish fold",
            "selkirk rex",
            "siamese",
            "siberian cat",
            "singapura",
            "somali cat",
            "sphynx",
            "tonkinese",
            "turkish angora",
            "turkish van",
        ],
    ),
];

/// Words of `text`, lower case, letters and digits only.
fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

/// Does `said` hold the breed's words in a row (the last one may be plural)?
fn says(said: &[String], breed: &str) -> bool {
    let want: Vec<&str> = breed.split(' ').collect();
    said.windows(want.len()).any(|w| {
        w.iter()
            .zip(&want)
            .enumerate()
            .all(|(i, (s, b))| s == b || (i + 1 == want.len() && s.strip_suffix('s') == Some(b)))
    })
}

/// The species ("dog" or "cat") of every breed named in `text`, each once.
pub fn species_named_by_breed(text: &str) -> Vec<&'static str> {
    let said = words(text);
    BREEDS
        .iter()
        .filter(|(_, breeds)| breeds.iter().any(|b| says(&said, b)))
        .map(|(species, _)| *species)
        .collect()
}

/// The species of a value that is itself a breed ("Yorkshire terrier"), when
/// it is nothing more than that breed.
pub fn breed_species(value: &str) -> Option<&'static str> {
    let said = words(value);
    BREEDS.iter().find_map(|(species, breeds)| {
        breeds.iter().any(|b| b.split(' ').count() == said.len() && says(&said, b)).then_some(*species)
    })
}

/// Species from a breed said alone, in a drafted note's Patient section
/// (`patient.species`, `patient.breed`): when Species is empty or holds a
/// breed, and that breed (or Breed's) names exactly one of dog or cat in
/// both the value and its cited lines, Species becomes "Dog" or "Cat" with
/// the breed's citations, and a breed found in Species moves to an empty
/// Breed. Drafts without these fields, or with a species already written,
/// are left as they are.
pub fn species_from_breed(note: &mut Value, transcript: &Transcript) {
    let Some(patient) = note.get_mut("patient").and_then(Value::as_object_mut) else { return };
    if !patient.contains_key("species") {
        return;
    }
    let value_of =
        |v: Option<&Value>| v.and_then(|f| f.get("value")).and_then(Value::as_str).map(str::to_string);
    let species = value_of(patient.get("species"));
    let in_species = species.as_deref().is_some_and(|s| breed_species(s).is_some());
    let source = if in_species {
        "species"
    } else if species.is_none() && value_of(patient.get("breed")).is_some() {
        "breed"
    } else {
        return;
    };
    let field = patient[source].clone();
    let breed = value_of(Some(&field)).unwrap_or_default();
    let evidence: Vec<u32> = field["evidence"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_u64).map(|v| v as u32).collect())
        .unwrap_or_default();
    let cited: String = evidence
        .iter()
        .filter_map(|id| transcript.get(*id))
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let [kind] = species_named_by_breed(&breed)[..] else { return };
    if !species_named_by_breed(&cited).contains(&kind) {
        return;
    }
    let word = if kind == "dog" { "Dog" } else { "Cat" };
    patient.insert("species".into(), json!({ "value": word, "unit": null, "evidence": field["evidence"] }));
    if in_species && patient.get("breed").is_some_and(Value::is_null) {
        patient.insert("breed".into(), field);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_breed_names_its_species_and_nothing_else_does() {
        assert_eq!(species_named_by_breed("Coco is a seven-month old female Yorkshire terrier"), vec!["dog"]);
        assert_eq!(species_named_by_breed("eight year old Shih Tzu in for his dental"), vec!["dog"]);
        assert_eq!(species_named_by_breed("a nine year old spayed female Siamese"), vec!["cat"]);
        assert_eq!(species_named_by_breed("two Labradors and a Maine Coon"), vec!["dog", "cat"]);
        // Other animals' breeds, unknown breeds and ordinary words imply nothing.
        for line in [
            "an eight year old Arabian mare",
            "a Holstein cow with mastitis",
            "a Himalayan, here for a nail trim",
            "an Abyssinian with a skin problem",
            "a Rex with sore hocks",
            "a twelve year old domestic shored eye",
            "Pepper is here for her recheck",
            "a Shetland with laminitis",
        ] {
            assert!(species_named_by_breed(line).is_empty(), "{line}");
        }
        // Whole words only.
        assert!(species_named_by_breed("the pugnacious one").is_empty());
    }

    fn line(text: &str) -> Transcript {
        Transcript {
            segments: vec![crate::transcript::Segment {
                id: 0,
                start: 0.0,
                end: 1.0,
                speaker: Some(0),
                text: text.into(),
                hints: Vec::new(),
                check: None,
            }],
        }
    }

    fn patient(species: Value, breed: Value) -> Value {
        json!({ "patient": { "name": null, "species": species, "breed": breed } })
    }

    fn said(value: &str) -> Value {
        json!({ "value": value, "unit": null, "evidence": [0] })
    }

    /// The dental e2e draft had "Species: Yorkshire terrier" (and Breed the
    /// same) for "a seven-month old female Yorkshire terrier".
    #[test]
    fn a_breed_said_alone_gives_dog_or_cat_and_nothing_else_is_filled() {
        let coco = line("Coco is a seven-month old female Yorkshire terrier in for her spay.");
        let mut n = patient(said("Yorkshire terrier"), said("Yorkshire terrier"));
        species_from_breed(&mut n, &coco);
        assert_eq!(n, patient(said("Dog"), said("Yorkshire terrier")));
        // A breed in Species moves to an empty Breed.
        let mut n = patient(said("Yorkshire terrier"), Value::Null);
        species_from_breed(&mut n, &coco);
        assert_eq!(n, patient(said("Dog"), said("Yorkshire terrier")));
        // Species empty, Breed said: Species from the breed.
        let mochi = line("This is Mochi, a nine year old spayed female Siamese.");
        let mut n = patient(Value::Null, said("Siamese"));
        species_from_breed(&mut n, &mochi);
        assert_eq!(n, patient(said("Cat"), said("Siamese")));
        // A species already written is kept, even a wrong one (the checker flags it).
        let mut n = patient(said("Cat"), said("Yorkshire terrier"));
        species_from_breed(&mut n, &coco);
        assert_eq!(n, patient(said("Cat"), said("Yorkshire terrier")));
        // Unknown or other animals' breeds, and a breed not in its cited line, fill nothing.
        for (text, breed) in [
            ("Bella, an eight year old Arabian mare.", "Arabian"),
            ("A twelve year old domestic shored eye.", "domestic shored eye"),
            ("Pepper is here for her recheck.", "Yorkshire terrier"),
        ] {
            let mut n = patient(Value::Null, said(breed));
            species_from_breed(&mut n, &line(text));
            assert_eq!(n, patient(Value::Null, said(breed)), "{text}");
        }
        // No Patient section, or one without Species: unchanged.
        let mut n = json!({ "indication": [] });
        species_from_breed(&mut n, &coco);
        assert_eq!(n, json!({ "indication": [] }));
        // The filled species passes the faithfulness check.
        let mut n = patient(said("Yorkshire terrier"), Value::Null);
        species_from_breed(&mut n, &coco);
        let r = crate::faithfulness::check(&n, &coco);
        assert!(r.unverified_values.is_empty(), "{:?}", r.unverified_values);
    }

    #[test]
    fn a_value_that_is_a_breed_is_known() {
        assert_eq!(breed_species("Yorkshire terrier"), Some("dog"));
        assert_eq!(breed_species("Siamese"), Some("cat"));
        assert_eq!(breed_species("dog"), None);
        assert_eq!(breed_species("Yorkshire terrier cross"), None);
        assert_eq!(breed_species("Thoroughbred"), None);
    }
}
