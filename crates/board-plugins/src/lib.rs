//! The plugins built into the `board` binary, each behind a Cargo feature
//! of its name (docs/plugins/built-in.md).

use board_ops::BuiltIn;

/// Every plugin this build of Board has built in.
pub const BUILT_IN: BuiltIn = BuiltIn(&[
    #[cfg(feature = "boundary")]
    &board_plugin_boundary::Boundary,
    #[cfg(feature = "controls")]
    &board_plugin_controls::Controls,
    #[cfg(feature = "evals")]
    &board_plugin_evals::Evals,
    #[cfg(feature = "github")]
    &board_plugin_github::Github,
    #[cfg(feature = "jira")]
    &board_plugin_jira::Jira,
    #[cfg(feature = "plans")]
    &board_plugin_plans::Plans,
    #[cfg(feature = "requests")]
    &board_plugin_requests::Requests,
    #[cfg(feature = "runs")]
    &board_plugin_runs::Runs,
]);
