import { expect, test } from 'bun:test'

import {
  CommandError,
  normalizeDirPage,
  normalizeExtractPlan,
  normalizeOpenResult,
  wrapNativeModule,
} from './napi'

test('Regression: omitted nextCursor is null so paging stops', () => {
  const page = normalizeDirPage({ path: '/', entries: [] })
  expect(page.nextCursor).toBeNull()
})

test('normalizeDirPage maps snake_case nullable fields', () => {
  const page = normalizeDirPage({
    path: '/',
    entries: [
      {
        name: 'dir-00',
        path: '/dir-00',
        is_dir: true,
        size: 0,
        mtime: null,
        mode: 0o755,
      },
    ],
    next_cursor: null,
    total_hint: 10,
  })
  expect(page.entries[0]?.isDir).toBe(true)
  expect(page.entries[0]?.mtime).toBeNull()
  expect(page.nextCursor).toBeNull()
  expect(page.totalHint).toBe(10)
})

test('normalizeOpenResult accepts sessionId or jobId', () => {
  expect(normalizeOpenResult({ sessionId: 1, jobId: null })).toEqual({ sessionId: 1 })
  expect(normalizeOpenResult({ session_id: null, job_id: 7 })).toEqual({ jobId: 7 })
})

test('wrapNativeModule exposes pickFile/open/list/close/on', async () => {
  const listPages = [
    { path: '/', entries: [{ name: 'a', path: '/a', isDir: false, size: 1, mtime: null, mode: 0 }], nextCursor: 'kset:/:1', totalHint: 2 },
    { path: '/', entries: [{ name: 'b', path: '/b', isDir: false, size: 1, mtime: null, mode: 0 }], nextCursor: null, totalHint: 2 },
  ]
  let listN = 0
  const addon = wrapNativeModule({
    pickFile: () => '/tmp/hello.tar',
    pickDir: () => '/tmp/out',
    open: () => ({ sessionId: 1, jobId: null }),
    close: () => {},
    list: () => listPages[listN++] ?? listPages[1],
    lookup: () => null,
    preview: () => ({ kind: 'skipped', reason: 'unknown' }),
    extractPlan: () => ({ files: 0, bytes: 0, conflictCount: 0, conflicts: [], conflictsTruncated: false }),
    extract: () => ({ jobId: 1 }),
    cancel: () => {},
    getConfig: () => ({
      extract: { overwrite: 'ask', allowUnsafePaths: false },
      preview: { maxBytes: 8 * 1024 * 1024, openLargeWithSystem: true },
    }),
    on: () => {},
  })
  expect(await addon.pickFile()).toBe('/tmp/hello.tar')
  expect(await addon.open({ source: '/tmp/hello.tar', policy: 'sibling', recreate: 'if-invalid' })).toEqual({
    sessionId: 1,
  })
  const first = await addon.list({ sessionId: 1, path: '/' })
  expect(first.nextCursor).toBe('kset:/:1')
  const last = await addon.list({ sessionId: 1, path: '/', cursor: first.nextCursor ?? undefined })
  expect(last.nextCursor).toBeNull()
})

test('wrapNativeModule maps command errors onto CommandError', async () => {
  const addon = wrapNativeModule({
    pickFile: () => {
      throw Object.assign(new Error('dialog failed'), { code: 'Internal', retryable: false })
    },
    pickDir: () => null,
    open: () => {
      throw Object.assign(new Error('unknown archive'), { code: 'NotFound', retryable: false })
    },
    close: () => {},
    list: () => {
      throw new Error('boom')
    },
    lookup: () => null,
    preview: () => ({ kind: 'skipped', reason: 'unknown' }),
    extractPlan: () => ({ files: 0, bytes: 0, conflictCount: 0, conflicts: [], conflictsTruncated: false }),
    extract: () => ({ jobId: 1 }),
    cancel: () => {},
    getConfig: () => ({
      extract: { overwrite: 'ask', allowUnsafePaths: false },
      preview: { maxBytes: 8 * 1024 * 1024, openLargeWithSystem: true },
    }),
    on: () => {
      throw new Error('bad event')
    },
  })
  try {
    await addon.open({ source: 'nope', policy: 'sibling', recreate: 'if-invalid' })
    throw new Error('expected open to throw')
  } catch (err) {
    expect(err).toBeInstanceOf(CommandError)
    expect((err as CommandError).code).toBe('NotFound')
    expect((err as CommandError).retryable).toBe(false)
  }
  try {
    await addon.list({ sessionId: 1, path: '/' })
    throw new Error('expected list to throw')
  } catch (err) {
    expect(err).toBeInstanceOf(CommandError)
    expect((err as CommandError).message).toBe('boom')
  }
  try {
    await addon.pickFile()
    throw new Error('expected pickFile to throw')
  } catch (err) {
    expect(err).toBeInstanceOf(CommandError)
    expect((err as CommandError).message).toBe('dialog failed')
  }
  expect(() => addon.on('jobSucceeded', () => {})).toThrow(CommandError)
})

test('wrapNativeModule extract rejects overwrite ask before native', async () => {
  const addon = wrapNativeModule({
    pickFile: () => null,
    pickDir: () => '/tmp/out',
    open: () => ({ sessionId: 1 }),
    close: () => {},
    list: () => ({ path: '/', entries: [], nextCursor: null, totalHint: 0 }),
    lookup: () => null,
    preview: () => ({ kind: 'skipped', reason: 'unknown' }),
    extractPlan: () => ({ files: 0, bytes: 0, conflictCount: 0, conflicts: [], conflictsTruncated: false }),
    extract: () => {
      throw new Error('native extract must not be called with ask')
    },
    cancel: () => {},
    getConfig: () => ({
      extract: { overwrite: 'ask', allowUnsafePaths: false },
      preview: { maxBytes: 8 * 1024 * 1024, openLargeWithSystem: true },
    }),
    on: () => {},
  })
  try {
    await addon.extract({
      sessionId: 1,
      members: [],
      destDir: '/tmp/out',
      overwrite: 'ask' as unknown as 'skip',
    })
    throw new Error('expected extract to throw')
  } catch (err) {
    expect(err).toBeInstanceOf(CommandError)
    expect((err as CommandError).code).toBe('Internal')
    expect((err as CommandError).retryable).toBe(false)
  }
})

test('normalizeExtractPlan caps conflicts at 50', () => {
  const plan = normalizeExtractPlan({
    files: 1000,
    bytes: 1,
    conflict_count: 1000,
    conflicts: Array.from({ length: 80 }, (_, i) => ({ member: `/m${i}`, dest_path: `/d/m${i}` })),
    conflicts_truncated: true,
  })
  expect(plan.conflicts.length).toBeLessThanOrEqual(50)
  expect(plan.conflictsTruncated).toBe(true)
  expect(plan.conflictCount).toBe(1000)
})
