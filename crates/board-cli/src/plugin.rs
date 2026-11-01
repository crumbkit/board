//! `board plugin list`, `find` and `enable`.

use board_model::{CapabilityName, PluginName};
use board_ops::Board;
use board_plugin::MetaFilter;
use clap::Subcommand;

use crate::config::Context;
use crate::render::{self, Printer};
use crate::report::Output;
use crate::run::{Page, plain};

#[derive(Debug, Subcommand)]
pub(crate) enum PluginCommand {
    /// List the installed plugins, their capabilities and whether this board enables them.
    #[command(after_help = "\
Examples:
  board plugin list
  board plugin list --long
  board plugin list --json")]
    List {
        #[command(flatten)]
        page: Page,
    },
    /// List the installed plugins' capabilities, or those that match every filter.
    #[command(after_help = "\
Examples:
  board plugin find
  board plugin find --capability available
  board plugin find --capability available --long
  board plugin find --meta category=tasks --json")]
    Find {
        /// Only capabilities of this name.
        #[arg(long)]
        capability: Option<CapabilityName>,
        /// `<key>=<value>`: the capability's metadata, or else its plugin's, holds this string.
        #[arg(long = "meta")]
        filters: Vec<MetaFilter>,
        #[command(flatten)]
        page: Page,
    },
    /// Enable an installed plugin: write the schemas it ships, then name it in .board/board.json.
    #[command(after_help = "\
Examples:
  board plugin enable tasks
  board plugin enable tasks --json")]
    Enable {
        /// The installed plugin's name, as `board plugin list` shows it.
        name: PluginName,
    },
}

/// `board plugin <command>`.
pub(crate) fn plugin(
    command: PluginCommand,
    board: &Board,
    context: &Context,
    printer: Printer,
) -> Output {
    match command {
        PluginCommand::Enable { name } => {
            let commands = crate::alias::commands();
            let commands: Vec<&str> = commands.iter().map(String::as_str).collect();
            let (plugins, cache) = (context.plugins.as_deref(), context.cache.as_deref());
            match board.plugin_enable(plugins, cache, &name, &commands) {
                Ok(report) => render::plugin_enable(printer, &report),
                Err(error) => render::refused(printer, plain(error.code(), &error)),
            }
        }
        PluginCommand::List { page } => match board.plugin_list(context.plugins.as_deref()) {
            Ok(report) => render::plugin_list(printer, &report, &page),
            Err(error) => render::refused(printer, plain(error.code(), &error)),
        },
        PluginCommand::Find {
            capability,
            filters,
            page,
        } => match board.plugin_find(context.plugins.as_deref(), capability.as_ref(), &filters) {
            Ok(report) => render::plugin_find(printer, &report, &page),
            Err(error) => render::refused(printer, plain(error.code(), &error)),
        },
    }
}
