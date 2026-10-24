---
name: notes
description: 'Append a note to today''s daily note, or to a topic''s note, in a Reflect vault mounted at /vaults/<name>, and read a day''s daily note. Use it when the person asks you to note something down or asks what their daily note says. Start with `board -C <board> call notes.daily`, which syncs the vault and prints today''s note. SKILL.md and README.md in the plugin''s directory say more.'
---

# notes

Write down what the person asks you to note, in their own notes, and read
their daily note back.

## Steps

1. Run `board -C <board> call notes.daily`. It syncs the vault with its
   remote and prints today's daily note. Add `--vault <name>` when several
   vaults are mounted, and `--date YYYY-MM-DD` for another day. `text` is
   null when the note does not exist yet or is private. Never try to read a
   private note another way.
2. To note something, run `board -C <board> call notes.add --text '<text>'`.
   It appends the text under a `## HH:MM` heading to today's daily note.
   Add `--topic '<title>'` to append to that topic's note instead, which it
   starts with the title as its heading when there is none. Write the text
   as the person would: plain Markdown, no heading of your own.
3. Read the output:
   - `parked` is null when the text went into the note. Otherwise the note
     could not be written safely, for the reason the Board code names, and
     the text went to the `.agent.md` file beside it, at `path`. Tell the
     person, so they can move it into the note.
   - `unpushed` is null when the commit reached the vault's remote.
     Otherwise the text is committed in this machine's clone only, and the
     code says why the push failed; Reflect's desktop app or the next call
     pushes it. Do not call again to retry, because the text is already
     written. Tell the person the code.
4. Fix what it refuses:
   - `notes::vault::ambiguous`: several vaults are mounted. Ask which one,
     and pass `--vault`.
   - `notes::vault::not_found`: no vault is mounted under that name. The
     `vaults` field lists those that are.
   - `notes::vault::not_a_mount`: the record at `/vaults/<name>` mounts no
     other repository. Tell the person to point its `repo` at the vault's
     clone.
   - `notes::note::busy`: the note and its `.agent.md` file both hold edits
     Reflect has not committed. Nothing was written. Ask the person to let
     Reflect save, then call again.
   - `notes::note::not_text`: the note is not UTF-8 text. Nothing was
     written. Tell the person.
   - `notes::board::refused` carrying a `board::sync::` code, such as
     `board::sync::failed`: the first sync could not reach the remote, so
     nothing was written. Tell the person.
