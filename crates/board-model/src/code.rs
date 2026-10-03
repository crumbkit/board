//! The stable codes that name every refusal and every degraded entry.

use std::fmt;

/// A stable code, `board::<area>::<reason>`. A code never changes after
/// release; docs/architecture.md (Codes) lists the v0 set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Code {
    JsonParse,
    JsonMergeConflict,
    JsonDuplicateKey,
    JsonTooLarge,
    JsonNotUtf8,
    RecordBadType,
    RecordUnknownType,
    RecordInvalid,
    RecordNotARecord,
    ConfigNotFound,
    ConfigInvalid,
    SchemaInvalid,
    RefMalformed,
    RefWrongType,
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
    IndexFailed,
    QueryInvalid,
}

impl Code {
    /// Every code, so a spelling can be read back.
    pub const ALL: [Code; 29] = [
        Self::JsonParse,
        Self::JsonMergeConflict,
        Self::JsonDuplicateKey,
        Self::JsonTooLarge,
        Self::JsonNotUtf8,
        Self::RecordBadType,
        Self::RecordUnknownType,
        Self::RecordInvalid,
        Self::RecordNotARecord,
        Self::ConfigNotFound,
        Self::ConfigInvalid,
        Self::SchemaInvalid,
        Self::RefMalformed,
        Self::RefWrongType,
        Self::PathMalformed,
        Self::PathNotFound,
        Self::PathShadowed,
        Self::PathCaseCollision,
        Self::RefDangling,
        Self::MountInvalid,
        Self::MountRepoMissing,
        Self::MountCommitMissing,
        Self::MountRemoteUnsupported,
        Self::ResolveCycle,
        Self::ResolveTooManyHops,
        Self::RepoUnreadable,
        Self::RepoSubmodule,
        Self::IndexFailed,
        Self::QueryInvalid,
    ];

    /// The code spelled `text`.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|code| code.as_str() == text)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::JsonParse => "board::json::parse",
            Self::JsonMergeConflict => "board::json::merge_conflict",
            Self::JsonDuplicateKey => "board::json::duplicate_key",
            Self::JsonTooLarge => "board::json::too_large",
            Self::JsonNotUtf8 => "board::json::not_utf8",
            Self::RecordBadType => "board::record::bad_type",
            Self::RecordUnknownType => "board::record::unknown_type",
            Self::RecordInvalid => "board::record::invalid",
            Self::RecordNotARecord => "board::record::not_a_record",
            Self::ConfigNotFound => "board::config::not_found",
            Self::ConfigInvalid => "board::config::invalid",
            Self::SchemaInvalid => "board::schema::invalid",
            Self::RefMalformed => "board::ref::malformed",
            Self::RefWrongType => "board::ref::wrong_type",
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
            Self::IndexFailed => "board::index::failed",
            Self::QueryInvalid => "board::query::invalid",
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

    /// Fails to compile when a variant is added, so `ALL` stays complete.
    fn listed(code: Code) {
        match code {
            Code::JsonParse
            | Code::JsonMergeConflict
            | Code::JsonDuplicateKey
            | Code::JsonTooLarge
            | Code::JsonNotUtf8
            | Code::RecordBadType
            | Code::RecordUnknownType
            | Code::RecordInvalid
            | Code::RecordNotARecord
            | Code::ConfigNotFound
            | Code::ConfigInvalid
            | Code::SchemaInvalid
            | Code::RefMalformed
            | Code::RefWrongType
            | Code::PathMalformed
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
            | Code::RepoSubmodule
            | Code::IndexFailed
            | Code::QueryInvalid => {}
        }
    }

    #[test]
    fn every_code_is_board_area_reason_in_snake_case() {
        for code in Code::ALL {
            listed(code);
            let parts: Vec<&str> = code.as_str().split("::").collect();
            assert_eq!(parts.len(), 3, "{code}");
            assert_eq!(parts[0], "board", "{code}");
            for part in &parts[1..] {
                assert!(!part.is_empty(), "{code}");
                assert!(
                    part.bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
                    "{code}"
                );
            }
        }
    }

    #[test]
    fn no_two_codes_share_a_spelling() {
        let spellings: HashSet<&str> = Code::ALL.iter().map(|code| code.as_str()).collect();
        assert_eq!(spellings.len(), Code::ALL.len());
    }

    #[test]
    fn every_code_parses_back_from_its_spelling() {
        for code in Code::ALL {
            assert_eq!(Code::parse(code.as_str()), Some(code));
        }
        assert_eq!(Code::parse("board::json::parse "), None);
    }

    #[test]
    fn a_code_displays_as_its_spelling() {
        assert_eq!(
            Code::ResolveTooManyHops.to_string(),
            "board::resolve::too_many_hops"
        );
    }
}
