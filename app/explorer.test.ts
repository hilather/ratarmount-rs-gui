import { expect, test } from 'bun:test'
import type { ReactElement } from 'react'

import { ExplorerView, explorerHandlers, type ExplorerHandlers } from './explorer-view'
import {
  countLabel,
  crumbsFor,
  crumbTestId,
  ExplorerController,
  EXTRACT_CONFIRM_BYTES,
  EXTRACT_CONFIRM_FILES,
  formatMtime,
  formatSize,
  LIST_LIMIT_DEFAULT,
  parentPath,
  shortenPath,
  siblingIndexPath,
  type ExplorerSnapshot,
} from './explorer'
import {
  createFakeNative,
  FAKE_ENCRYPTED_PASSWORD,
  FAKE_MTIME,
  FAKE_ROOT_FILE_COUNT,
  FAKE_ROOT_TOTAL,
  NINE_MIB,
} from './fake-native'
import {
  changeByTestId,
  clickByTestId,
  collectTestIds,
  getByTestId,
  keyDownByTestId,
  queryByTestId,
} from './gpuix-test'

const noop: ExplorerHandlers = {
  onOpen() {},
  onClose() {},
  onExtract() {},
  onExtractAll() {},
  onCrumb() {},
  onRowClick() {},
  onKey() {},
  onVisibleRange() {},
  onCancelExtract() {},
  onRetryExtract() {},
  onConfirmExtract() {},
  onOverwriteSkip() {},
  onOverwriteReplace() {},
  onDismissDialog() {},
  onPasswordSubmit() {},
  onExtractOpenSystem() {},
}

function renderView(model: ExplorerSnapshot, handlers: Partial<ExplorerHandlers> = {}) {
  return ExplorerView({ model, ...noop, ...handlers })
}

async function openRoot(limit = LIST_LIMIT_DEFAULT) {
  const fake = createFakeNative({ pickFile: '/archives/hello.tar' })
  const controller = new ExplorerController({ listLimit: limit })
  controller.setNative(fake)
  await controller.openPicked()
  return { fake, controller }
}

async function waitFor(
  controller: ExplorerController,
  pred: (snap: ExplorerSnapshot) => boolean,
): Promise<void> {
  if (pred(controller.getSnapshot())) {
    return
  }
  await new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => {
      unsub()
      reject(new Error(`timeout waiting for explorer snapshot`))
    }, 1000)
    const unsub = controller.subscribe(() => {
      if (pred(controller.getSnapshot())) {
        clearTimeout(timer)
        unsub()
        resolve()
      }
    })
  })
}

test('crumb helpers encode GPUIX crumb-* testIds', () => {
  expect(crumbTestId('/')).toBe('crumb-root')
  expect(crumbTestId('/dir-00')).toBe('crumb-dir-00')
  expect(crumbsFor('/').map((c) => c.testId)).toEqual(['crumb-root'])
  expect(crumbsFor('/dir-00').map((c) => c.testId)).toEqual(['crumb-root', 'crumb-dir-00'])
  expect(parentPath('/')).toBeNull()
  expect(parentPath('/dir-00')).toBe('/')
  expect(parentPath('/dir-00/a.txt')).toBe('/dir-00')
})

test('shortenPath keeps the last two components', () => {
  expect(shortenPath('hello.tar')).toBe('hello.tar')
  expect(shortenPath('/very/long/prefix/archives/hello.tar', 20)).toBe('…/archives/hello.tar')
  expect(siblingIndexPath('/archives/hello.tar')).toBe('/archives/hello.tar.index.sqlite')
})

test('formatSize and formatMtime are stable', () => {
  expect(formatSize({ isDir: true, size: 0 })).toBe('—')
  expect(formatSize({ isDir: false, size: 104 })).toBe('104 B')
  expect(formatMtime(null)).toBe('—')
  expect(formatMtime(FAKE_MTIME)).toBe('2023-11-14 22:13')
  expect(countLabel(660, 200, true)).toBe('660 entries')
  expect(countLabel(null, 200, true)).toBe('200+ entries')
})

test('getByTestId finds open on the idle explorer chrome', () => {
  const fake = createFakeNative()
  const controller = new ExplorerController()
  controller.setNative(fake)
  const tree = renderView(controller.getSnapshot())
  expect(getByTestId(tree, 'open')).toBeTruthy()
  expect(queryByTestId(tree, 'list')).toBeNull()
  expect(getByTestId(tree, 'placeholder')).toBeTruthy()
})

test('getByTestId open/list/crumb-* against the fake catalog', async () => {
  const { controller } = await openRoot()
  const snap = controller.getSnapshot()
  const tree = renderView(snap)

  expect(getByTestId(tree, 'open')).toBeTruthy()
  expect(getByTestId(tree, 'list')).toBeTruthy()
  expect(getByTestId(tree, 'crumb-root')).toBeTruthy()
  expect(queryByTestId(tree, 'crumb-dir-00')).toBeNull()
  expect(getByTestId(tree, 'row-dir-00')).toBeTruthy()
  expect(getByTestId(tree, 'status-count')).toBeTruthy()

  const ids = collectTestIds(tree)
  expect(ids.filter((id) => id.startsWith('crumb-'))).toEqual(['crumb-root'])
})

test('React state holds the current page, not the full fake catalog', async () => {
  const { fake, controller } = await openRoot()
  const snap = controller.getSnapshot()
  expect(snap.status).toBe('ready')
  expect(snap.entries.length).toBe(LIST_LIMIT_DEFAULT)
  expect(snap.entries.length).toBeLessThan(FAKE_ROOT_TOTAL)
  expect(snap.nextCursor).not.toBeNull()
  expect(snap.totalHint).toBe(FAKE_ROOT_TOTAL)
  expect(snap.entries.some((e) => e.name === 'file-649')).toBe(false)
  expect(fake.listCalls).toHaveLength(1)
  expect(fake.listCalls[0]?.cursor).toBeUndefined()
  expect(fake.listCalls[0]?.limit).toBe(LIST_LIMIT_DEFAULT)

  const tree = renderView(snap)
  const rowIds = collectTestIds(tree).filter((id) => id.startsWith('row-'))
  expect(rowIds.length).toBe(LIST_LIMIT_DEFAULT)
  expect(rowIds.length).toBeLessThan(FAKE_ROOT_TOTAL)
})

test('entering a directory updates crumbs and lists that page', async () => {
  const { controller } = await openRoot()
  await controller.enterPath('/dir-00')
  const snap = controller.getSnapshot()
  expect(snap.path).toBe('/dir-00')
  expect(snap.entries.map((e) => e.name)).toEqual(['a.txt', 'b.txt', 'c.txt'])
  expect(snap.nextCursor).toBeNull()
  expect(snap.entries.length).toBeLessThan(LIST_LIMIT_DEFAULT)

  const tree = renderView(snap)
  expect(getByTestId(tree, 'open')).toBeTruthy()
  expect(getByTestId(tree, 'list')).toBeTruthy()
  expect(getByTestId(tree, 'crumb-root')).toBeTruthy()
  expect(getByTestId(tree, 'crumb-dir-00')).toBeTruthy()
  expect(getByTestId(tree, 'row-a.txt')).toBeTruthy()
})

test('keyboard Enter/Backspace/j-k navigate the fake catalog', async () => {
  const { controller } = await openRoot()
  expect(controller.getSnapshot().entries[0]?.name).toBe('dir-00')
  controller.handleKey('enter')
  await waitFor(
    controller,
    (s) => s.path === '/dir-00' && !s.listing && s.entries.length === 3,
  )
  expect(controller.getSnapshot().entries.map((e) => e.name)).toEqual(['a.txt', 'b.txt', 'c.txt'])

  controller.handleKey('backspace')
  await waitFor(
    controller,
    (s) => s.path === '/' && !s.listing && s.entries.length === LIST_LIMIT_DEFAULT,
  )

  controller.handleKey('j')
  expect(controller.getSnapshot().selectedIndex).toBe(1)
  controller.handleKey('k')
  expect(controller.getSnapshot().selectedIndex).toBe(0)
  controller.handleKey('down')
  expect(controller.getSnapshot().selectedIndex).toBe(1)
})

test('go up from a crumb-root click stays at archive root', async () => {
  const { controller } = await openRoot()
  await controller.enterPath('/dir-00')
  await controller.enterPath('/')
  expect(controller.getSnapshot().path).toBe('/')
  expect(controller.getSnapshot().entries[0]?.name).toBe('dir-00')
})

test('loadMore appends the next page and does not parse the cursor', async () => {
  const { fake, controller } = await openRoot()
  const firstCursor = controller.getSnapshot().nextCursor
  expect(typeof firstCursor).toBe('string')
  expect(firstCursor).not.toBeNull()
  expect(Number.isFinite(Number(firstCursor))).toBe(false)

  await controller.loadMore()
  const snap = controller.getSnapshot()
  expect(snap.entries.length).toBe(LIST_LIMIT_DEFAULT * 2)
  expect(snap.entries.length).toBeLessThan(FAKE_ROOT_TOTAL)
  expect(fake.listCalls).toHaveLength(2)
  expect(fake.listCalls[1]?.cursor).toBe(firstCursor as string)
  expect(snap.entries.some((e) => e.name === `file-${String(FAKE_ROOT_FILE_COUNT - 1).padStart(3, '0')}`)).toBe(
    false,
  )
})

test('Regression: last-page nextCursor null must not restart page 1', async () => {
  const { fake, controller } = await openRoot()
  await controller.enterPath('/dir-00')
  expect(controller.getSnapshot().nextCursor).toBeNull()
  const calls = fake.listCalls.length
  await controller.loadMore()
  controller.onVisibleRange(0, 3)
  expect(fake.listCalls.length).toBe(calls)
  expect(controller.getSnapshot().entries[0]?.name).toBe('a.txt')
  expect(controller.getSnapshot().entries.length).toBe(3)
})

test('onVisibleRange near the end requests the next page', async () => {
  const { fake, controller } = await openRoot()
  controller.onVisibleRange(0, LIST_LIMIT_DEFAULT - 4)
  await waitFor(controller, (s) => s.entries.length === LIST_LIMIT_DEFAULT * 2)
  expect(fake.listCalls.length).toBe(2)
})

test('empty directory shows list-empty', async () => {
  const { controller } = await openRoot()
  await controller.enterPath('/dir-01')
  const tree = renderView(controller.getSnapshot())
  expect(getByTestId(tree, 'list')).toBeTruthy()
  expect(getByTestId(tree, 'list-empty')).toBeTruthy()
  expect(getByTestId(tree, 'crumb-dir-01')).toBeTruthy()
})

test('error state exposes testId error', async () => {
  const fake = createFakeNative()
  const controller = new ExplorerController()
  controller.setNative(fake)
  await controller.openSource('/tmp/hello.tar')
  await controller.enterPath('/missing')
  expect(controller.getSnapshot().status).toBe('error')
  const tree = renderView(controller.getSnapshot())
  expect(getByTestId(tree, 'error')).toBeTruthy()
  expect((getByTestId(tree, 'error').props as { children?: string }).children).toContain(
    'path not found',
  )
})

test('Close returns to the empty placeholder chrome', async () => {
  const { fake, controller } = await openRoot()
  await controller.closeArchive()
  expect(controller.getSnapshot().status).toBe('idle')
  expect(controller.getSnapshot().entries).toEqual([])
  expect(fake.closedSessions).toEqual([1])
  const tree = renderView(controller.getSnapshot())
  expect(getByTestId(tree, 'open')).toBeTruthy()
  expect(getByTestId(tree, 'placeholder')).toBeTruthy()
  expect(queryByTestId(tree, 'list')).toBeNull()
})

test('dismissing the file picker does not open a session', async () => {
  const fake = createFakeNative({ pickFile: null })
  const controller = new ExplorerController()
  controller.setNative(fake)
  await controller.openPicked()
  expect(controller.getSnapshot().status).toBe('idle')
  expect(fake.openCalls).toHaveLength(0)
})

test('double-clicking a directory row enters it', async () => {
  const { controller } = await openRoot()
  controller.onRowClick(0, 2)
  await waitFor(
    controller,
    (s) => s.path === '/dir-00' && !s.listing && s.entries.length === 3,
  )
})

test('W4 source does not call readAll and does not pass overwrite ask', async () => {
  const files = ['explorer.ts', 'explorer-view.tsx', 'app.tsx', 'napi.ts', 'native-addon.ts']
  for (const file of files) {
    const source = await Bun.file(new URL(`./${file}`, import.meta.url)).text()
    expect(source).not.toMatch(/\breadAll\s*\(/)
    expect(source).not.toMatch(/overwrite:\s*['"]ask['"]/)
  }
  const explorer = await Bun.file(new URL('./explorer.ts', import.meta.url)).text()
  expect(explorer).toMatch(/\bextractPlan\s*\(/)
  expect(explorer).toMatch(/\bpreview\s*\(/)
  expect(explorer).not.toMatch(/\bnative\.find\s*\(/)
})

test('ExplorerView is a GPUIX host tree (virtual-list + testIds)', async () => {
  const { controller } = await openRoot()
  const tree = renderView(controller.getSnapshot()) as ReactElement
  expect(tree.type).toBe('div')
  const ids = collectTestIds(tree)
  expect(ids).toContain('open')
  expect(ids).toContain('list')
  expect(ids).toContain('crumb-root')
  expect(ids).toContain('extract')
  expect(ids).toContain('preview')
  expect(ids).not.toContain('search')
})

test('getByTestId chrome controls fire ExplorerView handlers', async () => {
  const { controller } = await openRoot()
  await controller.enterPath('/dir-00')
  const tree = renderView(controller.getSnapshot(), explorerHandlers(controller))

  clickByTestId(tree, 'crumb-root')
  await waitFor(
    controller,
    (s) => s.path === '/' && !s.listing && s.entries.length === LIST_LIMIT_DEFAULT,
  )

  const listed = renderView(controller.getSnapshot(), explorerHandlers(controller))
  clickByTestId(listed, 'row-dir-00', { clickCount: 2 })
  await waitFor(controller, (s) => s.path === '/dir-00' && !s.listing && s.entries.length === 3)

  const nested = renderView(controller.getSnapshot(), explorerHandlers(controller))
  keyDownByTestId(nested, 'explorer', 'backspace')
  await waitFor(
    controller,
    (s) => s.path === '/' && !s.listing && s.entries.length === LIST_LIMIT_DEFAULT,
  )

  const root = renderView(controller.getSnapshot(), explorerHandlers(controller))
  keyDownByTestId(root, 'explorer', 'j')
  expect(controller.getSnapshot().selectedIndex).toBe(1)

  clickByTestId(root, 'close')
  await waitFor(controller, (s) => s.status === 'idle')
})

test('Open host onClick opens via the fake picker', async () => {
  const fake = createFakeNative({ pickFile: '/archives/hello.tar' })
  const controller = new ExplorerController()
  controller.setNative(fake)
  const tree = renderView(controller.getSnapshot(), explorerHandlers(controller))
  clickByTestId(tree, 'open')
  await waitFor(
    controller,
    (s) => s.status === 'ready' && s.entries.length === LIST_LIMIT_DEFAULT,
  )
  expect(fake.openCalls).toHaveLength(1)
})

test('Open is disabled until the native addon is ready', () => {
  const controller = new ExplorerController()
  let opened = false
  const tree = renderView(controller.getSnapshot(), {
    onOpen: () => {
      opened = true
    },
  })
  clickByTestId(tree, 'open')
  expect(opened).toBe(false)
  expect(controller.getSnapshot().nativeReady).toBe(false)
})

test('Open retries loadNative when the first import failed', async () => {
  const fake = createFakeNative({ pickFile: '/archives/hello.tar' })
  const controller = new ExplorerController()
  let attempts = 0
  controller.setNativeLoader(async () => {
    attempts++
    return fake
  })
  controller.failLoad(new Error('Native addon is not built'))
  expect(controller.getSnapshot().status).toBe('error')
  expect(controller.getSnapshot().nativeReady).toBe(false)

  await controller.openPicked()
  expect(attempts).toBe(1)
  expect(controller.getSnapshot().status).toBe('ready')
  expect(controller.getSnapshot().nativeReady).toBe(true)
})

test('setNative clears a previous addon-load error', () => {
  const fake = createFakeNative()
  const controller = new ExplorerController()
  controller.failLoad(new Error('Native addon is not built'))
  controller.setNative(fake)
  expect(controller.getSnapshot().status).toBe('idle')
  expect(controller.getSnapshot().error).toBeNull()
  expect(controller.getSnapshot().nativeReady).toBe(true)
})

test('open { jobId } plus jobSucceeded reaches ready', async () => {
  const fake = createFakeNative({ pickFile: '/archives/hello.tar', openMode: 'job' })
  const controller = new ExplorerController()
  controller.setNative(fake)
  await controller.openPicked()
  expect(controller.getSnapshot().status).toBe('ready')
  expect(controller.getSnapshot().entries.length).toBe(LIST_LIMIT_DEFAULT)
  expect(fake.openCalls[0]?.recreate).toBe('if-invalid')
})

test('open jobFailed surfaces the command error', async () => {
  const fake = createFakeNative({ pickFile: '/archives/hello.tar', openMode: 'job-failed' })
  const controller = new ExplorerController()
  controller.setNative(fake)
  await controller.openPicked()
  expect(controller.getSnapshot().status).toBe('error')
  expect(controller.getSnapshot().error).toContain('index failed')
})

test('jobSucceeded without sessionId rejects instead of hanging', async () => {
  const fake = createFakeNative({
    pickFile: '/archives/hello.tar',
    openMode: 'job-no-session',
  })
  const controller = new ExplorerController()
  controller.setNative(fake)
  await Promise.race([
    controller.openPicked(),
    new Promise((_, reject) => {
      setTimeout(() => reject(new Error('hung waiting for jobSucceeded sessionId')), 200)
    }),
  ])
  expect(controller.getSnapshot().status).toBe('error')
  expect(controller.getSnapshot().error).toMatch(/sessionId/)
})

test('App wires explorerHandlers onto the injected native', async () => {
  const source = await Bun.file(new URL('./app.tsx', import.meta.url)).text()
  expect(source).toContain('explorerHandlers')
  expect(source).toContain('setNativeLoader')
  expect(source).toContain('native?: NativeAddon')
})

test('ctrl-click multi-selects rows without entering a directory', async () => {
  const { controller } = await openRoot()
  controller.onRowClick(0, 1)
  controller.onRowClick(1, 1, { ctrl: true })
  const snap = controller.getSnapshot()
  expect(snap.selectedPaths).toEqual(['/dir-00', '/dir-01'])
  expect(snap.path).toBe('/')
})

test('Extract to uses pickDir, extractPlan, and skip|replace only', async () => {
  const { fake, controller } = await openRoot()
  const tree = renderView(controller.getSnapshot(), explorerHandlers(controller))
  clickByTestId(tree, 'extract')
  await waitFor(controller, (s) => s.extractJob?.status === 'succeeded')
  expect(fake.extractPlanCalls).toHaveLength(1)
  expect(fake.extractCalls).toHaveLength(1)
  expect(fake.extractCalls[0]?.overwrite === 'skip' || fake.extractCalls[0]?.overwrite === 'replace').toBe(
    true,
  )
  expect(fake.extractCalls[0]?.overwrite).not.toBe('ask')
  expect(fake.extractCalls[0]?.destDir).toBe('/tmp/out')
  expect(fake.extractCalls[0]?.members).toEqual(['/dir-00'])
  const after = renderView(controller.getSnapshot(), explorerHandlers(controller))
  expect(getByTestId(after, 'progress')).toBeTruthy()
})

test('extract-all confirm when extractPlan files exceed 1000', async () => {
  const fake = createFakeNative({
    pickFile: '/archives/hello.tar',
    extractPlan: {
      files: EXTRACT_CONFIRM_FILES + 1,
      bytes: 10,
      conflictCount: 0,
      conflicts: [],
      conflictsTruncated: false,
    },
  })
  const controller = new ExplorerController()
  controller.setNative(fake)
  await controller.openPicked()
  const tree = renderView(controller.getSnapshot(), explorerHandlers(controller))
  clickByTestId(tree, 'extract-all')
  await waitFor(controller, (s) => s.dialog.kind === 'confirm-extract')
  expect(fake.extractCalls).toHaveLength(0)
  expect(controller.getSnapshot().entries.length).toBe(LIST_LIMIT_DEFAULT)
  const dialog = renderView(controller.getSnapshot(), explorerHandlers(controller))
  expect(getByTestId(dialog, 'confirm-extract')).toBeTruthy()
  clickByTestId(dialog, 'confirm-extract-ok')
  await waitFor(controller, (s) => s.extractJob?.status === 'succeeded')
  expect(fake.extractCalls[0]?.overwrite).not.toBe('ask')
})

test('overwrite dialog uses the extractPlan sample, not 1k paths', async () => {
  const sample = Array.from({ length: 50 }, (_, i) => ({
    member: `/file-${String(i).padStart(4, '0')}.txt`,
    destPath: `/tmp/out/file-${String(i).padStart(4, '0')}.txt`,
  }))
  const fake = createFakeNative({
    pickFile: '/archives/hello.tar',
    extractPlan: {
      files: 1000,
      bytes: EXTRACT_CONFIRM_BYTES + 1,
      conflictCount: 1000,
      conflicts: sample,
      conflictsTruncated: true,
    },
  })
  const controller = new ExplorerController()
  controller.setNative(fake)
  await controller.openPicked()
  const tree = renderView(controller.getSnapshot(), explorerHandlers(controller))
  clickByTestId(tree, 'extract-all')
  await waitFor(controller, (s) => s.dialog.kind === 'confirm-extract')
  const confirm = renderView(controller.getSnapshot(), explorerHandlers(controller))
  clickByTestId(confirm, 'confirm-extract-ok')
  await waitFor(controller, (s) => s.dialog.kind === 'overwrite')
  const snap = controller.getSnapshot()
  expect(snap.entries.length).toBe(LIST_LIMIT_DEFAULT)
  expect(snap.entries.length).toBeLessThan(1000)
  if (snap.dialog.kind !== 'overwrite') {
    throw new Error('expected overwrite dialog')
  }
  expect(snap.dialog.conflicts.length).toBeLessThanOrEqual(50)
  expect(snap.dialog.truncated).toBe(true)
  expect(snap.dialog.conflictCount).toBe(1000)
  const dialog = renderView(snap, explorerHandlers(controller))
  expect(getByTestId(dialog, 'overwrite-dialog')).toBeTruthy()
  expect(getByTestId(dialog, 'overwrite-more')).toBeTruthy()
  clickByTestId(dialog, 'overwrite-skip')
  await waitFor(controller, (s) => s.extractJob?.status === 'succeeded')
  expect(fake.extractCalls[0]?.overwrite).toBe('skip')
})

test('preview pane shows text under 1 KiB', async () => {
  const { controller } = await openRoot()
  await controller.enterPath('/dir-00')
  const a = controller.getSnapshot().entries.findIndex((e) => e.name === 'a.txt')
  controller.onRowClick(a, 1)
  await waitFor(controller, (s) => s.preview?.kind === 'text')
  const tree = renderView(controller.getSnapshot(), explorerHandlers(controller))
  expect(getByTestId(tree, 'preview')).toBeTruthy()
  expect(getByTestId(tree, 'preview-text')).toBeTruthy()
  const text = (getByTestId(tree, 'preview-text').props as { children?: string }).children
  expect(text).toContain('hi!')
})

test('default 8 MiB preview cap skips a 9 MiB member', async () => {
  const fake = createFakeNative({
    pickFile: '/archives/hello.tar',
    extraFiles: [{ parent: '/dir-00', name: 'huge.bin', size: NINE_MIB }],
  })
  const controller = new ExplorerController()
  controller.setNative(fake)
  await controller.openPicked()
  await controller.enterPath('/dir-00')
  const huge = controller.getSnapshot().entries.findIndex((e) => e.name === 'huge.bin')
  expect(huge).toBeGreaterThanOrEqual(0)
  controller.onRowClick(huge, 1)
  await waitFor(controller, (s) => s.preview?.kind === 'skipped')
  const preview = controller.getSnapshot().preview
  expect(preview).toEqual({ kind: 'skipped', reason: 'too-large' })
  const tree = renderView(controller.getSnapshot(), explorerHandlers(controller))
  expect(getByTestId(tree, 'preview-skipped')).toBeTruthy()
  expect(getByTestId(tree, 'extract-open-system')).toBeTruthy()
  clickByTestId(tree, 'extract-open-system')
  await waitFor(controller, (s) => s.extractJob?.status === 'succeeded' || s.dialog.kind === 'overwrite')
  expect(fake.extractPlanCalls.length).toBeGreaterThan(0)
  expect(fake.extractCalls.every((c) => c.overwrite !== ('ask' as typeof c.overwrite))).toBe(true)
})

test('progress cancel fires view handler', async () => {
  const fake = createFakeNative({
    pickFile: '/archives/hello.tar',
    extractMode: 'hold',
  })
  const controller = new ExplorerController()
  controller.setNative(fake)
  await controller.openPicked()
  const tree = renderView(controller.getSnapshot(), explorerHandlers(controller))
  clickByTestId(tree, 'extract')
  await waitFor(controller, (s) => s.extractJob?.status === 'running')
  const progress = renderView(controller.getSnapshot(), explorerHandlers(controller))
  clickByTestId(progress, 'progress-cancel')
  await waitFor(controller, (s) => s.extractJob?.status === 'cancelled')
})

test('jobFailed.retryable shows Retry on the progress panel', async () => {
  const fake = createFakeNative({
    pickFile: '/archives/hello.tar',
    extractMode: 'busy',
  })
  const controller = new ExplorerController()
  controller.setNative(fake)
  await controller.openPicked()
  const tree = renderView(controller.getSnapshot(), explorerHandlers(controller))
  clickByTestId(tree, 'extract')
  await waitFor(controller, (s) => s.extractJob?.status === 'failed' && s.extractJob.retryable)
  const progress = renderView(controller.getSnapshot(), explorerHandlers(controller))
  expect(getByTestId(progress, 'progress-retry')).toBeTruthy()
})

test('PathEscape is surfaced and extract is not written', async () => {
  const fake = createFakeNative({
    pickFile: '/archives/hello.tar',
    extractMode: 'path-escape',
  })
  const controller = new ExplorerController()
  controller.setNative(fake)
  await controller.openPicked()
  const tree = renderView(controller.getSnapshot(), explorerHandlers(controller))
  clickByTestId(tree, 'extract')
  await waitFor(controller, (s) => s.dialog.kind === 'path-escape')
  expect(fake.written.size).toBe(0)
  expect(controller.getSnapshot().extractJob?.status).not.toBe('running')
  expect(controller.getSnapshot().extractJob?.status).toBe('failed')
  const dialog = renderView(controller.getSnapshot(), explorerHandlers(controller))
  expect(getByTestId(dialog, 'path-escape')).toBeTruthy()
  expect(queryByTestId(dialog, 'progress-cancel')).toBeNull()
})

test('password modal retries open without storing the secret on the snapshot', async () => {
  const fake = createFakeNative({
    pickFile: '/archives/encrypted.tar',
    openMode: 'bad-password',
  })
  const controller = new ExplorerController()
  controller.setNative(fake)
  const tree = renderView(controller.getSnapshot(), explorerHandlers(controller))
  clickByTestId(tree, 'open')
  await waitFor(controller, (s) => s.dialog.kind === 'password')
  const snap = controller.getSnapshot()
  expect(JSON.stringify(snap)).not.toContain(FAKE_ENCRYPTED_PASSWORD)
  const modal = renderView(snap, explorerHandlers(controller))
  expect(getByTestId(modal, 'password-modal')).toBeTruthy()
  clickByTestId(modal, 'password-submit', { value: FAKE_ENCRYPTED_PASSWORD })
  await waitFor(controller, (s) => s.status === 'ready' && s.entries.length === LIST_LIMIT_DEFAULT)
  expect(JSON.stringify(controller.getSnapshot())).not.toContain(FAKE_ENCRYPTED_PASSWORD)
  expect(fake.openCalls[1]?.password).toBe(FAKE_ENCRYPTED_PASSWORD)
})

test('password cancel does not resubmit the previous secret on Unlock', async () => {
  const fake = createFakeNative({
    pickFile: '/archives/encrypted.tar',
    openMode: 'bad-password',
  })
  const controller = new ExplorerController()
  controller.setNative(fake)
  const tree = renderView(controller.getSnapshot(), explorerHandlers(controller))
  clickByTestId(tree, 'open')
  await waitFor(controller, (s) => s.dialog.kind === 'password')
  const typed = renderView(controller.getSnapshot(), explorerHandlers(controller))
  changeByTestId(typed, 'password-input', FAKE_ENCRYPTED_PASSWORD)
  clickByTestId(typed, 'password-cancel')
  expect(controller.getSnapshot().dialog.kind).toBe('none')
  const idle = renderView(controller.getSnapshot(), explorerHandlers(controller))
  clickByTestId(idle, 'open')
  await waitFor(controller, (s) => s.dialog.kind === 'password')
  const again = renderView(controller.getSnapshot(), explorerHandlers(controller))
  clickByTestId(again, 'password-submit')
  await waitFor(controller, (s) => s.dialog.kind === 'password' || s.status === 'error')
  const last = fake.openCalls[fake.openCalls.length - 1]
  expect(last?.password ?? '').not.toBe(FAKE_ENCRYPTED_PASSWORD)
  expect(JSON.stringify(controller.getSnapshot())).not.toContain(FAKE_ENCRYPTED_PASSWORD)
})

test('Extract is disabled until native is ready', () => {
  const controller = new ExplorerController()
  let extracted = false
  const tree = renderView(controller.getSnapshot(), {
    onExtract: () => {
      extracted = true
    },
  })
  clickByTestId(tree, 'extract')
  expect(extracted).toBe(false)
})

