# Board

A fast, validated view of a git tree. Start with `DOCS.md`.

`PLAN.md` says what is being built now.

## Installing a release

A release is a tag `v<version>`. It attaches `board-<version>-<target>.tar.gz`
for aarch64-apple-darwin, aarch64-unknown-linux-gnu and x86_64-unknown-linux-gnu,
each holding the `board` binary, `<plugin>-<version>.tar.gz` for each plugin,
and `SHA256SUMS`. Check the sum, extract `board` onto `PATH`, and confirm
`board --version` prints the version. `docs/plugins.md` says how a plugin
archive is installed.
