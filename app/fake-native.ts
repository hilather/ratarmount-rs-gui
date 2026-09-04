import { parseLaunchArgv } from './argv'
import {
  CommandError,
  defaultConfig,
  PREVIEW_CEILING_BYTES,
  RECENT_MAX,
  type Config,
  type ConfigPatch,
  type CacheClearResult,
  type DirEnt,
  type ExtractOpts,
  type ExtractPlan,
  type ExtractProgressEvent,
  type FeatureProbe,
  type FileDropEvent,
  type FindOpts,
  type JobCancelledEvent,
  type JobFailedEvent,
  type JobSucceededEvent,
  type LaunchIntentWire,
  type ListOpts,
  type NativeAddon,
  type OpenOpts,
  type PreviewResult,
} from './napi'

export const FAKE_ROOT_DIR_COUNT = 10
export const FAKE_ROOT_FILE_COUNT = 650
export const FAKE_ROOT_TOTAL = FAKE_ROOT_DIR_COUNT + FAKE_ROOT_FILE_COUNT
export const FAKE_MTIME = 1_700_000_000
export const FAKE_ENCRYPTED_PASSWORD = 'secret'
export const NINE_MIB = 9 * 1024 * 1024

const LIST_LIMIT_DEFAULT = 200
const LIST_LIMIT_MAX = 500
const PREVIEW_DEFAULT = 8 * 1024 * 1024

type JobSucceededListener = (event: JobSucceededEvent) => void
type JobFailedListener = (event: JobFailedEvent) => void
type JobCancelledListener = (event: JobCancelledEvent) => void
type ExtractProgressListener = (event: ExtractProgressEvent) => void
type FileDropListener = (event: FileDropEvent) => void

export type FakeNative = NativeAddon & {
  listCalls: ListOpts[]
  findCalls: FindOpts[]
  openCalls: OpenOpts[]
  closedSessions: number[]
  config: Config
  cacheClears: number
  extractCalls: ExtractOpts[]
  previewCalls: { sessionId: number; path: string }[]
  extractPlanCalls: { sessionId: number; members: string[]; destDir: string }[]
  applyLaunchCalls: string[][]
  registerCalls: number
  unregisterCalls: number
  written: Map<string, Uint8Array>
  extractMode: 'ok' | 'hold' | 'busy' | 'path-escape'
  features: FeatureProbe
  siblingNotWritable: boolean
  fuseMounts: Map<number, string>
  httpUrls: Map<number, string>
  completeExtract(jobId: number): void
  failExtract(jobId: number, code: string, message: string, retryable: boolean): void
  emitExtractProgress(event: ExtractProgressEvent): void
  emitFileDrop(paths: string[]): void
  fileDropWatchStarted: number
}

export function createFakeNative(
  options: {
    pickFile?: string | null
    pickDir?: string | null
    openMode?: 'session' | 'job' | 'job-no-session' | 'job-failed' | 'bad-password'
    extractMode?: 'ok' | 'hold' | 'busy' | 'path-escape'
    extractPlan?: Partial<ExtractPlan>
    config?: Partial<Config>
    extraFiles?: { parent: string; name: string; size: number; body?: string }[]
    rootFileCount?: number
    features?: FeatureProbe
    siblingNotWritable?: boolean
  } = {},
): FakeNative {
  let nextSession = 1
  let nextJob = 1
  const sessions = new Map<number, string>()
  const catalog = buildCatalog(options.extraFiles, options.rootFileCount)
  const listCalls: ListOpts[] = []
  const findCalls: FindOpts[] = []
  const openCalls: OpenOpts[] = []
  const closedSessions: number[] = []
  const extractCalls: ExtractOpts[] = []
  const previewCalls: { sessionId: number; path: string }[] = []
  const extractPlanCalls: { sessionId: number; members: string[]; destDir: string }[] = []
  const applyLaunchCalls: string[][] = []
  let registerCalls = 0
  let unregisterCalls = 0
  const written = new Map<string, Uint8Array>()
  const fuseMounts = new Map<number, string>()
  const httpUrls = new Map<number, string>()
  const succeeded: JobSucceededListener[] = []
  const failed: JobFailedListener[] = []
  const cancelled: JobCancelledListener[] = []
  const extractProgress: ExtractProgressListener[] = []
  const fileDrop: FileDropListener[] = []
  let fileDropWatchStarted = 0
  const heldJobs = new Set<number>()
  let cacheClears = 0
  let features: FeatureProbe = options.features ?? { fuse: false, http: false }
  let siblingNotWritable = options.siblingNotWritable === true
  let config: Config = {
    ...defaultConfig(),
    ...options.config,
    index: {
      ...defaultConfig().index,
      ...options.config?.index,
    },
    extract: {
      overwrite: options.config?.extract?.overwrite ?? 'ask',
      allowUnsafePaths: options.config?.extract?.allowUnsafePaths ?? false,
    },
    preview: {
      maxBytes: options.config?.preview?.maxBytes ?? PREVIEW_DEFAULT,
      openLargeWithSystem: options.config?.preview?.openLargeWithSystem ?? true,
    },
    recent: {
      paths: options.config?.recent?.paths ?? [],
    },
  }

  const fake: FakeNative = {
    listCalls,
    findCalls,
    openCalls,
    closedSessions,
    get config() {
      return config
    },
    get cacheClears() {
      return cacheClears
    },
    extractCalls,
    previewCalls,
    extractPlanCalls,
    applyLaunchCalls,
    get registerCalls() {
      return registerCalls
    },
    get unregisterCalls() {
      return unregisterCalls
    },
    written,
    extractMode: options.extractMode ?? 'ok',
    get features() {
      return features
    },
    set features(next: FeatureProbe) {
      features = next
    },
    get siblingNotWritable() {
      return siblingNotWritable
    },
    set siblingNotWritable(next: boolean) {
      siblingNotWritable = next
    },
    fuseMounts,
    httpUrls,
    completeExtract(jobId: number) {
      heldJobs.delete(jobId)
      for (const cb of extractProgress) {
        cb({ jobId, filesDone: 1, filesHint: 1, bytesOut: 4, current: '/dir-00/a.txt' })
      }
      for (const cb of succeeded) {
        cb({ jobId })
      }
    },
    failExtract(jobId: number, code: string, message: string, retryable: boolean) {
      heldJobs.delete(jobId)
      for (const cb of failed) {
        cb({ jobId, code, message, retryable })
      }
    },
    emitExtractProgress(event: ExtractProgressEvent) {
      for (const cb of extractProgress) {
        cb(event)
      }
    },
    emitFileDrop(paths: string[]) {
      for (const cb of fileDrop) {
        cb({ paths })
      }
    },
    get fileDropWatchStarted() {
      return fileDropWatchStarted
    },
    async pickFile() {
      return options.pickFile === undefined ? '/tmp/hello.tar' : options.pickFile
    },
    async pickDir() {
      return options.pickDir === undefined ? '/tmp/out' : options.pickDir
    },
    async open(opts) {
      openCalls.push(opts)
      if (siblingNotWritable && opts.policy === 'sibling') {
        throw new CommandError(
          'SiblingNotWritable',
          'The directory next to the archive is not writable',
          true,
        )
      }
      const mode = options.openMode ?? (opts.recreate === 'always' ? 'job' : 'session')
      if (mode === 'bad-password') {
        if (opts.password !== FAKE_ENCRYPTED_PASSWORD) {
          throw new CommandError('BadPassword', 'incorrect password', false)
        }
      }
      if (mode === 'session' || mode === 'bad-password') {
        const sessionId = nextSession++
        sessions.set(sessionId, opts.source)
        rememberRecent(opts.source)
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
      rememberRecent(opts.source)
      for (const cb of succeeded) {
        cb({ jobId, sessionId })
      }
      return { jobId }
    },
    async close(sessionId) {
      if (!sessions.delete(sessionId)) {
        throw new CommandError('NotFound', `session ${sessionId} is closed`, false)
      }
      fuseMounts.delete(sessionId)
      httpUrls.delete(sessionId)
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
    async find(opts) {
      findCalls.push(opts)
      if (!sessions.has(opts.sessionId)) {
        throw new CommandError('NotFound', `session ${opts.sessionId} is closed`, false)
      }
      const mode = opts.mode === 'glob' ? 'glob' : 'fts'
      const key = `${opts.pattern}|${mode}`
      const start = decodeCursor(opts.cursor, key)
      const limit = Math.min(opts.limit ?? LIST_LIMIT_DEFAULT, LIST_LIMIT_MAX)
      const page = catalog.findPage(opts.pattern, mode, start, limit)
      return {
        pattern: opts.pattern,
        mode,
        entries: page.entries,
        nextCursor: page.nextIndex == null ? null : encodeCursor(key, page.nextIndex),
        totalHint: page.total,
      }
    },
    async preview(opts) {
      previewCalls.push(opts)
      if (!sessions.has(opts.sessionId)) {
        throw new CommandError('NotFound', `session ${opts.sessionId} is closed`, false)
      }
      const ent = catalog.entries.get(opts.path)
      if (!ent) {
        throw new CommandError('NotFound', `path not found: ${opts.path}`, false)
      }
      if (ent.isDir) {
        return { kind: 'skipped', reason: 'unknown' } satisfies PreviewResult
      }
      if (ent.size > config.preview.maxBytes) {
        return { kind: 'skipped', reason: 'too-large' }
      }
      const body = catalog.bodies.get(opts.path)
      if (body == null) {
        return { kind: 'skipped', reason: 'unknown' }
      }
      return { kind: 'text', text: body, truncated: false }
    },
    async extractPlan(opts) {
      extractPlanCalls.push(opts)
      if (!sessions.has(opts.sessionId)) {
        throw new CommandError('NotFound', `session ${opts.sessionId} is closed`, false)
      }
      if (options.extractPlan) {
        const conflicts = (options.extractPlan.conflicts ?? []).slice(0, 50)
        return {
          files: options.extractPlan.files ?? 0,
          bytes: options.extractPlan.bytes ?? 0,
          conflictCount: options.extractPlan.conflictCount ?? 0,
          conflicts,
          conflictsTruncated: options.extractPlan.conflictsTruncated ?? conflicts.length < (options.extractPlan.conflictCount ?? 0),
        }
      }
      const files = extractFiles(catalog, opts.members)
      return {
        files: files.length,
        bytes: files.reduce((n, e) => n + e.size, 0),
        conflictCount: 0,
        conflicts: [],
        conflictsTruncated: false,
      }
    },
    async extract(opts) {
      extractCalls.push(opts)
      if (opts.overwrite !== 'skip' && opts.overwrite !== 'replace') {
        throw new CommandError(
          'Internal',
          "extract overwrite 'ask' is UI-only; pass 'skip' or 'replace'",
          false,
        )
      }
      if (!sessions.has(opts.sessionId)) {
        throw new CommandError('NotFound', `session ${opts.sessionId} is closed`, false)
      }
      for (const member of opts.members) {
        if (member.split('/').includes('..') && !config.extract.allowUnsafePaths) {
          throw new CommandError('PathEscape', 'path escape', false)
        }
      }
      const jobId = nextJob++
      const mode = fake.extractMode
      if (mode === 'path-escape') {
        throw new CommandError('PathEscape', 'path escape', false)
      }
      if (mode === 'busy') {
        queueMicrotask(() => {
          for (const cb of failed) {
            cb({ jobId, code: 'Busy', message: 'destination is busy', retryable: true })
          }
        })
        return { jobId }
      }
      if (mode === 'hold') {
        heldJobs.add(jobId)
        queueMicrotask(() => {
          for (const cb of extractProgress) {
            cb({ jobId, filesDone: 0, filesHint: 1, bytesOut: 0, current: null })
          }
        })
        return { jobId }
      }
      const files = extractFiles(catalog, opts.members)
      for (const ent of files) {
        const rel = ent.path.replace(/^\//, '')
        const dest = `${opts.destDir.replace(/\/$/, '')}/${rel}`
        const body = catalog.bodies.get(ent.path) ?? `rgui-fake:${ent.path}\n`
        if (opts.overwrite === 'skip' && written.has(dest)) {
          continue
        }
        written.set(dest, new TextEncoder().encode(body))
      }
      queueMicrotask(() => {
        for (const cb of extractProgress) {
          cb({
            jobId,
            filesDone: files.length,
            filesHint: files.length,
            bytesOut: files.reduce((n, e) => n + e.size, 0),
            current: files[0]?.path ?? null,
          })
        }
        for (const cb of succeeded) {
          cb({ jobId })
        }
      })
      return { jobId }
    },
    async cancel(jobId) {
      heldJobs.delete(jobId)
      for (const cb of cancelled) {
        cb({ jobId })
      }
    },
    async getConfig() {
      return config
    },
    async setConfig(patch: ConfigPatch) {
      if (patch.index?.policy === 'memory') {
        throw new CommandError('Internal', "config.index.policy cannot be 'memory'", false)
      }
      if (patch.extract?.overwrite != null) {
        config.extract.overwrite = patch.extract.overwrite
      }
      if (patch.extract?.allowUnsafePaths != null) {
        config.extract.allowUnsafePaths = patch.extract.allowUnsafePaths
      }
      if (patch.preview?.maxBytes != null) {
        config.preview.maxBytes = Math.min(patch.preview.maxBytes, PREVIEW_CEILING_BYTES)
      }
      if (patch.preview?.openLargeWithSystem != null) {
        config.preview.openLargeWithSystem = patch.preview.openLargeWithSystem
      }
      if (patch.index) {
        config = {
          ...config,
          index: {
            ...config.index,
            ...patch.index,
            policy: patch.index.policy ?? config.index.policy,
          },
        }
      }
      if (patch.recent?.paths) {
        config = {
          ...config,
          recent: { paths: patch.recent.paths.filter((p) => p.length > 0).slice(0, RECENT_MAX) },
        }
      }
      if (patch.engine) {
        config = { ...config, engine: { ...config.engine, ...patch.engine } }
      }
      return config
    },
    async clearLocalIndexCache(): Promise<CacheClearResult> {
      cacheClears += 1
      return { removed: 0 }
    },
    async parseArgv(args: string[]): Promise<LaunchIntentWire> {
      const intent = parseLaunchArgv(args)
      if (intent.action.kind === 'extract-to') {
        return {
          action: 'extract-to',
          destDir: intent.action.destDir,
          archives: intent.archives,
          silent: intent.silent,
        }
      }
      return {
        action: intent.action.kind,
        destDir: null,
        archives: intent.archives,
        silent: intent.silent,
      }
    },
    async applyLaunch(args: string[]) {
      applyLaunchCalls.push(args)
      const intent = parseLaunchArgv(args)
      if (intent.action.kind === 'extract-to' && intent.action.destDir == null && intent.silent) {
        throw new CommandError(
          'Internal',
          'extract-to destination omitted; folder picker required',
          false,
        )
      }
    },
    async registerAssociations() {
      registerCalls += 1
    },
    async unregisterAssociations() {
      unregisterCalls += 1
    },
    async probeFeatures() {
      return features
    },
    async fuseMount(sessionId) {
      if (!sessions.has(sessionId)) {
        throw new CommandError('NotFound', `session ${sessionId} is closed`, false)
      }
      if (!features.fuse) {
        return { error: 'FUSE is not available' }
      }
      const existing = fuseMounts.get(sessionId)
      if (existing) {
        return { mountpoint: existing }
      }
      const mountpoint = `/tmp/rgui-fuse-${sessionId}`
      fuseMounts.set(sessionId, mountpoint)
      return { mountpoint }
    },
    async fuseUnmount(sessionId) {
      if (!sessions.has(sessionId)) {
        throw new CommandError('NotFound', `session ${sessionId} is closed`, false)
      }
      fuseMounts.delete(sessionId)
    },
    async httpStart(sessionId) {
      if (!sessions.has(sessionId)) {
        throw new CommandError('NotFound', `session ${sessionId} is closed`, false)
      }
      if (!features.http) {
        throw new CommandError('UnsupportedFormat', 'HTTP share is not available', false)
      }
      const existing = httpUrls.get(sessionId)
      if (existing) {
        return { url: existing }
      }
      const url = `http://127.0.0.1:${18754 + sessionId}/`
      httpUrls.set(sessionId, url)
      return { url }
    },
    async httpStop(sessionId) {
      if (!sessions.has(sessionId)) {
        throw new CommandError('NotFound', `session ${sessionId} is closed`, false)
      }
      httpUrls.delete(sessionId)
    },
    async startFileDropWatch() {
      fileDropWatchStarted += 1
    },
    on(event, cb) {
      if (event === 'jobSucceeded') {
        succeeded.push(cb as JobSucceededListener)
        return
      }
      if (event === 'jobFailed') {
        failed.push(cb as JobFailedListener)
        return
      }
      if (event === 'jobCancelled') {
        cancelled.push(cb as JobCancelledListener)
        return
      }
      if (event === 'extractProgress') {
        extractProgress.push(cb as ExtractProgressListener)
        return
      }
      if (event === 'fileDrop') {
        fileDrop.push(cb as FileDropListener)
      }
    },
  }
  function rememberRecent(source: string): void {
    const paths = config.recent.paths.filter((p) => p !== source && p.length > 0)
    paths.unshift(source)
    config = { ...config, recent: { paths: paths.slice(0, RECENT_MAX) } }
  }

  return fake
}

type Catalog = {
  entries: Map<string, DirEnt>
  children: Map<string, string[]>
  bodies: Map<string, string>
  findPage(
    pattern: string,
    mode: 'glob' | 'fts',
    start: number,
    limit: number,
  ): { entries: DirEnt[]; nextIndex: number | null; total: number }
}

function extractFiles(catalog: Catalog, members: string[]): DirEnt[] {
  if (members.length === 0) {
    return [...catalog.entries.values()].filter((e) => !e.isDir)
  }
  const out: DirEnt[] = []
  const seen = new Set<string>()
  for (const member of members) {
    const ent = catalog.entries.get(member)
    if (!ent) {
      continue
    }
    if (ent.isDir) {
      for (const child of catalog.entries.values()) {
        if (!child.isDir && child.path.startsWith(`${ent.path}/`) && !seen.has(child.path)) {
          seen.add(child.path)
          out.push(child)
        }
      }
    } else if (!seen.has(ent.path)) {
      seen.add(ent.path)
      out.push(ent)
    }
  }
  return out
}

function buildCatalog(
  extraFiles: { parent: string; name: string; size: number; body?: string }[] = [],
  rootFileCount = FAKE_ROOT_FILE_COUNT,
): Catalog {
  const entries = new Map<string, DirEnt>()
  const children = new Map<string, string[]>()
  const bodies = new Map<string, string>()

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

  function addFile(parent: string, name: string, size: number, body?: string): void {
    const path = childPath(parent, name)
    entries.set(path, {
      name,
      path,
      isDir: false,
      size,
      mtime: FAKE_MTIME,
      mode: 0o644,
    })
    if (body != null) {
      bodies.set(path, body)
    }
    children.get(parent)!.push(name)
  }

  addDir('/')
  for (let i = 0; i < FAKE_ROOT_DIR_COUNT; i++) {
    addDirChild('/', `dir-${String(i).padStart(2, '0')}`)
  }
  const filePad = rootFileCount >= 1000 ? 6 : 3
  for (let i = 0; i < rootFileCount; i++) {
    addFile('/', `file-${String(i).padStart(filePad, '0')}`, 100 + i)
  }
  addFile('/dir-00', 'a.txt', 4, 'hi!\n')
  addFile('/dir-00', 'b.txt', 4, 'bb!\n')
  addFile('/dir-00', 'c.txt', 4, 'cc!\n')
  for (const extra of extraFiles) {
    addFile(extra.parent, extra.name, extra.size, extra.body)
  }
  return {
    entries,
    children,
    bodies,
    findPage(pattern, mode, start, limit) {
      const out: DirEnt[] = []
      let matched = 0
      let nextIndex: number | null = null
      for (const ent of entries.values()) {
        if (ent.path === '/') {
          continue
        }
        const hit =
          mode === 'glob'
            ? globMatch(pattern, ent.name) || globMatch(pattern, ent.path)
            : ent.name.toLowerCase().includes(pattern.toLowerCase()) ||
              ent.path.toLowerCase().includes(pattern.toLowerCase())
        if (!hit) {
          continue
        }
        if (matched >= start && out.length < limit) {
          out.push(ent)
        } else if (matched >= start + limit && nextIndex == null) {
          nextIndex = start + limit
        }
        matched += 1
      }
      return { entries: out, nextIndex, total: matched }
    },
  }
}

function globMatch(pattern: string, text: string): boolean {
  return globRec(pattern, text)
}

function globRec(pat: string, text: string): boolean {
  if (pat.length === 0) {
    return text.length === 0
  }
  if (pat[0] === '*') {
    return globRec(pat.slice(1), text) || (text.length > 0 && globRec(pat, text.slice(1)))
  }
  if (pat[0] === '?') {
    return text.length > 0 && globRec(pat.slice(1), text.slice(1))
  }
  return text[0] === pat[0] && globRec(pat.slice(1), text.slice(1))
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
