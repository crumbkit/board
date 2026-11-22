# Unstable interfaces

released.json's `unstable` list names type ids and codes that a release ships
without freezing them, with a note saying until when. The test skips a
schema file, a report or a code that the list names, and the file does not
list them elsewhere. Each type the list names must be one of the external
plugin protocol's, and each code must be one Board has.

In 0.1.0 the list holds the external plugin protocol, "unstable until
0.2.0": `board/plugin-manifest/v1`, `board/call/v1`,
`board/call-result/v1`, `board/error/v1` and the `board::plugin::` codes.
Three external plugins test the protocol first, and 0.2.0 freezes it by
emptying the list. An external plugin written against 0.1.0 may need
changes in 0.2.0. Built-in plugins' types, capabilities and codes
are frozen from 0.1.0.
