use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::{Context, Trusted, owner_home, trust};

#[test]
fn an_approval_and_an_order_trust_the_signer_and_their_allowed_signers_files_in_the_owners_home() {
    assert_eq!(
        trust(Path::new("/Users/o")),
        Trusted {
            signer: OsString::from("/Users/o/.local/bin/attention-sign"),
            allowed: PathBuf::from("/Users/o/.config/board/allowed_signers"),
            attention_allowed: PathBuf::from("/Users/o/.config/attention/allowed_signers"),
        }
    );
}

#[test]
fn the_owners_home_comes_from_the_passwd_entry_as_an_absolute_path() {
    let home = owner_home().expect("the test runs as a user with a passwd entry");
    assert!(home.is_absolute(), "{}", home.display());
}

/// Every test that runs board runs a debug build, so this fails if a test
/// would run the owner's signer and open a Touch ID window on their Mac.
#[test]
fn a_debug_build_names_no_signer_so_no_test_runs_the_owners() {
    assert_eq!(
        Context::read().signer,
        None,
        "the tests must run a debug build of board, which runs no signer"
    );
}
