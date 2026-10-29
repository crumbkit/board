//! The two children an approval needs: the signer, which shows the owner a
//! statement and signs it with Touch ID, and `ssh-keygen`, which checks a
//! signature against the allowed signers file
//! (docs/architecture/writes/signing.md).

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use board_model::signature_namespace;

use crate::process::{Finished, Limits, Scope, Stopped, run};

/// How long the owner has to read a statement and touch the sensor. With
/// the 3 s verify and the 5 s lock wait it fits the effects mod's 30 s
/// limit on `board answer`.
const SIGN_TIME: Duration = Duration::from_secs(20);

/// How long `ssh-keygen` has to check a signature, which takes
/// milliseconds.
const VERIFY_TIME: Duration = Duration::from_secs(3);

/// The exit status with which attention-sign says the owner asked for
/// changes instead of signing.
const DECLINED: i32 = 3;

/// The principal an approval is signed as, in the allowed signers file.
const PRINCIPAL: &str = "owner";

/// `ssh-keygen` reads a signature only from a file, and the statement comes
/// on stdin, so the signature reaches it on descriptor 3 through a here
/// document. It never sees a shell metacharacter: the shell expands `$3`
/// once and does not read the result again.
const VERIFY: &str =
    "exec /usr/bin/ssh-keygen -Y verify -f \"$1\" -I \"$2\" -n \"$3\" -s /dev/fd/3 3<<EOF\n$4\nEOF";

/// Why no good signature was made or found.
#[derive(Debug, thiserror::Error)]
pub enum SignError {
    #[error("{} could not be run: {source}", program.display())]
    NotStarted {
        program: OsString,
        source: std::io::Error,
    },
    #[error("{} did not finish within {}s", program.display(), time.as_secs())]
    Timeout { program: OsString, time: Duration },
    #[error("the owner did not sign and asked for changes: {feedback}")]
    Declined { feedback: String },
    #[error("{} exited with {status}: {stderr}", program.display())]
    Failed {
        program: OsString,
        status: String,
        stderr: String,
    },
    #[error("{} printed something other than an SSH signature", program.display())]
    NotASignature { program: OsString },
    #[error("no allowed signers file is at {}", file.display())]
    NoSigners { file: std::path::PathBuf },
    #[error("{} is not trusted: {why}", file.display())]
    Untrusted {
        file: std::path::PathBuf,
        why: &'static str,
    },
    #[error("the signature does not verify for {PRINCIPAL} in {}: {stderr}", file.display())]
    Bad {
        file: std::path::PathBuf,
        stderr: String,
    },
}

/// Runs `program approve <statement>`, the signer's command line, and
/// returns the armored signature it prints. `environment` is all the
/// signer sees. The program must be a regular file that this user or
/// root owns and that neither its group nor others may write.
pub fn sign(
    program: &OsString,
    statement: &Path,
    environment: &[(&str, OsString)],
) -> Result<String, SignError> {
    trusted(Path::new(program))?;
    let arguments = ["approve".to_owned(), statement.display().to_string()];
    let limits = Limits {
        time: SIGN_TIME,
        ..Limits::default()
    };
    let (_, finished) = run(
        Path::new(program),
        &arguments,
        Path::new("/"),
        environment,
        Vec::new(),
        limits,
        Scope::Group,
    );
    let finished = finished_or(program, SIGN_TIME, finished)?;
    let stdout = String::from_utf8_lossy(&finished.stdout).into_owned();
    match finished.code {
        Some(0) if stdout.starts_with("-----BEGIN SSH SIGNATURE-----") => Ok(stdout),
        Some(0) => Err(SignError::NotASignature {
            program: program.clone(),
        }),
        Some(DECLINED) => Err(SignError::Declined {
            feedback: stdout.trim().to_owned(),
        }),
        _ => Err(failed(program, &finished)),
    }
}

/// Checks that `signature` is the owner's signature of `statement`, by a
/// key the allowed signers file `allowed` lists for the principal `owner`,
/// in the namespace the statement's header names.
/// The file must be trusted as the signer is.
pub fn verify(allowed: &Path, statement: &str, signature: &str) -> Result<(), SignError> {
    if !allowed.is_file() {
        return Err(SignError::NoSigners {
            file: allowed.to_owned(),
        });
    }
    trusted(allowed)?;
    let program = OsString::from("/bin/sh");
    let arguments = [
        "-c".to_owned(),
        VERIFY.to_owned(),
        "sh".to_owned(),
        allowed.display().to_string(),
        PRINCIPAL.to_owned(),
        signature_namespace(statement).to_owned(),
        signature.trim().to_owned(),
    ];
    let limits = Limits {
        time: VERIFY_TIME,
        ..Limits::default()
    };
    let (_, finished) = run(
        Path::new(&program),
        &arguments,
        Path::new("/"),
        &[],
        statement.as_bytes().to_vec(),
        limits,
        Scope::Group,
    );
    let finished = finished_or(&program, VERIFY_TIME, finished)?;
    if finished.code == Some(0) {
        return Ok(());
    }
    Err(SignError::Bad {
        file: allowed.to_owned(),
        stderr: String::from_utf8_lossy(&finished.stderr).trim().to_owned(),
    })
}

/// Refuses `file` unless it is a regular file, after links, that this user
/// or root owns and that neither its group nor others may write, so another
/// account cannot change the signer or the keys an approval trusts. A
/// program that runs as this user can still change them, which only the
/// sandbox an agent runs in stops.
fn trusted(file: &Path) -> Result<(), SignError> {
    use std::os::unix::fs::MetadataExt as _;
    let untrusted = |why| SignError::Untrusted {
        file: file.to_owned(),
        why,
    };
    let metadata = file
        .metadata()
        .map_err(|_| untrusted("it cannot be read"))?;
    if !metadata.is_file() {
        return Err(untrusted("it is not a regular file"));
    }
    let owner = metadata.uid();
    if owner != rustix::process::getuid().as_raw() && owner != 0 {
        return Err(untrusted("another user owns it"));
    }
    if metadata.mode() & 0o022 != 0 {
        return Err(untrusted("its group or others may write it"));
    }
    Ok(())
}

fn finished_or(
    program: &OsString,
    time: Duration,
    finished: Result<Finished, Stopped>,
) -> Result<Finished, SignError> {
    finished.map_err(|stopped| match stopped {
        Stopped::Io(source) => SignError::NotStarted {
            program: program.clone(),
            source,
        },
        Stopped::Timeout(_) => SignError::Timeout {
            program: program.clone(),
            time,
        },
        Stopped::TooLarge(_) => SignError::NotASignature {
            program: program.clone(),
        },
    })
}

fn failed(program: &OsString, finished: &Finished) -> SignError {
    SignError::Failed {
        program: program.clone(),
        status: finished
            .code
            .map_or_else(|| "a signal".to_owned(), |code| code.to_string()),
        stderr: String::from_utf8_lossy(&finished.stderr).trim().to_owned(),
    }
}

#[cfg(test)]
mod tests;
