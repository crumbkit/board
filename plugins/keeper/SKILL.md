---
name: keeper
description: 'Consolidate the owner''s projects, tasks, changes and questions into a project tree and a status board page. Use it when the owner asks for a consolidation pass or the work view. Start with `board -C <board> keeper gather`, which reads the live facts. SKILL.md and README.md in the plugin''s directory say more.'
---

# keeper

The keeper plugin keeps a project tree of the owner's open work and draws it
as a status board page. A consolidation pass reads the live facts, decides
where every open change belongs, writes that as a map, and renders the page.

## A pass

1. `board -C <board> keeper gather` reads and writes nothing. It lists every
   open change with its phase, mount, task and touched date, the open tasks,
   the questions waiting on the owner, the stale work (tasks open 14 days with
   no change, changes in build with no commit for 3 days), the changes no
   project holds yet, and the latest map.
2. Decide the tree. Each open change goes in exactly one project, and a
   project belongs to one realm. A realm is a group such as one board or one
   team. A project's state is `active`, `queued`, `held`, `waiting-on-owner`,
   `stale` or `done`.
3. Record what you found as findings. A finding has a kind (`landed`,
   `superseded`, `merge`, `stale-plan`, `empty-plan`, `stale`, `loose-link`,
   `duplicate-mount` or `answered-open`), the paths it is about, the evidence
   and the capability that fixes it. Give no id; keeper.map computes one from
   the kind and the paths, so the same finding keeps its id from pass to pass.
4. Give keeper.map the input as JSON on stdin. It refuses, and writes
   nothing, when an open change is in no project, a change is in two, or a
   path names a change, task or realm that does not exist. The refusal code
   says which (`keeper::map::unplaced`, `duplicate`, `unknown_path`,
   `unknown_project`, `unknown_realm`).
5. `board -C <board> keeper render` writes the page. It reads the latest map
   and the live records, and refuses with `keeper::render::no_map` when there
   is none. The default path is `/projects/keeper/work-view/index.html`; pass
   `--path` for another.

## What the map holds

keeper.map writes `/projects/keeper/map/<timestamp>` and replaces
`/projects/keeper/map/latest` in one commit. It computes the phase, touched
date and pull request of each change from the change records, the finding ids
and the number of waiting questions per realm. Do not write them.

A `systems` list describes the repositories and services the work touches. A
system with `crates_from` names a mounted Cargo workspace, and render reads
its crates, their dependencies and their line counts from the mount; one with
`modules` lists them by hand; one with `external` is drawn as an outside
service. `question_rules` says which project a waiting question belongs to
when it names no change: `tasks` matches text in its blocked work and
`names` matches a project folder. `mount_realms` and `localRealm` give the
realm of a question that no rule places.

## Rules

- The page is a pure function of the map and the records. The same facts give
  the same bytes, whatever order they arrive in.
- Never edit the page by hand. Run keeper.render again.
- A map names the paths that existed on the day of the pass. A later pass
  replaces `latest`, and the timestamped records stay.
- Re-run the pass when `keeper.gather` shows changes in no project, and
  render again after any change to a map or to the tracked records.
