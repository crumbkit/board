import { expect, test } from 'claude-code/testing'

import { PROMPT_DAYS, digest, digests, kept, typedByOwner } from '../hooks/prompts'

// These prove which prompts the log takes and the digests it keeps, which
// `plans.answer --session` compares with an answer's words. The plans
// plugin's crate tests the comparison itself (board_model::heard_in).

// SHA-256 of "abc", the vector board_model::prompt_digest's test also checks.
const ABC = 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad'

test('a digest is the hex SHA-256 of the trimmed text', async () => {
  expect(await digest('abc')).toBe(ABC)
  expect(await digest('  abc\n')).toBe(ABC)
})

test('a prompt adds the digest of the whole and of each line with text, each once', async () => {
  const found = await digests('Approve it.\n\n  yes  \nApprove it.')
  expect(found).toEqual([
    await digest('Approve it.\n\n  yes  \nApprove it.'),
    await digest('Approve it.'),
    await digest('yes'),
  ])
  expect(await digests('   ')).toEqual([])
})

test('the log keeps 30 days of well-formed lines and drops the rest', () => {
  const now = 10_000_000
  const day = 24 * 60 * 60
  const fresh = `${now - day} ${ABC}`
  const edge = `${now - PROMPT_DAYS * day} ${ABC}`
  const old = `${now - (PROMPT_DAYS * day + 1)} ${ABC}`
  expect(kept([fresh, edge, old, 'garbage', `${now} short`, ''].join('\n'), now)).toEqual([fresh, edge])
})

test('the log drops a line dated after now, so no digest stays for good', () => {
  const now = 10_000_000
  expect(kept(`${now + 1} ${ABC}\n${now} ${ABC}`, now)).toEqual([`${now} ${ABC}`])
})

test('only a prompt the owner typed at the terminal or through Remote Control is logged', () => {
  const typed = { text: 'yes', origin: { kind: 'composer' } }
  expect(typedByOwner(typed)).toBe(true)
  expect(typedByOwner({ ...typed, origin: { kind: 'bridge' } })).toBe(true)
  for (const kind of ['peer', 'plugin', 'notification', 'sdk', 'unclassified']) {
    expect(typedByOwner({ ...typed, origin: { kind } })).toBe(false)
  }
  expect(typedByOwner({ ...typed, isMeta: true })).toBe(false)
  expect(typedByOwner({ ...typed, command: { name: 'ask', args: '' } })).toBe(false)
})
