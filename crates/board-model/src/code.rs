//! The stable codes that name every refusal and every degraded entry.

use std::fmt;

mod hint;

/// A code, `board::<area>::<reason>`, which changes as STANDARDS.md 3 and 4
/// say. docs/architecture/errors.md lists the codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Code {
    JsonParse,
    JsonMergeConflict,
    JsonDuplicateKey,
    JsonTooLarge,
    JsonNotUtf8,
    JsonLossyNumber,
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
    MountFollowMissing,
    MountNotAncestor,
    MountStale,
    ResolveCycle,
    ResolveTooManyHops,
    RepoUnreadable,
    RepoSubmodule,
    IndexFailed,
    QueryInvalid,
    QueryTimeout,
    OutputOverBudget,
    PutStale,
    PutLocked,
    PutThroughMount,
    PutSymlink,
    PutInvalid,
    PutNoIdentity,
    PutHeadMoved,
    PutFailed,
    RefsTargetChanged,
    PluginNotInstalled,
    PluginBadManifest,
    PluginNotEnabled,
    PluginSchemaMissing,
    PluginSchemaDiffers,
    PluginAmbiguous,
    PluginBadInput,
    PluginBadOutput,
    PluginFailed,
    PluginNoDatabase,
    PluginNotStarted,
    PluginTimeout,
    PluginTooLarge,
    PluginTooDeep,
    CliUsage,
}

impl Code {
    /// Every code, so a spelling can be read back.
    pub const ALL: [Code; 59] = [
        Self::JsonParse,
        Self::JsonMergeConflict,
        Self::JsonDuplicateKey,
        Self::JsonTooLarge,
        Self::JsonNotUtf8,
        Self::JsonLossyNumber,
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
        Self::MountFollowMissing,
        Self::MountNotAncestor,
        Self::MountStale,
        Self::ResolveCycle,
        Self::ResolveTooManyHops,
        Self::RepoUnreadable,
        Self::RepoSubmodule,
        Self::IndexFailed,
        Self::QueryInvalid,
        Self::QueryTimeout,
        Self::OutputOverBudget,
        Self::PutStale,
        Self::PutLocked,
        Self::PutThroughMount,
        Self::PutSymlink,
        Self::PutInvalid,
        Self::PutNoIdentity,
        Self::PutHeadMoved,
        Self::PutFailed,
        Self::RefsTargetChanged,
        Self::PluginNotInstalled,
        Self::PluginBadManifest,
        Self::PluginNotEnabled,
        Self::PluginSchemaMissing,
        Self::PluginSchemaDiffers,
        Self::PluginAmbiguous,
        Self::PluginBadInput,
        Self::PluginBadOutput,
        Self::PluginFailed,
        Self::PluginNoDatabase,
        Self::PluginNotStarted,
        Self::PluginTimeout,
        Self::PluginTooLarge,
        Self::PluginTooDeep,
        Self::CliUsage,
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
            Self::JsonLossyNumber => "board::json::lossy_number",
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
            Self::MountFollowMissing => "board::mount::follow_missing",
            Self::MountNotAncestor => "board::mount::not_ancestor",
            Self::MountStale => "board::mount::stale",
            Self::ResolveCycle => "board::resolve::cycle",
            Self::ResolveTooManyHops => "board::resolve::too_many_hops",
            Self::RepoUnreadable => "board::repo::unreadable",
            Self::RepoSubmodule => "board::repo::submodule",
            Self::IndexFailed => "board::index::failed",
            Self::QueryInvalid => "board::query::invalid",
            Self::QueryTimeout => "board::query::timeout",
            Self::OutputOverBudget => "board::output::over_budget",
            Self::PutStale => "board::put::stale",
            Self::PutLocked => "board::put::locked",
            Self::PutThroughMount => "board::put::through_mount",
            Self::PutSymlink => "board::put::symlink",
            Self::PutInvalid => "board::put::invalid",
            Self::PutNoIdentity => "board::put::no_identity",
            Self::PutHeadMoved => "board::put::head_moved",
            Self::PutFailed => "board::put::failed",
            Self::RefsTargetChanged => "board::refs::target_changed",
            Self::PluginNotInstalled => "board::plugin::not_installed",
            Self::PluginBadManifest => "board::plugin::bad_manifest",
            Self::PluginNotEnabled => "board::plugin::not_enabled",
            Self::PluginSchemaMissing => "board::plugin::schema_missing",
            Self::PluginSchemaDiffers => "board::plugin::schema_differs",
            Self::PluginAmbiguous => "board::plugin::ambiguous",
            Self::PluginBadInput => "board::plugin::bad_input",
            Self::PluginBadOutput => "board::plugin::bad_output",
            Self::PluginFailed => "board::plugin::failed",
            Self::PluginNoDatabase => "board::plugin::no_database",
            Self::PluginNotStarted => "board::plugin::not_started",
            Self::PluginTimeout => "board::plugin::timeout",
            Self::PluginTooLarge => "board::plugin::too_large",
            Self::PluginTooDeep => "board::plugin::too_deep",
            Self::CliUsage => "board::cli::usage",
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
    use crate::{Fields, LogicalPath};

    /// Fails to compile when a variant is added, so `ALL` stays complete.
    fn listed(code: Code) {
        match code {
            Code::JsonParse
            | Code::JsonMergeConflict
            | Code::JsonDuplicateKey
            | Code::JsonTooLarge
            | Code::JsonNotUtf8
            | Code::JsonLossyNumber
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
            | Code::MountFollowMissing
            | Code::MountNotAncestor
            | Code::MountStale
            | Code::ResolveCycle
            | Code::ResolveTooManyHops
            | Code::RepoUnreadable
            | Code::RepoSubmodule
            | Code::IndexFailed
            | Code::QueryInvalid
            | Code::QueryTimeout
            | Code::OutputOverBudget
            | Code::PutStale
            | Code::PutLocked
            | Code::PutThroughMount
            | Code::PutSymlink
            | Code::PutInvalid
            | Code::PutNoIdentity
            | Code::PutHeadMoved
            | Code::PutFailed
            | Code::RefsTargetChanged
            | Code::PluginNotInstalled
            | Code::PluginBadManifest
            | Code::PluginNotEnabled
            | Code::PluginSchemaMissing
            | Code::PluginSchemaDiffers
            | Code::PluginAmbiguous
            | Code::PluginBadInput
            | Code::PluginBadOutput
            | Code::PluginFailed
            | Code::PluginNoDatabase
            | Code::PluginNotStarted
            | Code::PluginTimeout
            | Code::PluginTooLarge
            | Code::PluginTooDeep
            | Code::CliUsage => {}
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

    #[test]
    fn a_mount_hint_names_its_own_remedy_with_the_mount_path_and_without_one_the_generic_hint() {
        let mount: LogicalPath = "/vendor".parse().unwrap();
        let named = Fields::at(&mount);
        let cases = [
            (
                Code::MountNotAncestor,
                "to pin the mount to the mounted HEAD",
            ),
            (Code::MountStale, "reapply your change"),
        ];
        for (code, remedy) in cases {
            let hint = code.hint(&named);
            assert!(hint.contains("`board mount update /vendor`"), "{hint}");
            assert!(hint.contains(remedy), "{hint}");
            assert!(code.hint(&Fields::default()).contains("<path>"));
        }
        let other = Code::PutThroughMount.hint(&named);
        assert!(!other.contains("reapply your change"), "{other}");
    }
}
