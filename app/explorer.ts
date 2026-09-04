import {
  CommandError,
  commandErrorFromUnknown,
  type Cursor,
  type DirEnt,
  type ExtractConflict,
  type ExtractPlan,
  type IndexPolicy,
  type NativeAddon,
  type NativeOverwrite,
  type OpenResult,
  type PersistablePolicy,
  type PreviewResult,
  type SessionId,
} from './napi'
import {
  effectiveOpenPolicy,
  hideMemoryPolicy,
  indexLocationHint,
  volumeKeyForSource,
} from './settings'

export const LIST_LIMIT_DEFAULT = 200
export const LIST_LIMIT_MAX = 500
export const LIST_NEAR_END = 24
export const EXTRACT_CONFIRM_FILES = 1000
export const EXTRACT_CONFIRM_BYTES = 1024 * 1024 * 1024
export const EXTRACT_PLAN_CONFLICT_SAMPLE = 50

export type ExplorerStatus = 'idle' | 'opening' | 'ready' | 'error'

export type ClickMods = {
  shift?: boolean
  ctrl?: boolean
  cmd?: boolean
}

export type ExtractJobView = {
  jobId: number
  filesDone: number
  filesHint: number | null
  bytesOut: number
  current: string | null
  status: 'running' | 'succeeded' | 'failed' | 'cancelled'
  error: string | null
  errorCode: string | null
  retryable: boolean
}

export type ExplorerDialog =
  | { kind: 'none' }
  | { kind: 'confirm-extract'; destDir: string; members: string[]; files: number; bytes: number }
  | {
      kind: 'overwrite'
      destDir: string
      members: string[]
      conflictCount: number
      conflicts: ExtractConflict[]
      truncated: boolean
    }
  | { kind: 'password' }
  | { kind: 'path-escape'; message: string }

export type Crumb = {
  testId: string
  label: string
  path: string
}

export type SiblingDialog = {
  source: string
  remember: boolean
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
  policy: IndexPolicy | null
  siblingDialog: SiblingDialog | null
  path: string
  entries: DirEnt[]
  nextCursor: Cursor | null
  totalHint: number | null
  selectedIndex: number
  selectedPaths: string[]
  preview: PreviewResult | null
  previewPath: string | null
  extractJob: ExtractJobView | null
  dialog: ExplorerDialog
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
  policy: null,
  siblingDialog: null,
  path: '/',
  entries: [],
  nextCursor: null,
  totalHint: null,
  selectedIndex: 0,
  selectedPaths: [],
  preview: null,
  previewPath: null,
  extractJob: null,
  dialog: { kind: 'none' },
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
  private extractJobId: number | null = null
  private extractInFlight = false
  private selectAnchor = 0
  private lastExtract: {
    destDir: string
    members: string[]
    overwrite: NativeOverwrite
  } | null = null
  private openAfterExtract: { jobId: number | null; destDir: string; member: string } | null =
    null
  private systemOpens: string[] = []

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

  openedWithSystem(): readonly string[] {
    return this.systemOpens
  }

  setNativeLoader(loader: () => Promise<NativeAddon>): void {
    this.loadNative = loader
  }

  setNative(native: NativeAddon): void {
    this.native = native
    if (!this.listening) {
      native.on('jobSucceeded', (event) => this.onJobSucceeded(event))
      native.on('jobFailed', (event) => this.onJobFailed(event))
      native.on('jobCancelled', (event) => this.onJobCancelled(event))
      native.on('extractProgress', (event) => this.onExtractProgress(event))
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

  async openSource(source: string, password?: string, policyOverride?: PersistablePolicy): Promise<void> {
    const native = this.requireNative()
    if (!native) {
      return
    }
    const gen = ++this.gen
    let policy: PersistablePolicy = policyOverride ?? 'sibling'
    let recreate: 'never' | 'if-invalid' | 'always' = 'if-invalid'
    let explicitPath: string | undefined
    try {
      const cfg = await native.getConfig()
      policy = policyOverride ?? hideMemoryPolicy(cfg.index.policy)
      if (policyOverride == null) {
        policy = effectiveOpenPolicy(
          policy,
          source,
          cfg.index.rememberedVolumes,
          cfg.index.rememberUnwritableVolumes,
        )
      }
      recreate = cfg.index.recreate
      explicitPath = cfg.index.explicitPath || undefined
    } catch {
      // Keep sibling / if-invalid if config is unavailable.
    }
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
      selectedPaths: [],
      preview: null,
      previewPath: null,
      path: '/',
      archivePath: source,
      policy,
      siblingDialog: null,
      indexPath: indexLocationHint(policy, source, explicitPath),
      dialog: { kind: 'none' },
    })
    try {
      await this.closeSession()
      if (this.disposed || gen !== this.gen) {
        return
      }
      const outcome = await native.open({
        source,
        policy,
        recreate,
        ...(explicitPath ? { explicitPath } : {}),
        ...(password === undefined ? {} : { password }),
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
      const ce = commandErrorFromUnknown(err)
      if (ce.code === 'BadPassword') {
        this.patch({
          status: 'error',
          listing: false,
          loadingMore: false,
          error: ce.message,
          errorRetryable: false,
          dialog: { kind: 'password' },
        })
        return
      }
      if (ce.code === 'SiblingNotWritable') {
        this.patch({
          status: 'error',
          listing: false,
          loadingMore: false,
          error: ce.message,
          errorRetryable: true,
          siblingDialog: { source, remember: false },
        })
        return
      }
      this.setError(err)
    }
  }

  toggleSiblingRemember(): void {
    const dialog = this.snapshot.siblingDialog
    if (!dialog) {
      return
    }
    this.patch({ siblingDialog: { ...dialog, remember: !dialog.remember } })
  }

  async confirmSiblingUseCache(remember = this.snapshot.siblingDialog?.remember ?? false): Promise<void> {
    const dialog = this.snapshot.siblingDialog
    const native = this.native
    if (!dialog || !native) {
      return
    }
    if (remember) {
      try {
        const cfg = await native.getConfig()
        const volume = volumeKeyForSource(dialog.source)
        const volumes = cfg.index.rememberedVolumes.includes(volume)
          ? cfg.index.rememberedVolumes
          : [...cfg.index.rememberedVolumes, volume]
        await native.setConfig({
          index: { rememberUnwritableVolumes: true, rememberedVolumes: volumes },
        })
      } catch (err) {
        this.setError(err)
        return
      }
    }
    this.patch({ siblingDialog: null })
    await this.openSource(dialog.source, undefined, 'user-cache')
  }

  cancelSiblingDialog(): void {
    this.patch({ siblingDialog: null })
  }

  async submitPassword(password: string): Promise<void> {
    const source = this.snapshot.archivePath
    this.patch({ dialog: { kind: 'none' } })
    if (source == null) {
      return
    }
    await this.openSource(source, password)
  }

  dismissDialog(): void {
    this.openAfterExtract = null
    this.patch({ dialog: { kind: 'none' } })
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
      selectedPaths: [],
      preview: null,
      previewPath: null,
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

  handleKey(key: string, mods: ClickMods = {}): void {
    if (this.snapshot.dialog.kind !== 'none') {
      if (key === 'escape') {
        this.dismissDialog()
      }
      return
    }
    if (this.snapshot.status !== 'ready') {
      return
    }
    switch (key) {
      case 'j':
      case 'down':
      case 'arrowdown':
        this.moveSelection(1, mods)
        break
      case 'k':
      case 'up':
      case 'arrowup':
        this.moveSelection(-1, mods)
        break
      case ' ':
      case 'space':
        this.toggleSelection(this.snapshot.selectedIndex)
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
    const path = this.snapshot.entries[next]?.path
    this.selectAnchor = next
    this.patch({
      selectedIndex: next,
      selectedPaths: path ? [path] : [],
    })
    this.queuePreview()
  }

  onRowClick(index: number, clickCount: number, mods: ClickMods = {}): void {
    const last = this.snapshot.entries.length - 1
    if (last < 0) {
      return
    }
    const next = Math.max(0, Math.min(index, last))
    if (mods.shift) {
      this.selectRange(this.selectAnchor, next)
    } else if (mods.ctrl || mods.cmd) {
      this.toggleSelection(next)
    } else {
      this.selectIndex(next)
    }
    if (clickCount >= 2 && !mods.ctrl && !mods.cmd && !mods.shift) {
      void this.activateSelection()
    }
  }

  private toggleSelection(index: number): void {
    const ent = this.snapshot.entries[index]
    if (!ent) {
      return
    }
    const selected = new Set(this.snapshot.selectedPaths)
    if (selected.has(ent.path)) {
      selected.delete(ent.path)
    } else {
      selected.add(ent.path)
    }
    this.selectAnchor = index
    this.patch({
      selectedIndex: index,
      selectedPaths: [...selected],
    })
    this.queuePreview()
  }

  private selectRange(from: number, to: number): void {
    const start = Math.min(from, to)
    const end = Math.max(from, to)
    const paths = this.snapshot.entries.slice(start, end + 1).map((e) => e.path)
    this.patch({
      selectedIndex: to,
      selectedPaths: paths,
    })
    this.queuePreview()
  }

  private moveSelection(delta: number, mods: ClickMods = {}): void {
    const last = this.snapshot.entries.length - 1
    if (last < 0) {
      return
    }
    const next = Math.max(0, Math.min(this.snapshot.selectedIndex + delta, last))
    if (mods.shift) {
      this.selectRange(this.selectAnchor, next)
    } else {
      this.selectIndex(next)
    }
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
    const selectedIndex = opts.append
      ? Math.min(this.snapshot.selectedIndex, Math.max(0, entries.length - 1))
      : 0
    const selectedPaths = opts.append
      ? this.snapshot.selectedPaths
      : entries[selectedIndex]?.path
        ? [entries[selectedIndex].path]
        : []
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
      selectedIndex,
      selectedPaths,
    })
    if (!opts.append) {
      this.selectAnchor = 0
      this.queuePreview()
    }
  }

  async extractTo(): Promise<void> {
    if (this.snapshot.status === 'opening') {
      return
    }
    if (!(await this.ensureNative())) {
      return
    }
    const native = this.native
    if (!native || this.sessionId == null || this.snapshot.archivePath == null) {
      return
    }
    if (!this.snapshot.nativeReady) {
      return
    }
    try {
      const destDir = await native.pickDir()
      if (destDir == null) {
        return
      }
      await this.planAndExtract(destDir, this.selectedMembers())
    } catch (err) {
      this.handleExtractError(err)
    }
  }

  async extractAllTo(): Promise<void> {
    if (this.snapshot.status === 'opening') {
      return
    }
    if (!(await this.ensureNative())) {
      return
    }
    const native = this.native
    if (!native || this.sessionId == null || this.snapshot.archivePath == null) {
      return
    }
    if (!this.snapshot.nativeReady) {
      return
    }
    try {
      const destDir = await native.pickDir()
      if (destDir == null) {
        return
      }
      await this.planAndExtract(destDir, [])
    } catch (err) {
      this.handleExtractError(err)
    }
  }

  async extractOpenWithSystem(): Promise<void> {
    const path = this.snapshot.previewPath
    if (path == null) {
      return
    }
    if (!(await this.ensureNative())) {
      return
    }
    const native = this.native
    if (!native || this.sessionId == null) {
      return
    }
    try {
      const destDir = await native.pickDir()
      if (destDir == null) {
        return
      }
      this.openAfterExtract = { jobId: null, destDir, member: path }
      await this.planAndExtract(destDir, [path])
    } catch (err) {
      this.openAfterExtract = null
      this.handleExtractError(err)
    }
  }

  async confirmExtract(): Promise<void> {
    const dialog = this.snapshot.dialog
    if (dialog.kind !== 'confirm-extract') {
      return
    }
    const { destDir, members } = dialog
    this.patch({ dialog: { kind: 'none' } })
    try {
      const plan = await this.requirePlan(destDir, members)
      await this.continueAfterPlan(destDir, members, plan)
    } catch (err) {
      this.handleExtractError(err)
    }
  }

  async chooseOverwrite(overwrite: NativeOverwrite): Promise<void> {
    const dialog = this.snapshot.dialog
    if (dialog.kind !== 'overwrite') {
      return
    }
    const { destDir, members } = dialog
    this.patch({ dialog: { kind: 'none' } })
    try {
      await this.startExtract(destDir, members, overwrite)
    } catch (err) {
      this.handleExtractError(err)
    }
  }

  async cancelExtract(): Promise<void> {
    const jobId = this.extractJobId
    if (jobId == null || !this.native) {
      return
    }
    try {
      await this.native.cancel(jobId)
    } catch (err) {
      this.setError(err)
    }
  }

  async retryExtract(): Promise<void> {
    const last = this.lastExtract
    const job = this.snapshot.extractJob
    if (last == null || job == null || !job.retryable) {
      return
    }
    try {
      await this.startExtract(last.destDir, last.members, last.overwrite)
    } catch (err) {
      this.handleExtractError(err)
    }
  }

  private selectedMembers(): string[] {
    if (this.snapshot.selectedPaths.length > 0) {
      return this.snapshot.selectedPaths
    }
    const ent = this.snapshot.entries[this.snapshot.selectedIndex]
    return ent ? [ent.path] : []
  }

  private async planAndExtract(destDir: string, members: string[]): Promise<void> {
    const plan = await this.requirePlan(destDir, members)
    const extractAll = members.length === 0
    if (extractAll && (plan.files > EXTRACT_CONFIRM_FILES || plan.bytes > EXTRACT_CONFIRM_BYTES)) {
      this.patch({
        dialog: {
          kind: 'confirm-extract',
          destDir,
          members,
          files: plan.files,
          bytes: plan.bytes,
        },
      })
      return
    }
    await this.continueAfterPlan(destDir, members, plan)
  }

  private async continueAfterPlan(
    destDir: string,
    members: string[],
    plan: ExtractPlan,
  ): Promise<void> {
    const native = this.native
    if (!native) {
      return
    }
    const cfg = await native.getConfig()
    if (cfg.extract.overwrite === 'ask' && plan.conflictCount > 0) {
      this.patch({
        dialog: {
          kind: 'overwrite',
          destDir,
          members,
          conflictCount: plan.conflictCount,
          conflicts: plan.conflicts.slice(0, EXTRACT_PLAN_CONFLICT_SAMPLE),
          truncated: plan.conflictsTruncated,
        },
      })
      return
    }
    const overwrite: NativeOverwrite =
      cfg.extract.overwrite === 'replace' ? 'replace' : 'skip'
    await this.startExtract(destDir, members, overwrite)
  }

  private async requirePlan(destDir: string, members: string[]): Promise<ExtractPlan> {
    const native = this.native
    const sessionId = this.sessionId
    if (!native || sessionId == null) {
      throw new CommandError('Internal', 'no open session', false)
    }
    return native.extractPlan({ sessionId, members, destDir })
  }

  private async startExtract(
    destDir: string,
    members: string[],
    overwrite: NativeOverwrite,
  ): Promise<void> {
    const native = this.native
    const sessionId = this.sessionId
    if (!native || sessionId == null) {
      throw new CommandError('Internal', 'no open session', false)
    }
    if (overwrite !== 'skip' && overwrite !== 'replace') {
      throw new CommandError(
        'Internal',
        "extract overwrite 'ask' is UI-only; pass 'skip' or 'replace'",
        false,
      )
    }
    this.lastExtract = { destDir, members, overwrite }
    this.extractInFlight = true
    this.patch({
      extractJob: {
        jobId: this.extractJobId ?? 0,
        filesDone: 0,
        filesHint: null,
        bytesOut: 0,
        current: null,
        status: 'running',
        error: null,
        errorCode: null,
        retryable: false,
      },
    })
    let jobId: number
    try {
      ;({ jobId } = await native.extract({
        sessionId,
        members,
        destDir,
        overwrite,
      }))
    } catch (err) {
      this.applyExtractFailed(commandErrorFromUnknown(err))
      throw err
    }
    this.extractJobId = jobId
    if (this.openAfterExtract && this.openAfterExtract.jobId == null) {
      this.openAfterExtract = { ...this.openAfterExtract, jobId }
    }
    const finished = this.finishedJobs.get(jobId)
    if (finished) {
      this.finishedJobs.delete(jobId)
      if (finished.error) {
        this.applyExtractFailed(finished.error)
        return
      }
      this.extractInFlight = false
      this.patch({
        extractJob: {
          jobId,
          filesDone: this.snapshot.extractJob?.filesDone ?? 0,
          filesHint: this.snapshot.extractJob?.filesHint ?? null,
          bytesOut: this.snapshot.extractJob?.bytesOut ?? 0,
          current: this.snapshot.extractJob?.current ?? null,
          status: 'succeeded',
          error: null,
          errorCode: null,
          retryable: false,
        },
      })
      void this.maybeOpenExtracted(jobId)
      return
    }
    const current = this.snapshot.extractJob
    if (current) {
      this.patch({ extractJob: { ...current, jobId } })
    }
    if (this.snapshot.extractJob?.status === 'succeeded') {
      void this.maybeOpenExtracted(jobId)
    }
  }

  private async maybeOpenExtracted(jobId: number): Promise<void> {
    const pending = this.openAfterExtract
    if (pending == null || pending.jobId !== jobId) {
      return
    }
    this.openAfterExtract = null
    if (!this.native) {
      return
    }
    try {
      const cfg = await this.native.getConfig()
      if (!cfg.preview.openLargeWithSystem) {
        return
      }
      const rel = pending.member.replace(/^\//, '')
      const dest = `${pending.destDir.replace(/\/$/, '')}/${rel}`
      this.systemOpens.push(dest)
      spawnOpenWithSystem(dest)
    } catch {
      // Extract already succeeded.
    }
  }

  private applyExtractFailed(err: CommandError): void {
    this.openAfterExtract = null
    this.extractInFlight = false
    const current = this.snapshot.extractJob
    this.patch({
      extractJob: {
        jobId: current?.jobId ?? 0,
        filesDone: current?.filesDone ?? 0,
        filesHint: current?.filesHint ?? null,
        bytesOut: current?.bytesOut ?? 0,
        current: current?.current ?? null,
        status: 'failed',
        error: err.message,
        errorCode: err.code,
        retryable: err.retryable,
      },
      ...(err.code === 'PathEscape'
        ? { dialog: { kind: 'path-escape' as const, message: err.message } }
        : {}),
    })
  }

  private queuePreview(): void {
    const paths = this.snapshot.selectedPaths
    if (paths.length !== 1) {
      this.patch({ preview: null, previewPath: null })
      return
    }
    const path = paths[0]
    const ent = this.snapshot.entries.find((e) => e.path === path)
    if (!ent || ent.isDir) {
      this.patch({ preview: null, previewPath: null })
      return
    }
    void this.loadPreview(path)
  }

  private async loadPreview(path: string): Promise<void> {
    const native = this.native
    const sessionId = this.sessionId
    if (!native || sessionId == null) {
      return
    }
    const gen = this.gen
    this.patch({ previewPath: path, preview: null })
    try {
      const preview = await native.preview({ sessionId, path })
      if (this.disposed || gen !== this.gen || this.snapshot.previewPath !== path) {
        return
      }
      this.patch({ preview })
    } catch (err) {
      if (this.disposed || gen !== this.gen) {
        return
      }
      const ce = commandErrorFromUnknown(err)
      if (ce.code === 'PathEscape') {
        this.patch({
          preview: null,
          dialog: { kind: 'path-escape', message: ce.message },
        })
        return
      }
      this.patch({ preview: { kind: 'skipped', reason: 'unknown' } })
    }
  }

  private handleExtractError(err: unknown): void {
    const ce = commandErrorFromUnknown(err)
    this.applyExtractFailed(ce)
    if (ce.code !== 'PathEscape') {
      this.setError(err)
    }
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
    if (this.extractInFlight || this.extractJobId === event.jobId) {
      const status = this.snapshot.extractJob?.status
      if (status === 'cancelled' || status === 'failed') {
        this.extractInFlight = false
        return
      }
      this.extractJobId = event.jobId
      this.extractInFlight = false
      const current = this.snapshot.extractJob
      this.patch({
        extractJob: {
          jobId: event.jobId,
          filesDone: current?.filesDone ?? 0,
          filesHint: current?.filesHint ?? null,
          bytesOut: current?.bytesOut ?? 0,
          current: current?.current ?? null,
          status: 'succeeded',
          error: null,
          errorCode: null,
          retryable: false,
        },
      })
      void this.maybeOpenExtracted(event.jobId)
      return
    }
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
    if (this.extractInFlight || this.extractJobId === event.jobId) {
      const status = this.snapshot.extractJob?.status
      if (status === 'cancelled' || status === 'succeeded') {
        this.extractInFlight = false
        return
      }
      this.extractJobId = event.jobId
      this.applyExtractFailed(new CommandError(event.code, event.message, event.retryable))
      return
    }
    const err = new CommandError(event.code, event.message, event.retryable)
    const waiter = this.jobWaiters.get(event.jobId)
    if (waiter) {
      this.jobWaiters.delete(event.jobId)
      waiter.reject(err)
      return
    }
    this.finishedJobs.set(event.jobId, { error: err })
  }

  private onJobCancelled(event: { jobId: number }): void {
    if (!this.extractInFlight && this.extractJobId !== event.jobId) {
      return
    }
    if (this.openAfterExtract?.jobId === event.jobId || this.openAfterExtract?.jobId == null) {
      this.openAfterExtract = null
    }
    this.extractJobId = event.jobId
    this.extractInFlight = false
    const current = this.snapshot.extractJob
    this.patch({
      extractJob: current
        ? { ...current, jobId: event.jobId, status: 'cancelled' }
        : {
            jobId: event.jobId,
            filesDone: 0,
            filesHint: null,
            bytesOut: 0,
            current: null,
            status: 'cancelled',
            error: null,
            errorCode: null,
            retryable: false,
          },
    })
  }

  private onExtractProgress(event: {
    jobId: number
    filesDone: number
    filesHint?: number | null
    bytesOut: number
    current?: string | null
  }): void {
    const status = this.snapshot.extractJob?.status
    if (status === 'cancelled' || status === 'failed' || status === 'succeeded') {
      return
    }
    if (!this.extractInFlight && this.extractJobId !== event.jobId) {
      return
    }
    this.extractJobId = event.jobId
    const current = this.snapshot.extractJob
    this.patch({
      extractJob: {
        jobId: event.jobId,
        filesDone: event.filesDone,
        filesHint: event.filesHint ?? current?.filesHint ?? null,
        bytesOut: event.bytesOut,
        current: event.current ?? null,
        status: 'running',
        error: null,
        errorCode: null,
        retryable: false,
      },
    })
  }

  private async closeSession(): Promise<void> {
    const sessionId = this.sessionId
    this.sessionId = null
    this.extractJobId = null
    this.extractInFlight = false
    this.lastExtract = null
    this.openAfterExtract = null
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

function spawnOpenWithSystem(path: string): void {
  if (process.argv.some((arg) => arg === 'test' || arg.endsWith('.test.ts'))) {
    return
  }
  try {
    const cmd = process.platform === 'darwin' ? 'open' : 'xdg-open'
    void Bun.spawn([cmd, path], { stdout: 'ignore', stderr: 'ignore', stdin: 'ignore' })
  } catch {
    // Extract already succeeded.
  }
}
