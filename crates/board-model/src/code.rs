//! The stable codes that name every refusal and every degraded entry.

use std::fmt;

/// A stable code, `board::<area>::<reason>`. A code never changes after
/// release; docs/architecture.md (Codes) lists the v0 set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Code {
    PathMalformed,
    PathNotFound,
    PathShadowed,
    PathCaseCollision,
    RefDangling,
    MountInvalid,
    MountRepoMissing,
    MountCommitMissing,
    MountRemoteUnsupported,
    ResolveCycle,
    ResolveTooManyHops,
    RepoUnreadable,
    RepoSubmodule,
}

impl Code {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PathMalformed => "board::path::malformed",
            Self::PathNotFound => "board::path::not_found",
            Self::PathShadowed => "board::path::shadowed",
            Self::PathCaseCollision => "board::path::case_collision",
            Self::RefDangling => "board::ref::dangling",
            Self::MountInvalid => "board::mount::invalid",
            Self::MountRepoMissing => "board::mount::repo_missing",
            Self::MountCommitMissing => "board::mount::commit_missing",
            Self::MountRemoteUnsupported => "board::mount::remote_unsupported",
            Self::ResolveCycle => "board::resolve::cycle",
            Self::ResolveTooManyHops => "board::resolve::too_many_hops",
            Self::RepoUnreadable => "board::repo::unreadable",
            Self::RepoSubmodule => "board::repo::submodule",
        }
    }
}

impl fmt::Display for Code {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    const ALL: [Code; 13] = [
        Code::PathMalformed,
        Code::PathNotFound,
        Code::PathShadowed,
        Code::PathCaseCollision,
        Code::RefDangling,
        Code::MountInvalid,
        Code::MountRepoMissing,
        Code::MountCommitMissing,
        Code::MountRemoteUnsupported,
        Code::ResolveCycle,
        Code::ResolveTooManyHops,
        Code::RepoUnreadable,
        Code::RepoSubmodule,
    ];

    /// Fails to compile when a variant is added, so `ALL` stays complete.
    fn listed(code: Code) {
        match code {
            Code::PathMalformed
            | Code::PathNotFound
            | Code::PathShadowed
            | Code::PathCaseCollision
            | Code::RefDangling
            | Code::MountInvalid
            | Code::MountRepoMissing
            | Code::MountCommitMissing
            | Code::MountRemoteUnsupported
            | Code::ResolveCycle
            | Code::ResolveTooManyHops
            | Code::RepoUnreadable
            | Code::RepoSubmodule => {}
        }
    }

    #[test]
    fn every_code_is_board_area_reason_in_snake_case() {
        for code in ALL {
            listed(code);
            let parts: Vec<&str> = code.as_str().split("::").collect();
            assert_eq!(parts.len(), 3, "{code}");
            assert_eq!(parts[0], "board", "{code}");
            for part in &parts[1..] {
                assert!(!part.is_empty(), "{code}");
                assert!(
                    part.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
                    "{code}"
                );
            }
        }
    }

    #[test]
    fn no_two_codes_share_a_spelling() {
        let spellings: HashSet<&str> = ALL.iter().map(|code| code.as_str()).collect();
        assert_eq!(spellings.len(), ALL.len());
    }

    #[test]
    fn a_code_displays_as_its_spelling() {
        assert_eq!(
            Code::ResolveTooManyHops.to_string(),
            "board::resolve::too_many_hops"
        );
    }
}
