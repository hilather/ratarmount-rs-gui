import {
  CommandError,
  commandErrorFromUnknown,
  type Cursor,
  type DirEnt,
  type NativeAddon,
  type OpenResult,
  type SessionId,
} from './napi'

export const LIST_LIMIT_DEFAULT = 200
export const LIST_LIMIT_MAX = 500
export const LIST_NEAR_END = 24

export type ExplorerStatus = 'idle' | 'opening' | 'ready' | 'error'

export type Crumb = {
  testId: string
  label: string
  path: string
}

export type ExplorerSnapshot = {
  status: ExplorerStatus
  listing: boolean
  loadingMore: boolean
  error: string | null
  errorRetryable: boolean
  nativeReady: boolean
  archivePath: string | null
  indexPath: string | null
  path: string
  entries: DirEnt[]
  nextCursor: Cursor | null
  totalHint: number | null
  selectedIndex: number
}

const IDLE: ExplorerSnapshot = {
  status: 'idle',
  listing: false,
  loadingMore: false,
  error: null,
  errorRetryable: false,
  nativeReady: false,
  archivePath: null,
  indexPath: null,
  path: '/',
  entries: [],
  nextCursor: null,
  totalHint: null,
  selectedIndex: 0,
}

export function crumbTestId(path: string): string {
  if (path === '/') {
    return 'crumb-root'
  }
  return `crumb-${path.slice(1).replaceAll('/', '-')}`
}

export function crumbsFor(path: string): Crumb[] {
  const crumbs: Crumb[] = [{ testId: 'crumb-root', label: '/', path: '/' }]
  if (path === '/') {
    return crumbs
  }
  const parts = path.split('/').filter(Boolean)
  let acc = ''
  for (const part of parts) {
    acc += `/${part}`
    crumbs.push({ testId: crumbTestId(acc), label: part, path: acc })
  }
  return crumbs
}

export function parentPath(path: string): string | null {
  if (path === '/') {
    return null
  }
  const idx = path.lastIndexOf('/')
  if (idx <= 0) {
    return '/'
  }
  return path.slice(0, idx)
}

export function siblingIndexPath(archivePath: string): string {
  return `${archivePath}.index.sqlite`
}

export function shortenPath(path: string, maxChars = 42): string {
  if (path.length <= maxChars) {
    return path
  }
  const norm = path.replaceAll('\\', '/')
  const parts = norm.split('/').filter(Boolean)
  if (parts.length >= 2) {
    return `…/${parts[parts.length - 2]}/${parts[parts.length - 1]}`
  }
  return `…${path.slice(-(maxChars - 1))}`
}

export function formatSize(ent: { isDir: boolean; size: number }): string {
  if (ent.isDir) {
    return '—'
  }
  const n = ent.size
  if (n < 1024) {
    return `${n} B`
  }
  if (n < 1024 * 1024) {
    return `${(n / 1024).toFixed(1)} KiB`
  }
  if (n < 1024 * 1024 * 1024) {
    return `${(n / (1024 * 1024)).toFixed(1)} MiB`
  }
  return `${(n / (1024 * 1024 * 1024)).toFixed(1)} GiB`
}

export function formatMtime(mtime: number | null): string {
  if (mtime == null) {
    return '—'
  }
  const d = new Date(mtime * 1000)
  if (Number.isNaN(d.getTime())) {
    return '—'
  }
  const y = d.getUTCFullYear()
  const mo = String(d.getUTCMonth() + 1).padStart(2, '0')
  const day = String(d.getUTCDate()).padStart(2, '0')
  const hh = String(d.getUTCHours()).padStart(2, '0')
  const mm = String(d.getUTCMinutes()).padStart(2, '0')
  return `${y}-${mo}-${day} ${hh}:${mm}`
}

export function countLabel(
  totalHint: number | null,
  loaded: number,
  hasMore: boolean,
): string {
  if (totalHint != null) {
    return totalHint === 1 ? '1 entry' : `${totalHint} entries`
  }
  if (hasMore) {
    return `${loaded}+ entries`
  }
  return loaded === 1 ? '1 entry' : `${loaded} entries`
}

export type ExplorerOptions = {
  listLimit?: number
  nearEnd?: number
}

type Listener = () => void

type JobWaiter = {
  resolve: (sessionId: SessionId) => void
  reject: (err: CommandError) => void
}

export class ExplorerController {
  private native: NativeAddon | null = null
  private sessionId: SessionId | null = null
  private snapshot: ExplorerSnapshot = { ...IDLE }
  private readonly listeners = new Set<Listener>()
  private readonly listLimit: number
  private readonly nearEnd: number
  private gen = 0
  private disposed = false
  private readonly jobWaiters = new Map<number, JobWaiter>()
  private readonly finishedJobs = new Map<number, { sessionId?: SessionId; error?: CommandError }>()
  private listening = false
  private loadNative: (() => Promise<NativeAddon>) | null = null

  constructor(options: ExplorerOptions = {}) {
    const limit = options.listLimit ?? LIST_LIMIT_DEFAULT
    this.listLimit = Math.min(Math.max(1, limit), LIST_LIMIT_MAX)
    this.nearEnd = options.nearEnd ?? LIST_NEAR_END
  }

  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  getSnapshot = (): ExplorerSnapshot => this.snapshot

  setNativeLoader(loader: () => Promise<NativeAddon>): void {
    this.loadNative = loader
  }

  setNative(native: NativeAddon): void {
    this.native = native
    if (!this.listening) {
      native.on('jobSucceeded', (event) => this.onJobSucceeded(event))
      native.on('jobFailed', (event) => this.onJobFailed(event))
      this.listening = true
    }
    const clearLoadError =
      this.snapshot.status === 'error' && this.snapshot.archivePath == null
    this.patch({
      nativeReady: true,
      ...(clearLoadError
        ? { status: 'idle' as const, error: null, errorRetryable: false }
        : {}),
    })
  }

  failLoad(err: unknown): void {
    const ce = commandErrorFromUnknown(err)
    this.patch({
      status: 'error',
      error: ce.message,
      errorRetryable: ce.retryable,
      nativeReady: false,
    })
  }

  dispose(): void {
    this.disposed = true
    const sessionId = this.sessionId
    this.sessionId = null
    if (sessionId != null && this.native) {
      void this.native.close(sessionId)
    }
  }

  async openPicked(): Promise<void> {
    if (this.snapshot.status === 'opening') {
      return
    }
    if (!(await this.ensureNative())) {
      return
    }
    const native = this.native
    if (!native) {
      return
    }
    try {
      const source = await native.pickFile()
      if (source == null) {
        return
      }
      await this.openSource(source)
    } catch (err) {
      this.setError(err)
    }
  }

  async openSource(source: string): Promise<void> {
    const native = this.requireNative()
    if (!native) {
      return
    }
    const gen = ++this.gen
    this.patch({
      status: 'opening',
      listing: true,
      loadingMore: false,
      error: null,
      errorRetryable: false,
      entries: [],
      nextCursor: null,
      totalHint: null,
      selectedIndex: 0,
      path: '/',
      archivePath: source,
      indexPath: siblingIndexPath(source),
    })
    try {
      await this.closeSession()
      if (this.disposed || gen !== this.gen) {
        return
      }
      const outcome = await native.open({
        source,
        policy: 'sibling',
        recreate: 'if-invalid',
      })
      const sessionId = await this.sessionFromOpen(outcome)
      if (this.disposed || gen !== this.gen) {
        return
      }
      this.sessionId = sessionId
      await this.fetchPage({ path: '/', cursor: null, append: false, gen })
    } catch (err) {
      if (this.disposed || gen !== this.gen) {
        return
      }
      this.sessionId = null
      this.setError(err)
    }
  }

  async closeArchive(): Promise<void> {
    const gen = ++this.gen
    try {
      await this.closeSession()
    } catch (err) {
      if (this.disposed || gen !== this.gen) {
        return
      }
      this.setError(err)
      return
    }
    if (this.disposed || gen !== this.gen) {
      return
    }
    this.replace({
      ...IDLE,
      nativeReady: this.native != null,
    })
  }

  async enterPath(path: string): Promise<void> {
    if (this.sessionId == null) {
      return
    }
    if (path === this.snapshot.path && this.snapshot.status === 'ready') {
      return
    }
    const gen = ++this.gen
    this.patch({
      path,
      listing: true,
      loadingMore: false,
      entries: [],
      nextCursor: null,
      selectedIndex: 0,
      error: null,
    })
    try {
      await this.fetchPage({ path, cursor: null, append: false, gen })
    } catch (err) {
      if (this.disposed || gen !== this.gen) {
        return
      }
      this.setError(err)
    }
  }

  async goUp(): Promise<void> {
    const parent = parentPath(this.snapshot.path)
    if (parent == null) {
      return
    }
    await this.enterPath(parent)
  }

  async loadMore(): Promise<void> {
    if (this.snapshot.loadingMore || this.snapshot.listing) {
      return
    }
    const cursor = this.snapshot.nextCursor
    if (cursor === null) {
      return
    }
    const gen = this.gen
    this.patch({ loadingMore: true })
    try {
      await this.fetchPage({
        path: this.snapshot.path,
        cursor,
        append: true,
        gen,
      })
    } catch (err) {
      if (this.disposed || gen !== this.gen) {
        return
      }
      this.patch({ loadingMore: false })
      this.setError(err)
    }
  }

  onVisibleRange(_startIndex: number, endIndex: number): void {
    if (this.snapshot.nextCursor === null || this.snapshot.loadingMore) {
      return
    }
    const remaining = this.snapshot.entries.length - endIndex
    if (remaining <= this.nearEnd) {
      void this.loadMore()
    }
  }

  handleKey(key: string): void {
    if (this.snapshot.status !== 'ready') {
      return
    }
    switch (key) {
      case 'j':
      case 'down':
      case 'arrowdown':
        this.moveSelection(1)
        break
      case 'k':
      case 'up':
      case 'arrowup':
        this.moveSelection(-1)
        break
      case 'enter':
        void this.activateSelection()
        break
      case 'backspace':
        void this.goUp()
        break
      default:
        break
    }
  }

  selectIndex(index: number): void {
    const last = this.snapshot.entries.length - 1
    if (last < 0) {
      return
    }
    const next = Math.max(0, Math.min(index, last))
    if (next !== this.snapshot.selectedIndex) {
      this.patch({ selectedIndex: next })
    }
  }

  onRowClick(index: number, clickCount: number): void {
    this.selectIndex(index)
    if (clickCount >= 2) {
      void this.activateSelection()
    }
  }

  private moveSelection(delta: number): void {
    const last = this.snapshot.entries.length - 1
    if (last < 0) {
      return
    }
    const next = Math.max(0, Math.min(this.snapshot.selectedIndex + delta, last))
    this.patch({ selectedIndex: next })
    if (delta > 0 && last - next <= this.nearEnd) {
      void this.loadMore()
    }
  }

  private async activateSelection(): Promise<void> {
    const ent = this.snapshot.entries[this.snapshot.selectedIndex]
    if (ent?.isDir) {
      await this.enterPath(ent.path)
    }
  }

  private async fetchPage(opts: {
    path: string
    cursor: Cursor | null
    append: boolean
    gen: number
  }): Promise<void> {
    const native = this.native
    const sessionId = this.sessionId
    if (!native || sessionId == null) {
      throw new CommandError('Internal', 'no open session', false)
    }
    const page = await native.list({
      sessionId,
      path: opts.path,
      ...(opts.cursor === null ? {} : { cursor: opts.cursor }),
      limit: this.listLimit,
    })
    if (this.disposed || opts.gen !== this.gen) {
      return
    }
    const entries = opts.append
      ? [...this.snapshot.entries, ...page.entries]
      : page.entries
    this.patch({
      status: 'ready',
      listing: false,
      loadingMore: false,
      error: null,
      errorRetryable: false,
      path: opts.path,
      entries,
      nextCursor: page.nextCursor,
      totalHint: page.totalHint,
      selectedIndex: opts.append
        ? Math.min(this.snapshot.selectedIndex, Math.max(0, entries.length - 1))
        : 0,
    })
  }

  private async sessionFromOpen(outcome: OpenResult): Promise<SessionId> {
    if ('sessionId' in outcome) {
      return outcome.sessionId
    }
    return this.waitForJobSession(outcome.jobId)
  }

  private waitForJobSession(jobId: number): Promise<SessionId> {
    const finished = this.finishedJobs.get(jobId)
    if (finished?.sessionId != null) {
      this.finishedJobs.delete(jobId)
      return Promise.resolve(finished.sessionId)
    }
    if (finished?.error) {
      const err = finished.error
      this.finishedJobs.delete(jobId)
      return Promise.reject(err)
    }
    return new Promise((resolve, reject) => {
      this.jobWaiters.set(jobId, { resolve, reject })
    })
  }

  private onJobSucceeded(event: { jobId: number; sessionId?: SessionId | null }): void {
    const sessionId = event.sessionId ?? null
    if (sessionId == null) {
      const err = new CommandError('Internal', 'jobSucceeded missing sessionId', false)
      const waiter = this.jobWaiters.get(event.jobId)
      if (waiter) {
        this.jobWaiters.delete(event.jobId)
        waiter.reject(err)
        return
      }
      this.finishedJobs.set(event.jobId, { error: err })
      return
    }
    const waiter = this.jobWaiters.get(event.jobId)
    if (waiter) {
      this.jobWaiters.delete(event.jobId)
      waiter.resolve(sessionId)
      return
    }
    this.finishedJobs.set(event.jobId, { sessionId })
  }

  private onJobFailed(event: {
    jobId: number
    code: string
    message: string
    retryable: boolean
  }): void {
    const err = new CommandError(event.code, event.message, event.retryable)
    const waiter = this.jobWaiters.get(event.jobId)
    if (waiter) {
      this.jobWaiters.delete(event.jobId)
      waiter.reject(err)
      return
    }
    this.finishedJobs.set(event.jobId, { error: err })
  }

  private async closeSession(): Promise<void> {
    const sessionId = this.sessionId
    this.sessionId = null
    if (sessionId != null && this.native) {
      await this.native.close(sessionId)
    }
  }

  private async ensureNative(): Promise<boolean> {
    if (this.native) {
      return true
    }
    if (this.loadNative) {
      try {
        this.setNative(await this.loadNative())
        return this.native != null
      } catch (err) {
        this.failLoad(err)
        return false
      }
    }
    this.requireNative()
    return false
  }

  private requireNative(): NativeAddon | null {
    if (this.native) {
      return this.native
    }
    this.setError(
      new CommandError(
        'Internal',
        'Native addon is not built. From native/: bun install && bun run build',
        false,
      ),
    )
    return null
  }

  private setError(err: unknown): void {
    const ce = commandErrorFromUnknown(err)
    this.patch({
      status: 'error',
      listing: false,
      loadingMore: false,
      error: ce.message,
      errorRetryable: ce.retryable,
    })
  }

  private patch(partial: Partial<ExplorerSnapshot>): void {
    this.replace({ ...this.snapshot, ...partial })
  }

  private replace(next: ExplorerSnapshot): void {
    this.snapshot = next
    for (const listener of this.listeners) {
      listener()
    }
  }
}
