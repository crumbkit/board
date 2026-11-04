# Beyond the called board

This file holds detail for docs/plugins/built-in.md. It says what a built-in
plugin's `Host` offers beyond the board it was called in. Each method runs
the code path of the `board` command a process plugin would run, so the
checks, refusals and codes are the same.

- `call(target, input)` runs another capability as the plugin's own
  `board call` does: one call deeper, so `board::plugin::too_deep` stops
  the same nesting, under the `BOARD_PLUGIN` it would set, with only the
  capability's declared `env` among the variables, and the called board's
  root as `caller`. Board chooses, checks and validates it as any call,
  and a refusal comes back as `board call` prints it. A process plugin it
  starts leads its own process group, as one `board call` starts does, and
  counts in the call's `started`.
- `board(elsewhere)` is a `Host` on the board that `board --personal`
  (`Elsewhere::Personal`) or `board -C <directory>`
  (`Elsewhere::Directory`, read against the called board's root) opens,
  with the same refusals, as the same plugin at the same depth.
- `decide(op, path, shown)` answers as `board decide` does under
  `BOARD_PLUGIN`: the pending effect goes in the personal board, and an
  approval is checked against the owner's allowed signers file
  (docs/architecture/writes/effects.md). An `op` that is not
  `<plugin>.<capability>` is `board::cli::usage`.
- `sync(push, deadline)` syncs the host's board as `board sync` does, or
  `board sync --no-push` without `push`. A plugin syncs a notes clone on
  the host `board(Elsewhere::Directory(clone))` gives, as `board -C <clone>
  sync` does. It waits for the board lock no longer than `board sync`, and
  its fetches and pushes share what is left of `deadline`. git still
  running when it passes is stopped, and the sync is
  `board::sync::timeout`.
- `mount_update(path, deadline)` moves a mount as `board mount update`
  does, waiting for the lock no longer than `deadline` or that command;
  past it, the update is `board::put::locked`.
