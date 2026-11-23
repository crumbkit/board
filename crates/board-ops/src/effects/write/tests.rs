#![expect(
    clippy::disallowed_methods,
    reason = "the owner's tests sign and verify writes as the effects module does"
)]
//! Signing and verifying the owner's write of a plugin's record, each
//! refusal beside an admitted twin.

use board_scenario::signer::Signer;
use serde_json::json;

use super::{OwnerWriteError, sign_owner_write, verify_owner_write};
use crate::effects::Signing;

fn signing(signer: &Signer) -> Signing {
    Signing {
        signer: Some(signer.program().into_os_string()),
        cache: Some(signer.directory().join("cache")),
        allowed: Some(signer.allowed()),
        ..Signing::default()
    }
}

const PATH: &str = "/autosync/boards/team";

fn record() -> serde_json::Value {
    json!({"$type": "autosync/board/v1", "path": "~/team", "origins": ["git@example.com:team/board.git"]})
}

#[test]
fn a_write_the_owner_signs_verifies_and_the_same_signature_of_another_path_or_record_does_not() {
    let signer = Signer::new();
    let signing = signing(&signer);
    let signature = sign_owner_write(&signing, signer.directory(), PATH, &record()).unwrap();
    verify_owner_write(&signing, PATH, &record(), &signature).unwrap();
    let moved = verify_owner_write(&signing, "/autosync/boards/other", &record(), &signature);
    assert!(matches!(moved, Err(OwnerWriteError::Bad { .. })), "{moved:?}");
    let mut edited = record();
    edited["origins"] = json!(["git@example.com:someone/else.git"]);
    let edited = verify_owner_write(&signing, PATH, &edited, &signature);
    assert!(matches!(edited, Err(OwnerWriteError::Bad { .. })), "{edited:?}");
}

#[test]
fn a_write_signed_by_a_key_the_allowed_signers_file_does_not_list_is_refused() {
    let owner = Signer::new();
    let stranger = Signer::new();
    let theirs = sign_owner_write(&signing(&stranger), stranger.directory(), PATH, &record());
    let refused = verify_owner_write(&signing(&owner), PATH, &record(), &theirs.unwrap());
    assert!(
        matches!(refused, Err(OwnerWriteError::Bad { .. })),
        "{refused:?}"
    );
}

#[test]
fn with_no_signer_a_write_is_refused_and_with_one_it_is_signed() {
    let signer = Signer::new();
    let unsigned = Signing {
        signer: None,
        ..signing(&signer)
    };
    let refused = sign_owner_write(&unsigned, signer.directory(), PATH, &record());
    assert!(
        matches!(refused, Err(OwnerWriteError::NoSigner)),
        "{refused:?}"
    );
    assert!(sign_owner_write(&signing(&signer), signer.directory(), PATH, &record()).is_ok());
}

#[test]
fn with_no_allowed_signers_file_named_a_signature_is_refused_unread() {
    let signer = Signer::new();
    let signature = sign_owner_write(&signing(&signer), signer.directory(), PATH, &record());
    let unnamed = Signing {
        allowed: None,
        ..signing(&signer)
    };
    let refused = verify_owner_write(&unnamed, PATH, &record(), &signature.unwrap());
    assert!(
        matches!(refused, Err(OwnerWriteError::NoneNamed)),
        "{refused:?}"
    );
}
