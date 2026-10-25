// The one query the mod reads pending effects with. It is a plain constant,
// without interpolation, because crates/board-cli/src/tests/effects_mod.rs
// runs the same text against a real board: the text after the constant's
// opening backtick, up to the next backtick.

/**
 * Each pending effect no seen record answers, by id, with the fields the mod
 * shows: the first 2,000 characters of the text or record a put would write,
 * its size in bytes and its number of lines. A whole body could pass the
 * 4 MiB a process run keeps, so no column holds one.
 *
 * `board answer` keeps the pending effect and writes a `board/seen/v1` at the
 * same id under `/effects/seen/`, so an answered effect is one with a seen
 * record. The NOT IN can go once answered effects leave `/effects/pending/`.
 * `pendingSql(id)` in effects.ts narrows the first `LIKE` to one effect's
 * path, so that text must stay as it is.
 */
export const PENDING_SQL = `WITH e AS (
  SELECT logical, body, coalesce(json_extract(body, '$.text'), json_extract(body, '$.record')) AS content
  FROM records WHERE type = 'board/effect/v1' AND logical LIKE '/effects/pending/%'
)
SELECT substr(logical, 18) AS id,
  json_extract(body, '$.board') AS board,
  json_extract(body, '$.plugin') AS plugin,
  json_extract(body, '$.op') AS op,
  json_extract(body, '$.path') AS path,
  json_extract(body, '$.prior') IS NOT NULL AS replaces,
  substr(content, 1, 2000) AS head,
  length(CAST(content AS BLOB)) AS bytes,
  length(content) - length(replace(content, char(10), '')) + (content <> '' AND substr(content, -1) <> char(10)) AS lines
FROM e
WHERE substr(logical, 18) NOT IN (
  SELECT substr(logical, 15) FROM records WHERE type = 'board/seen/v1' AND logical LIKE '/effects/seen/%'
)
ORDER BY id`
