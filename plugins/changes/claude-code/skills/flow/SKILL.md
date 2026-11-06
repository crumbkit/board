---
name: flow
description: 'Turn a request such as "/flow plan this and stop" into one changes.run call on the current repository, and follow the brief it returns. Use it when the person types /flow or asks to run a change through its flow up to a phase.'
---

# flow

Make one `changes.run` call with `--inline true`, then follow its brief.

1. Find the mount: `board ls /repos` names the current repository's.
2. Read the request:
   - "plan this and stop": `--through plan`.
   - "up to review", "until review": `--until review`.
   - "small: <title>": `--flow small`, with the flow's own stop.
   - No phase named: leave the stop out; the flow's or the change's holds.
3. A new piece of work starts a change:
   `board call changes.run --mount /repos/<name> --title '<title>' [--flow <name>] [--through <phase> | --until <phase>] --inline true`.
   Resume one with `--path /changes/<name>` in place of `--mount` and
   `--title`.
4. Follow the returned brief: work through `steps`, run `changes.next` at
   each phase, and stop where `stopping` says. When `stop.reached` is true,
   record `changes.stop --reason reached` and report to the person.
