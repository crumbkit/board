import { expect, test } from 'claude-code/testing'

import { boardOf, packedRef, stampOf, unseen, whenChanged } from '../hooks/stamp'
import type { Reader } from '../hooks/stamp'

// These prove what the board's stamp reads and when a poll reads `board`.
// The files and the environment are stood in by a Reader, so each case
// states the board's .git exactly; register.tsx passes the plugin's own `$`; the effects mod's copy of stamp.ts is the same file, which
// .github/workflows/mods.yml checks.

const HOME = '/Users/me'
const SHA1 = '1'.repeat(40)
const SHA2 = '2'.repeat(40)

type File = { text?: string; size?: number; mtimeMs?: number; kind?: 'file' | 'dir' }

/** Stands in for the file system and the environment: each path the stamp may read, and each variable. */
function files(held: Record<string, File>, env: Record<string, string> = { HOME }): { $: Reader; held: Record<string, File> } {
  const $: Reader = {
    env: { get: async name => env[name] },
    fs: {
      read: async path => {
        const file = held[path]
        if (file?.text === undefined) throw new Error(`ENOENT: ${path}`)
        return file.text
      },
      exists: async path => held[path] !== undefined,
      stat: async path => {
        const file = held[path]
        if (file === undefined) throw new Error(`ENOENT: ${path}`)
        return { kind: file.kind ?? 'file', size: file.size ?? 0, mtimeMs: file.mtimeMs ?? 0 }
      },
    },
  }
  return { $, held }
}

const CONFIG = `${HOME}/.config/board/config.json`
/** A personal board at `~/board` whose HEAD names main, held in a ref file. */
const board = (): Record<string, File> => ({
  [CONFIG]: { text: JSON.stringify({ boards: [], personal: '~/board' }) },
  [`${HOME}/board/.git`]: { kind: 'dir' },
  [`${HOME}/board/.git/HEAD`]: { text: 'ref: refs/heads/main\n' },
  [`${HOME}/board/.git/refs/heads/main`]: { text: `${SHA1}\n` },
  [`${HOME}/board/.git/index`]: { size: 4096, mtimeMs: 1000 },
})

test('the stamp is the commit HEAD names through its ref file and the index size and modified time', async () => {
  const { $ } = files(board())
  expect(await stampOf($, '')).toBe(`${SHA1} 4096 1000`)
})

test('a HEAD whose branch is packed is read from packed-refs, and a detached HEAD names its commit itself', async () => {
  const { $, held } = files(board())
  delete held[`${HOME}/board/.git/refs/heads/main`]
  held[`${HOME}/board/.git/packed-refs`] = { text: `# pack-refs with: peeled fully-peeled sorted\n${SHA2} refs/heads/other\n${SHA1} refs/heads/main\n^${SHA2}\n` }
  expect(await stampOf($, '')).toBe(`${SHA1} 4096 1000`)
  held[`${HOME}/board/.git/HEAD`] = { text: `${SHA2}\n` }
  expect(await stampOf($, '')).toBe(`${SHA2} 4096 1000`)
})

test('a board whose .git is a file reads HEAD and index from the directory it names and refs from its common directory', async () => {
  const { $ } = files({
    [CONFIG]: { text: JSON.stringify({ personal: '/work/tree' }) },
    '/work/tree/.git': { text: 'gitdir: /work/main/.git/worktrees/tree\n' },
    '/work/main/.git/worktrees/tree/HEAD': { text: 'ref: refs/heads/side\n' },
    '/work/main/.git/worktrees/tree/commondir': { text: '../..\n' },
    '/work/main/.git/worktrees/tree/../../refs/heads/side': { text: `${SHA2}\n` },
    '/work/main/.git/worktrees/tree/index': { size: 7, mtimeMs: 9 },
  })
  expect(await stampOf($, '')).toBe(`${SHA2} 7 9`)
})

test('the configured board wins over the personal one, and XDG_CONFIG_HOME moves the config', async () => {
  const { $ } = files({ '/xdg/board/config.json': { text: JSON.stringify({ personal: '/p' }) } }, { HOME, XDG_CONFIG_HOME: '/xdg' })
  expect(await boardOf($, '/boards/me')).toBe('/boards/me')
  expect(await boardOf($, '~/b')).toBe(`${HOME}/b`)
  expect(await boardOf($, '')).toBe('/p')
})

test('the stamp is unknown when the config names no personal board, a ref names no commit, or the index is missing', async () => {
  const { $, held } = files(board())
  expect(await stampOf($, '')).not.toBeNull()
  delete held[`${HOME}/board/.git/index`]
  expect(await stampOf($, '')).toBeNull()
  held[`${HOME}/board/.git/index`] = { size: 1, mtimeMs: 1 }
  delete held[`${HOME}/board/.git/refs/heads/main`]
  expect(await stampOf($, '')).toBeNull()
  held[`${HOME}/board/.git/refs/heads/main`] = { text: SHA1 }
  held[CONFIG] = { text: JSON.stringify({ boards: [] }) }
  expect(await stampOf($, '')).toBeNull()
  held[CONFIG] = { text: 'not json' }
  expect(await stampOf($, '')).toBeNull()
})

test('packed-refs is read by exact ref name, past comments and peeled lines', () => {
  const packed = `# pack-refs\n${SHA2} refs/heads/mainline\n^${SHA1}\n${SHA1} refs/heads/main\n`
  expect(packedRef(packed, 'refs/heads/main')).toBe(SHA1)
  expect(packedRef(packed, 'refs/heads/gone')).toBeNull()
})

test('an unchanged stamp reads nothing, and its twin, a new commit or an index write, reads once', async () => {
  const { $, held } = files(board())
  const seen = unseen()
  let reads = 0
  const read = async () => {
    reads++
    return true
  }
  expect(await whenChanged($, '', seen, read)).toBe(true)
  expect(await whenChanged($, '', seen, read)).toBe(false)
  expect(reads).toBe(1)
  held[`${HOME}/board/.git/refs/heads/main`] = { text: SHA2 }
  expect(await whenChanged($, '', seen, read)).toBe(true)
  expect(await whenChanged($, '', seen, read)).toBe(false)
  held[`${HOME}/board/.git/index`] = { size: 4096, mtimeMs: 2000 }
  expect(await whenChanged($, '', seen, read)).toBe(true)
  expect(await whenChanged($, '', seen, read)).toBe(false)
  expect(reads).toBe(3)
})

test('a read that fails is tried again by the next poll, and its twin that works is not', async () => {
  const { $ } = files(board())
  const seen = unseen()
  let worked = false
  let reads = 0
  const read = async () => {
    reads++
    return worked
  }
  await whenChanged($, '', seen, read)
  await whenChanged($, '', seen, read)
  expect(reads).toBe(2)
  worked = true
  await whenChanged($, '', seen, read)
  await whenChanged($, '', seen, read)
  expect(reads).toBe(3)
})

test('a read that throws is tried again by the next poll', async () => {
  const { $ } = files(board())
  const seen = unseen()
  await whenChanged($, '', seen, async () => true)
  const thrown = await whenChanged($, '', seen, async () => {
    throw new Error('board did not run')
  }, true).catch((error: unknown) => String(error))
  expect(thrown).toBe('Error: board did not run')
  expect(await whenChanged($, '', seen, async () => true)).toBe(true)
})

test('an unknown stamp reads every time, and a read the owner starts runs even when the stamp is unchanged', async () => {
  const { $, held } = files(board())
  const seen = unseen()
  let reads = 0
  const read = async () => {
    reads++
    return true
  }
  await whenChanged($, '', seen, read)
  expect(await whenChanged($, '', seen, read, true)).toBe(true)
  delete held[CONFIG]
  expect(await whenChanged($, '', seen, read)).toBe(true)
  expect(await whenChanged($, '', seen, read)).toBe(true)
  expect(reads).toBe(4)
})
