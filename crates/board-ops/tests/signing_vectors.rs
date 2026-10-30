//! The signing vectors in vectors/signing/v1 (docs/architecture/writes/signing.md):
//! Board builds the approval and owner-write statements byte for byte, signs
//! each in its namespace, and its verifier admits each good signature and
//! refuses each one made in another namespace or by another key.
#![cfg(test)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use board_model::{Effect, parse_json, signature_namespace};
use board_plugin::SignError;
use serde::Deserialize;

const VECTORS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../vectors/signing/v1");

/// The principal Board verifies every signature as.
const OWNER: &str = "owner";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    layouts: BTreeMap<String, Layout>,
    signatures: BTreeMap<String, Signature>,
    files: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Layout {
    namespace: String,
    owner: String,
    built_by: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Signature {
    statement: String,
    namespace: String,
    signer: String,
}

fn path(name: &str) -> PathBuf {
    Path::new(VECTORS).join(name)
}

fn read(name: &str) -> String {
    std::fs::read_to_string(path(name)).unwrap()
}

fn manifest() -> Manifest {
    serde_json::from_str(&read("manifest.json")).unwrap()
}

fn effect() -> Effect {
    Effect::from_value(&parse_json(read("approval.effect.json").as_bytes()).unwrap()).unwrap()
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "it builds each statement as the effects module does"
)]
fn board_builds_the_approval_and_owner_write_statements_byte_for_byte() {
    let manifest = manifest();
    assert_eq!(manifest.version, 1);
    let effect = effect();
    let write = Effect::owner_write(
        &effect.board,
        &effect.op,
        &effect.path,
        effect.record.as_ref(),
        effect.text.as_deref(),
    );
    let built = [
        ("approval.statement", effect.approval()),
        ("write.statement", write),
    ];
    for (name, statement) in built {
        assert_eq!(statement, read(name), "{name}");
        // v1 is frozen, so it names this repository by its earlier name.
        assert_eq!(manifest.layouts[name].owner, "viitta", "{name}");
        assert!(
            manifest.layouts[name]
                .built_by
                .starts_with("board_model::Effect::")
        );
    }
}

#[test]
fn each_board_statement_is_signed_in_the_namespace_the_manifest_names() {
    let manifest = manifest();
    for name in ["approval.statement", "write.statement"] {
        assert_eq!(
            signature_namespace(&read(name)),
            manifest.layouts[name].namespace,
            "{name}"
        );
    }
    assert_ne!(
        manifest.layouts["approval.statement"].namespace,
        manifest.layouts["write.statement"].namespace
    );
}

/// Whether Board's verifier admits the signature `name` of its statement.
#[expect(
    clippy::disallowed_methods,
    reason = "it verifies a vector as the effects module does"
)]
fn verified(name: &str, signature: &Signature) -> Result<(), SignError> {
    board_plugin::verify(
        &path("allowed_signers"),
        &read(&signature.statement),
        &read(name),
    )
}

#[test]
fn the_verifier_admits_the_owners_signature_of_a_board_statement_in_its_namespace() {
    for name in ["approval.sig", "write.sig"] {
        let signature = &manifest().signatures[name];
        assert_eq!(signature.signer, OWNER);
        assert_eq!(
            signature.namespace,
            signature_namespace(&read(&signature.statement))
        );
        verified(name, signature).unwrap_or_else(|error| panic!("{name}: {error}"));
    }
}

#[test]
fn the_verifier_refuses_a_signature_in_another_namespace_or_by_another_key() {
    let manifest = manifest();
    let refused = [
        "cross/approval-in-write.sig",
        "cross/write-in-approval.sig",
        "cross/approval-by-other.sig",
        "order.sig",
    ];
    for name in refused {
        let signature = &manifest.signatures[name];
        let statement = read(&signature.statement);
        assert!(
            signature.namespace != signature_namespace(&statement) || signature.signer != OWNER,
            "{name} would be a good signature"
        );
        let result = verified(name, signature);
        assert!(
            matches!(&result, Err(SignError::Bad { file, .. }) if *file == path("allowed_signers")),
            "{name}: {result:?}"
        );
    }
}

#[test]
fn the_manifest_lists_every_vector_file_with_its_sha256() {
    let manifest = manifest();
    let mut found = Vec::new();
    let mut pending = vec![PathBuf::from(VECTORS)];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let entry = entry.unwrap().path();
            if entry.is_dir() {
                pending.push(entry);
            } else {
                let name = entry.strip_prefix(VECTORS).unwrap().to_string_lossy();
                if name != "manifest.json" {
                    found.push(name.replace('\\', "/"));
                }
            }
        }
    }
    found.sort();
    assert_eq!(
        found,
        manifest.files.keys().cloned().collect::<Vec<_>>(),
        "the manifest lists other files than the directory holds"
    );
    for (name, sha256) in &manifest.files {
        let bytes = std::fs::read(path(name)).unwrap();
        assert_eq!(&board_model::Digest::of(&bytes).hex(), sha256, "{name}");
    }
}
