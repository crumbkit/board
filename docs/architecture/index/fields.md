# Fields of a reference's target

This file holds the `refs --field` detail for [../index.md](../index.md).

`--field <pointer>`, repeated, reads the target of each reference shown,
within the limit, where refs resolved it, through links and mounts. A
reference that resolves to nothing gets nothing. Each other `from` gains
`values`: in the order asked, each JSON pointer with its `value`, or with
`absent: true` when the record has nothing there. `target_code` is the code
check gives the target, or get's code when it is not a record; a target
that is not a record has no `values`. As text, the line gains
` target=<code>`, or ` target=-` for a healthy record, before the values;
each value is ` <pointer>=<value>`, the pointer a JSON string and the value
compact JSON or `absent`, so a pointer or value holding ` target=` cannot be
mistaken for the code. The check snapshot decides where a target is and its
code. `refs` reads exactly the blob that the walk listed, or that a kept
row holds for a source the run did not walk
([decisions.md](../performance/warm/decisions.md)), and that the code was
decided for. A target that resolved to another blob, or whose file changed
after the check, has `refs::target_changed` and no `values`, and the run counts it as `target_changed` in the timing log; run
`refs` again. `get` instead reads a file that changed during its run as it
is now, with the blob of the bytes it read. A pointer that is not RFC
6901 refuses the run as `board::cli::usage`. `tasks.available` asks for `/status`
and `/$type` this way to tell which waits are over.
