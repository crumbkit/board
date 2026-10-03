# Board

Board is a fast, validated view of a git tree. It indexes every file in a
board, validates JSON records against schemas stored in the tree, resolves
typed references through links and mounts, and gives agents an easy way to
read and write records. Plugins add higher-level constructs as separate
programs. DOCS.md is the map; PLAN.md says what is being built now.

## Gates

Run `bin/lint` and `bin/test` before every push. CI runs the same scripts on
macOS and Linux. STANDARDS.md lists what they enforce and the rules they
cannot.

## Rules tooling cannot enforce

1. **Stay small.** Add code, a guard, a crate or a concept only for a need that
   exists now. A new core concept needs two plugins that use it. Every pull
   request states its net change in production lines.
2. **The tree is the truth.** No cache, index or database may be needed for a
   correct answer; every query answers the same from a cold rebuild. Key
   derived data by blob or tree hash, never by commit or time.
3. **One resolver.** Only `board-tree` reads git objects or the working tree,
   and only it knows that links and mounts exist. Everything else takes
   resolved Locations.
4. **Validate in, report out.** `put` refuses an invalid record. `check` and the
   index never refuse a file; they mark it degraded with a stable code.
5. **Mounting brings data and schemas, never code.**
6. **Copy, don't invent.** Use the semantics of git, POSIX symlinks, JSON
   Schema and SQL. There is no query language beyond SQL.
7. **Pair refusals.** A test that something is refused has an otherwise
   identical twin that is admitted, and asserts the exact code.
8. **Docs say what exists.** docs/ describes the design and says what is built.
   PLAN.md holds what comes next; when a milestone lands, its settled design
   moves into docs/ and leaves PLAN.md.

## Writing

Use short sentences and plain words. Say each thing once. `lint.toml` sets word
budgets for this file with everything it imports, for DOCS.md and for docs/;
`bin/lint` fails when one is exceeded. Raise a budget only in a change that says
why.
