//! The plugins built into the `board` binary, each behind a Cargo feature
//! of its name (docs/plugins/built-in.md).

use board_ops::BuiltIn;

/// Every plugin this build of Board has built in.
pub const BUILT_IN: BuiltIn = BuiltIn(&[
    #[cfg(feature = "decisions")]
    &board_plugin_decisions::Decisions,
    #[cfg(feature = "evals")]
    &board_plugin_evals::Evals,
    #[cfg(feature = "plans")]
    &board_plugin_plans::Plans,
    #[cfg(feature = "requests")]
    &board_plugin_requests::Requests,
]);
