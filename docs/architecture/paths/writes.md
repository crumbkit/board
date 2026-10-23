# Writes to a path

This file holds detail for docs/architecture/paths.md. It says which paths a
write refuses.

`board put` resolves the path, following pointers, so a write through a link
lands at its target; put does not create a missing target, and refuses with its
resolution code. A path through a mount writes as architecture/writes.md says. A
segment git keeps, `.git` under any spelling HFS+ or NTFS opens as it, is
`board::path::malformed`, in `mount update` too. Missing directories are
created. A write that would leave its own path degraded is refused with the
code the walk would give it, such as `path::shadowed`. When that code comes
from a directory above, such as one whose name collides in case, the error
names the directory, says whether the write would create it, and names the
existing entry it collides with.

A put never writes through or over a symlink (architecture/paths/symlinks.md).
An ignored path is written like any other: put writes into an ignored
directory and commits the file. An ignored file where a directory must go
cannot be read as one, so the put is refused with `board::repo::unreadable`
before anything is written.
