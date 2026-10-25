---
name: notes
description: 'Append a note to today''s daily note, or to a topic''s note, in a notes repo in Reflect''s layout, and read a day''s daily note. Use it when the person asks you to note something down or asks what their daily note says. Start with `board -C <board> call notes.daily`, run in the repository you work in, which syncs the notes repo that repository''s git remote routes to and prints today''s note. SKILL.md and README.md in the plugin''s directory say more.'
---

# notes

Write down what the person asks you to note, in their own notes, and read
their daily note back.

## Steps

1. Run the calls from the repository you are working in. Board passes the
   plugin that directory, and the plugin reads its git remote to pick the
   notes repo that covers that work.
2. Run `board -C <board> call notes.daily`. It syncs the notes repo with its
   remote and prints today's daily note. Add `--repo <path>` to name a notes
   repo, such as `/repos/notes-personal`, or `/` for the board's own. Add
   `--date YYYY-MM-DD` for another day. `text` is null when the note does not
   exist yet or is private. Never try to read a private note another way.
3. To note something, run `board -C <board> call notes.add --text '<text>'`.
   It appends the text under a `## HH:MM` heading to today's daily note. Add
   `--topic '<title>'` to append to that topic's note instead, which it starts
   with the title as its heading when there is none. Write the text as the
   person would: plain Markdown, no heading of your own.
4. Read the output:
   - `candidates` is null when one notes repo took the note. Otherwise no
     single repo covers your remote, and the note went to a new file in the
     personal repo's `inbox/`; `candidates` lists the repos that cover it.
     Tell the person.
   - `parked` is null when the text went into the note. Otherwise the note
     could not be written safely, for the reason the Board code names, and
     the text went to the `.agent.md` file beside it, at `path`. Tell the
     person, so they can move it into the note.
   - `commit` is null when the note was written but not committed. Tell the
     person.
   - `unpushed` is null when the commit reached the repo's remote.
     `notes::repo::not_synced` means the note is in the board's own tree,
     which `board sync` pushes with the rest of the board. Any other code
     means the text is committed in this machine's clone only and the push
     failed; Reflect's desktop app or the next call pushes it. Do not call
     again to retry, because the text is already written. Tell the person
     the code.
5. Fix what it refuses:
   - `notes::repo::not_found`: no notes repo is at `--repo`, or there is
     none. The `repos` field lists those there are.
   - `notes::repo::no_personal`: no notes repo covers your remote, and not
     exactly one is marked personal to take the note in its inbox. Nothing
     was written. Ask the person which repo to use, and pass `--repo`.
   - `notes::repo::ambiguous`: `daily` found no single repo covering your
     remote. Ask the person which repo's note to read, and pass `--repo`.
   - `notes::repo::invalid_record`: a notes repo's `notes-repo.json` is not
     a valid record. Tell the person; the message says how to fix it.
   - `board::config::invalid`: the person's Board config file is not valid,
     such as a `day_start` that is not `HH:MM`. Tell the person.
   - `notes::note::busy`: nothing was written. When `path` is under
     `inbox/`, the inbox names were taken, so call again. Otherwise the note and its `.agent.md` file both hold edits
     Reflect has not committed; ask the person to let Reflect save, then
     call again.
   - `notes::note::not_text`: the note is not UTF-8 text. Nothing was
     written. Tell the person.
   - `notes::board::refused` carrying a `board::sync::` code, such as
     `board::sync::failed`: the first sync could not reach the remote, so
     nothing was written. Tell the person.
