# How a reference is coded

`follow` in crates/board-ops/src/check.rs decides each reference for
`check` and the index, and `unresolvable` in crates/board-ops/src/put.rs
decides them for a write when no view of the repositories could be made.
Each run decides every reference again, and stores nothing between runs.

- **ok**: the target resolves and has an accepted type.
- **dangling**: Board read where the target would be and found nothing
  (`ResolveError::NotFound` or `ResolveError::Dangling`), or found a fault
  it is sure of, such as a cycle. Code `board::ref::dangling`.
- **unreadable**: a mounted repository or commit could not be read
  (`ResolveError::Unreadable`, `RepoMissing`, `CommitMissing` or `Git`), so
  Board does not know whether the target exists. Code
  `board::ref::unreadable`; the detail's `at` names the path Board could
  not read.
- **wrong type** and **through mount**: as before.

| From | Event | To | Effect |
| --- | --- | --- | --- |
| any | the next run resolves the target | ok | the record is ok in the index |
| any | the next run finds nothing at the target | dangling | the record is degraded with `board::ref::dangling` |
| any | the next run cannot read the mount | unreadable | the record is degraded with `board::ref::unreadable`; a rule stays live |
| unreadable | `board mount update` repairs the mount, then a run | ok or dangling | as above |

A record with an unreadable reference and a dangling one, or any other
failure Board is sure of, is coded with the surer failure, and its
`failures` count includes both.

Crash points: a run that stops mid-check leaves the index as the last
complete refresh left it, and the next run decides each reference again.
`put` refuses before it writes, so a write refused as unreadable leaves the
tree unchanged and can be sent again once the mount reads.

Invariants: Board reports `board::ref::dangling` only when it has read the
place where the target would be. `put` and `check` give the same code for
the same reference. `rules.check` never treats an unreadable rule as
removed.
