//! A signer for tests: a software key that `ssh-keygen` makes, the allowed
//! signers file that lists it, and a program with attention-sign's command
//! line that signs with it in the namespace the statement's header names, so tests check real signatures without Touch ID.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{Item, Scenario};

/// A key, its allowed signers file and its signer program, in a scenario's
/// temporary directory.
#[derive(Debug)]
pub struct Signer {
    scenario: Scenario,
}

impl Signer {
    pub fn new() -> Self {
        let scenario = Scenario::new();
        let key = scenario.workdir().join("key");
        let key_text = key.to_str().unwrap();
        let keygen = ["-q", "-t", "ecdsa", "-b", "256", "-N", "", "-C", "test"];
        run(
            Path::new("/usr/bin/ssh-keygen"),
            &[&keygen[..], &["-f", key_text]].concat(),
            "",
        );
        let public = std::fs::read_to_string(key.with_extension("pub")).unwrap();
        let script = format!(
            "#!/bin/sh\n[ \"$1\" = approve ] || exit 1\nstatement=$(mktemp)\ncat \"$2\" > \"$statement\"\nIFS= read -r namespace < \"$statement\"\ncase \"$namespace\" in board/approve/v1|board/write/v1) ;; *) exit 1 ;; esac\nexec /usr/bin/ssh-keygen -Y sign -q -f '{key_text}' -n \"$namespace\" < \"$statement\"\n"
        );
        scenario
            .write("allowed_signers", Item::File(&format!("owner {public}")))
            .write("sign", Item::Executable(&script));
        Self { scenario }
    }

    /// The program that signs as attention-sign does: `<program> approve <file>`.
    pub fn program(&self) -> PathBuf {
        self.directory().join("sign")
    }

    /// The allowed signers file that lists this key as `owner`.
    pub fn allowed(&self) -> PathBuf {
        self.directory().join("allowed_signers")
    }

    /// The armored signature of `statement`.
    pub fn sign(&self, statement: &str) -> String {
        run(&self.program(), &["approve", "/dev/stdin"], statement)
    }

    /// The armored signature of `statement` in `namespace`, whatever its
    /// first line says, as a signer that ignored it would make.
    pub fn sign_in(&self, namespace: &str, statement: &str) -> String {
        let key = self.directory().join("key");
        let key = key.to_str().unwrap();
        let arguments = ["-Y", "sign", "-q", "-f", key, "-n", namespace];
        run(Path::new("/usr/bin/ssh-keygen"), &arguments, statement)
    }

    /// The directory that holds the key, for a cache or another program.
    pub fn directory(&self) -> &Path {
        self.scenario.workdir()
    }

    /// Writes `item` at `path` beside the key.
    pub fn write(&self, path: &str, item: Item<'_>) {
        self.scenario.write(path, item);
    }
}

impl Default for Signer {
    fn default() -> Self {
        Self::new()
    }
}

/// What `program` prints given `arguments` and `stdin`, which must succeed.
#[expect(
    clippy::disallowed_methods,
    reason = "the test signer runs ssh-keygen, which no production owner of child processes offers to a test fixture"
)]
fn run(program: &Path, arguments: &[&str], stdin: &str) -> String {
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{} failed", program.display());
    String::from_utf8(output.stdout).unwrap()
}
