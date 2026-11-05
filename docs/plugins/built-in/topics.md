# Topics

This file holds detail for docs/plugins/built-in.md. The `topics` plugin
ships one record type and no capability.

A topic names a concept, tool, model or design that several records
concern, such as a library several evals test or a sandbox several suites
compare. Records name it by its path instead of each repeating its name in
their own text.

## Records

- `topics/topic/v1` at `/topics/<slug>`, where the slug is the name in
  lowercase letters and digits joined by `-`.
  - `title`: the name as people write it, such as `Raycast Workspaces`.
  - `description`: what it is, in words. Optional.
  - `aliases`: other ways people write the name, such as `sandbox-exec`
    for `seatbelt`. Optional.
  - `urls`, `repos` and `refs`: optional links to pages, to repositories
    as `owner/name`, and to other board records.

## Who reads topics

An eval's or a suite's `subjects` may name a topic by its path, such as
`/topics/seatbelt`, since a subject is a repository or a board path.
`board call evals.about --subject <name>` lists the evals that name a topic
whose title or alias is `<name>`, ignoring ASCII case, beside those that
name `<name>` itself. A subject names a topic in the eval's own board, so a
mounted board's evals match only that board's topics.

## Why a plugin of its own

A type's namespace is its plugin's name, and topics serve evals, suites and
any other record that names what it concerns. So the type lives in a plugin
of its own rather than in `evals`. The plugin has no capability because
nothing needs one: a topic is written with `board put /topics/<slug>`, and
`evals.about` reads topics through Board's index.
