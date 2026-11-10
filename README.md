# Board

A fast, validated view of a git tree. Start with `DOCS.md`.

`PLAN.md` says what is being built now.

## Installing a release

A release is a tag `v<version>`. It attaches `board-<version>-<target>.tar.gz`
for aarch64-apple-darwin, aarch64-unknown-linux-gnu and x86_64-unknown-linux-gnu,
each holding the `board` binary, and `SHA256SUMS`. Check the sum, extract
`board` onto `PATH`, and confirm `board --version` prints the version and,
after a `+`, the commit it was built from. A build outside the checkout, such
as in a copy with its own git repository or from a source tarball, sets
`BOARD_BUILD_COMMIT` at build time to the commit the source was taken from,
and the version names it. Setting it asserts that the source is that commit:
the local git still decides whether `.dirty.` follows, and with no local git
the build reads as clean and shares index decisions with every build that
names the same commit. Every plugin Board ships is built into the binary.

Then run `board skills install`, which writes the skills built into `board`
into `~/.claude/skills`. For an agent that reads `~/.agents/skills`, run
`board skills install --dir ~/.agents/skills` too. Run them again after each
upgrade, so the skills match the binary.

## Reading Board's output from Rust

A program that reads `board --json` output or writes Board records can use
the types in [crates/board-types](crates/board-types/README.md), which Board
prints through.

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this project by you, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.
