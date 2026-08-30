import {
  CommandError,
  type DirEnt,
  type JobFailedEvent,
  type JobSucceededEvent,
  type ListOpts,
  type NativeAddon,
  type OpenOpts,
} from './napi'

export const FAKE_ROOT_DIR_COUNT = 10
export const FAKE_ROOT_FILE_COUNT = 650
export const FAKE_ROOT_TOTAL = FAKE_ROOT_DIR_COUNT + FAKE_ROOT_FILE_COUNT
export const FAKE_MTIME = 1_700_000_000

const LIST_LIMIT_DEFAULT = 200
const LIST_LIMIT_MAX = 500

type JobSucceededListener = (event: JobSucceededEvent) => void
type JobFailedListener = (event: JobFailedEvent) => void

export type FakeNative = NativeAddon & {
  listCalls: ListOpts[]
  openCalls: OpenOpts[]
  closedSessions: number[]
}

export function createFakeNative(
  options: {
    pickFile?: string | null
    openMode?: 'session' | 'job' | 'job-no-session' | 'job-failed'
  } = {},
): FakeNative {
  let nextSession = 1
  let nextJob = 1
  const sessions = new Map<number, string>()
  const catalog = buildCatalog()
  const listCalls: ListOpts[] = []
  const openCalls: OpenOpts[] = []
  const closedSessions: number[] = []
  const succeeded: JobSucceededListener[] = []
  const failed: JobFailedListener[] = []

  const fake: FakeNative = {
    listCalls,
    openCalls,
    closedSessions,
    async pickFile() {
      return options.pickFile === undefined ? '/tmp/hello.tar' : options.pickFile
    },
    async open(opts) {
      openCalls.push(opts)
      const mode = options.openMode ?? (opts.recreate === 'always' ? 'job' : 'session')
      if (mode === 'session') {
        const sessionId = nextSession++
        sessions.set(sessionId, opts.source)
        return { sessionId }
      }
      const jobId = nextJob++
      if (mode === 'job-failed') {
        for (const cb of failed) {
          cb({ jobId, code: 'Internal', message: 'index failed', retryable: false })
        }
        return { jobId }
      }
      if (mode === 'job-no-session') {
        for (const cb of succeeded) {
          cb({ jobId })
        }
        return { jobId }
      }
      const sessionId = nextSession++
      sessions.set(sessionId, opts.source)
      for (const cb of succeeded) {
        cb({ jobId, sessionId })
      }
      return { jobId }
    },
    async close(sessionId) {
      if (!sessions.delete(sessionId)) {
        throw new CommandError('NotFound', `session ${sessionId} is closed`, false)
      }
      closedSessions.push(sessionId)
    },
    async list(opts) {
      listCalls.push(opts)
      if (!sessions.has(opts.sessionId)) {
        throw new CommandError('NotFound', `session ${opts.sessionId} is closed`, false)
      }
      const children = catalog.children.get(opts.path)
      if (!children) {
        throw new CommandError('NotFound', `path not found: ${opts.path}`, false)
      }
      const start = decodeCursor(opts.cursor, opts.path)
      const limit = Math.min(opts.limit ?? LIST_LIMIT_DEFAULT, LIST_LIMIT_MAX)
      const end = Math.min(start + limit, children.length)
      const entries = children.slice(start, end).map((name) => {
        const path = childPath(opts.path, name)
        return catalog.entries.get(path)!
      })
      const nextCursor = end < children.length ? encodeCursor(opts.path, end) : null
      return {
        path: opts.path,
        entries,
        nextCursor,
        totalHint: children.length,
      }
    },
    async lookup(opts) {
      if (!sessions.has(opts.sessionId)) {
        throw new CommandError('NotFound', `session ${opts.sessionId} is closed`, false)
      }
      return catalog.entries.get(opts.path) ?? null
    },
    on(event, cb) {
      if (event === 'jobSucceeded') {
        succeeded.push(cb as JobSucceededListener)
        return
      }
      if (event === 'jobFailed') {
        failed.push(cb as JobFailedListener)
      }
    },
  }
  return fake
}

type Catalog = {
  entries: Map<string, DirEnt>
  children: Map<string, string[]>
}

function buildCatalog(): Catalog {
  const entries = new Map<string, DirEnt>()
  const children = new Map<string, string[]>()

  function addDir(path: string): void {
    entries.set(path, {
      name: path === '/' ? '' : path.slice(path.lastIndexOf('/') + 1),
      path,
      isDir: true,
      size: 0,
      mtime: FAKE_MTIME,
      mode: 0o755,
    })
    if (!children.has(path)) {
      children.set(path, [])
    }
  }

  function addDirChild(parent: string, name: string): void {
    const path = childPath(parent, name)
    addDir(path)
    children.get(parent)!.push(name)
  }

  function addFile(parent: string, name: string, size: number): void {
    const path = childPath(parent, name)
    entries.set(path, {
      name,
      path,
      isDir: false,
      size,
      mtime: FAKE_MTIME,
      mode: 0o644,
    })
    children.get(parent)!.push(name)
  }

  addDir('/')
  for (let i = 0; i < FAKE_ROOT_DIR_COUNT; i++) {
    addDirChild('/', `dir-${String(i).padStart(2, '0')}`)
  }
  for (let i = 0; i < FAKE_ROOT_FILE_COUNT; i++) {
    addFile('/', `file-${String(i).padStart(3, '0')}`, 100 + i)
  }
  for (const name of ['a.txt', 'b.txt', 'c.txt']) {
    addFile('/dir-00', name, 4)
  }
  return { entries, children }
}

function childPath(parent: string, name: string): string {
  return parent === '/' ? `/${name}` : `${parent}/${name}`
}

function encodeCursor(path: string, nextIndex: number): string {
  return `kset:${path}:${nextIndex}`
}

function decodeCursor(cursor: string | undefined, expectedPath: string): number {
  if (cursor == null) {
    return 0
  }
  const rest = cursor.startsWith('kset:') ? cursor.slice('kset:'.length) : ''
  const split = rest.lastIndexOf(':')
  const path = rest.slice(0, split)
  const idx = Number(rest.slice(split + 1))
  if (path !== expectedPath || !Number.isFinite(idx)) {
    throw new CommandError('Internal', 'invalid cursor', false)
  }
  return idx
}
