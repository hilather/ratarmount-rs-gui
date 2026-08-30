import { expect, test } from 'bun:test'
import type { ReactElement } from 'react'

import { ExplorerView, type ExplorerHandlers } from './explorer-view'
import {
  countLabel,
  crumbsFor,
  crumbTestId,
  ExplorerController,
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
  FAKE_MTIME,
  FAKE_ROOT_FILE_COUNT,
  FAKE_ROOT_TOTAL,
} from './fake-native'
import { collectTestIds, getByTestId, queryByTestId } from './gpuix-test'

const noop: ExplorerHandlers = {
  onOpen() {},
  onClose() {},
  onCrumb() {},
  onRowClick() {},
  onKey() {},
  onVisibleRange() {},
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

test('W3 source does not call extract, preview, find, or readAll', async () => {
  const files = ['explorer.ts', 'explorer-view.tsx', 'app.tsx', 'napi.ts', 'native-addon.ts']
  for (const file of files) {
    const source = await Bun.file(new URL(`./${file}`, import.meta.url)).text()
    expect(source).not.toMatch(/\breadAll\s*\(/)
    expect(source).not.toMatch(/\bextract\s*\(/)
    expect(source).not.toMatch(/\bextractPlan\s*\(/)
    expect(source).not.toMatch(/\bpreview\s*\(/)
    expect(source).not.toMatch(/\bfind\s*\(/)
  }
})

test('ExplorerView is a GPUIX host tree (virtual-list + testIds)', async () => {
  const { controller } = await openRoot()
  const tree = renderView(controller.getSnapshot()) as ReactElement
  expect(tree.type).toBe('div')
  const ids = collectTestIds(tree)
  expect(ids).toContain('open')
  expect(ids).toContain('list')
  expect(ids).toContain('crumb-root')
  expect(ids).not.toContain('search')
  expect(ids).not.toContain('preview')
  expect(ids).not.toContain('extract')
})
