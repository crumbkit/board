//! Plan test: Board's copy of Flare's case file is the file at the Flare
//! commit `cases/pin.json` names, byte for byte, so the two parsers cannot
//! drift apart without a test failing.
#![cfg(test)]

use std::fmt::Write as _;

use serde::Deserialize;
use sha2::{Digest, Sha256};

const CASES: &[u8] = include_bytes!("../cases/search-cases.json");
const PIN: &str = include_str!("../cases/pin.json");

#[derive(Debug, Deserialize)]
struct Pin {
    repository: String,
    commit: String,
    path: String,
    blob: String,
    sha256: String,
}

fn pin() -> Pin {
    serde_json::from_str(PIN).unwrap()
}

/// The lower-case hex SHA-256 of `bytes`.
fn sha256(bytes: &[u8]) -> String {
    let mut text = String::new();
    for byte in Sha256::digest(bytes) {
        write!(text, "{byte:02x}").unwrap();
    }
    text
}

/// Whether `bytes` are the file the pin names.
fn pinned(bytes: &[u8]) -> bool {
    sha256(bytes) == pin().sha256
}

#[test]
fn the_pin_names_a_full_flare_commit_and_the_case_file_in_it() {
    let pin = pin();
    assert_eq!(pin.repository, "crumbkit/flare");
    assert_eq!(pin.path, "crates/flare-query/cases/search-cases.json");
    for (name, id, length) in [("commit", &pin.commit, 40), ("blob", &pin.blob, 40)] {
        assert_eq!(id.len(), length, "{name}");
        assert!(id.bytes().all(|byte| byte.is_ascii_hexdigit()), "{name}");
    }
}

#[test]
fn the_copy_is_the_pinned_commit_s_case_file() {
    assert!(
        pinned(CASES),
        "cases/search-cases.json is not the file at {} of {}; copy {} from that commit",
        pin().commit,
        pin().repository,
        pin().path
    );
}

#[test]
fn a_copy_that_differs_by_one_byte_fails_the_pin() {
    for at in [0, CASES.len() / 2, CASES.len() - 1] {
        let mut changed = CASES.to_vec();
        changed[at] ^= 1;
        assert!(!pinned(&changed), "byte {at}");
    }
    let mut longer = CASES.to_vec();
    longer.push(b'\n');
    assert!(!pinned(&longer));
}
