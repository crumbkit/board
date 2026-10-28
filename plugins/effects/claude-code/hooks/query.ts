// The one query the mod reads pending effects with. It is a plain constant,
// without interpolation, because crates/board-cli/src/tests/effects_mod.rs
// runs the same text against a real board: the text after the constant's
// opening backtick, up to the next backtick.

/**
 * Each pending effect no seen record answers, by id, with the fields the mod
 * shows: the board, plugin, op and path.
 *
 * `board answer` keeps the pending effect and writes a `board/seen/v1` at the
 * same id under `/effects/seen/`, so an answered effect is one with a seen
 * record Board reads as an answer. The index parses each seen record as
 * Board's `Seen` and marks one that fails as degraded, so `status = 'ok'`
 * is the same check Board makes.
 * The NOT IN can go once answered effects leave `/effects/pending/`.
 * `pendingSql(id)` in effects.ts narrows the first `LIKE` to one effect's
 * path, so that text must stay as it is.
 */
export const PENDING_SQL = `WITH e AS (
  SELECT logical, body FROM records WHERE type = 'board/effect/v1' AND logical LIKE '/effects/pending/%'
)
SELECT substr(logical, 18) AS id,
  json_extract(body, '$.board') AS board,
  json_extract(body, '$.plugin') AS plugin,
  json_extract(body, '$.op') AS op,
  json_extract(body, '$.path') AS path
FROM e
WHERE substr(logical, 18) NOT IN (
  SELECT substr(logical, 15) FROM records WHERE type = 'board/seen/v1' AND logical LIKE '/effects/seen/%'
    AND status = 'ok'
)
ORDER BY id`
