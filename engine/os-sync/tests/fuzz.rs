//! The parsers fuzzed (ADR-0017 section 5: "The OBS1 parser is fuzzed before
//! release"; M3 plan phase 2 acceptance). A deterministic mutation fuzzer
//! that runs with `cargo test` on stable Rust: random inputs, and valid
//! objects with bytes flipped, inserted, cut and spliced. Nothing may panic;
//! a changed byte must never open; the guard must refuse what doesn't parse.
//!
//! `OPENBARK_FUZZ_ITERS` raises the number of cases (default 20 000).

use os_sync::keys::{self, DeviceKeys, SymKey};
use os_sync::{guard, seal};

/// xorshift64*: deterministic, so a failure reproduces.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
    /// Up to `max` random bytes.
    fn some(&mut self, max: usize) -> Vec<u8> {
        let n = self.below(max);
        self.bytes(n)
    }
}

fn iters() -> usize {
    std::env::var("OPENBARK_FUZZ_ITERS").ok().and_then(|v| v.parse().ok()).unwrap_or(20_000)
}

fn mutate(rng: &mut Rng, input: &[u8]) -> Vec<u8> {
    let mut b = input.to_vec();
    for _ in 0..=rng.below(4) {
        match rng.below(6) {
            0 if !b.is_empty() => {
                let i = rng.below(b.len());
                b[i] ^= 1 << rng.below(8);
            }
            1 if !b.is_empty() => {
                let i = rng.below(b.len());
                b[i] = rng.next() as u8;
            }
            2 => {
                let i = rng.below(b.len() + 1);
                let extra = rng.some(16);
                b.splice(i..i, extra);
            }
            3 if !b.is_empty() => {
                let i = rng.below(b.len());
                let left = b.len() - i;
                let n = rng.below(left) + 1;
                b.drain(i..i + n);
            }
            4 => {
                let n = rng.below(b.len() + 1);
                b.truncate(n)
            }
            _ => {
                // The header length field, a favourite of format bugs.
                if b.len() > 7 {
                    let v = rng.next() as u16;
                    b[5..7].copy_from_slice(&v.to_be_bytes());
                }
            }
        }
    }
    b
}

fn sample_obs1(dev: &DeviceKeys, dek: &SymKey) -> Vec<u8> {
    let h = seal::Header {
        v: 1,
        id: uuid::Uuid::new_v4().to_string(),
        space: uuid::Uuid::new_v4().to_string(),
        kind: "visit".into(),
        ver: 3,
        dev: uuid::Uuid::now_v7().to_string(),
        ctr: 9,
    };
    seal::seal_obs1(&h, br#"{"hlc":"0000000000001-0000-a","data":{}}"#, dek, dev).unwrap()
}

#[test]
fn obs1_parser_never_panics_and_mutations_never_open() {
    let mut rng = Rng(0x0BAD_5EED_2026_1020);
    let dev = DeviceKeys::generate().unwrap();
    let dek = SymKey::random().unwrap();
    let good = sample_obs1(&dev, &dek);
    let mut parsed = 0;
    for i in 0..iters() {
        let input = if i % 5 == 0 { rng.some(9000) } else { mutate(&mut rng, &good) };
        if let Ok(obj) = seal::parse_obs1(&input) {
            parsed += 1;
            // Anything changed must fail the signature or the cipher.
            if input != good {
                let opens = seal::verify_obs1(&obj, &dev.ed_pub()) && seal::open_obs1(&obj, &dek).is_ok();
                assert!(!opens, "a changed object opened (case {i})");
            }
        }
        let _ = guard::check_request(&serde_json::json!({ "body": keys::b64(&input) }));
    }
    assert!(parsed > 0, "some mutations should still parse structurally");
}

#[test]
fn obb1_parser_never_panics_and_mutations_never_open() {
    let mut rng = Rng(0xFEED_FACE_0BB1);
    let dev = DeviceKeys::generate().unwrap();
    let dek = SymKey::random().unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let blob = seal::seal_blob(&id, "space", 1, b"transcript text", &dek, &dev, "dev", &mut || 1).unwrap();
    let good = &blob.parts[0];
    let ed = dev.ed_pub();
    let signer = |d: &str| (d == "dev").then_some(ed);
    for i in 0..iters() / 4 {
        let input = mutate(&mut rng, good);
        let _ = seal::parse_obb1(&input);
        if input != *good {
            let h = hex::encode(keys::sha256(&[&input]));
            assert!(
                seal::open_blob(&[input], &[h], &id, "space", 1, &dek, &signer).is_err(),
                "a changed blob part opened (case {i})"
            );
        }
    }
}

#[test]
fn envelopes_and_wrapped_keys_never_panic() {
    let mut rng = Rng(0xE5E1_0FE5);
    let dev = DeviceKeys::generate().unwrap();
    let amk = keys::random32().unwrap();
    let env = keys::seal_to(&dev.x_pub(), &amk, b"amk|1|d", None).unwrap();
    let k = SymKey::random().unwrap();
    let wrapped = keys::wrap(&k, b"dek", &amk).unwrap();
    for i in 0..iters() / 4 {
        let e = mutate(&mut rng, &env);
        if e != env {
            assert!(keys::open_envelope(dev.x_secret(), &e, b"amk|1|d", None).is_err(), "case {i}");
        }
        let w = mutate(&mut rng, &wrapped);
        if w != wrapped {
            assert!(keys::unwrap(&k, b"dek", &w).is_err(), "case {i}");
        }
        let words: String = rng.some(200).iter().map(|b| (b % 96 + 32) as char).collect();
        let _ = keys::Kit::from_words(&words);
        let _ = keys::PairingCode::parse(&words);
        let _ = os_sync::roster::Roster::open(&e, &k, "acct", 1);
    }
}
