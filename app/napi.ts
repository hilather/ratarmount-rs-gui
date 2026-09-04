export type SessionId = number
export type JobId = number
export type Cursor = string

export type IndexPolicy = 'sibling' | 'user-cache' | 'explicit' | 'temp' | 'memory'
export type Recreate = 'never' | 'if-invalid' | 'always'

export interface DirEnt {
  name: string
  path: string
  isDir: boolean
  size: number
  mtime: number | null
  mode: number
  archiveOffset?: number
}

export interface DirPage {
  path: string
  entries: DirEnt[]
  nextCursor: Cursor | null
  totalHint: number | null
}

export interface OpenOpts {
  source: string
  policy: IndexPolicy
  explicitPath?: string
  recreate: Recreate
  password?: string
  recursive?: boolean
  recursionDepth?: number
}

export type OpenResult = { sessionId: SessionId } | { jobId: JobId }

export interface ListOpts {
  sessionId: SessionId
  path: string
  cursor?: Cursor
  limit?: number
}

export interface LookupOpts {
  sessionId: SessionId
  path: string
}

export interface JobSucceededEvent {
  jobId: JobId
  sessionId?: SessionId | null
}

export interface JobFailedEvent {
  jobId: JobId
  code: string
  message: string
  retryable: boolean
}

export interface JobCancelledEvent {
  jobId: JobId
}

export interface ExtractProgressEvent {
  jobId: JobId
  filesDone: number
  filesHint: number | null
  bytesOut: number
  current: string | null
}

export type NativeOverwrite = 'skip' | 'replace'
export type ConfigOverwrite = 'ask' | 'skip' | 'replace'

export interface ExtractConflict {
  member: string
  destPath: string
}

export interface ExtractPlan {
  files: number
  bytes: number
  conflictCount: number
  conflicts: ExtractConflict[]
  conflictsTruncated: boolean
}

export type PreviewResult =
  | { kind: 'text'; text: string; truncated: boolean }
  | { kind: 'image'; png: Uint8Array }
  | { kind: 'skipped'; reason: 'too-large' | 'binary' | 'unknown' }

export interface PreviewOpts {
  sessionId: SessionId
  path: string
}

export interface ExtractPlanOpts {
  sessionId: SessionId
  members: string[]
  destDir: string
}

export interface ExtractOpts {
  sessionId: SessionId
  members: string[]
  destDir: string
  overwrite: NativeOverwrite
}

export interface Config {
  extract: {
    overwrite: ConfigOverwrite
    allowUnsafePaths: boolean
  }
  preview: {
    maxBytes: number
    openLargeWithSystem: boolean
  }
}

export class CommandError extends Error {
  readonly code: string
  readonly retryable: boolean

  constructor(code: string, message: string, retryable: boolean) {
    super(message)
    this.name = 'CommandError'
    this.code = code
    this.retryable = retryable
  }
}

export function commandErrorFromUnknown(err: unknown): CommandError {
  if (err instanceof CommandError) {
    return err
  }
  if (err && typeof err === 'object') {
    const rec = err as { code?: unknown; message?: unknown; retryable?: unknown }
    const message = typeof rec.message === 'string' ? rec.message : String(err)
    const code = typeof rec.code === 'string' ? rec.code : 'Internal'
    const retryable = rec.retryable === true
    return new CommandError(code, message, retryable)
  }
  return new CommandError('Internal', String(err), false)
}

export interface NativeAddon {
  pickFile(): Promise<string | null>
  pickDir(): Promise<string | null>
  open(opts: OpenOpts): Promise<OpenResult>
  close(sessionId: SessionId): Promise<void>
  list(opts: ListOpts): Promise<DirPage>
  lookup(opts: LookupOpts): Promise<DirEnt | null>
  preview(opts: PreviewOpts): Promise<PreviewResult>
  extractPlan(opts: ExtractPlanOpts): Promise<ExtractPlan>
  extract(opts: ExtractOpts): Promise<{ jobId: JobId }>
  cancel(jobId: JobId): Promise<void>
  getConfig(): Promise<Config>
  on(event: 'jobSucceeded', cb: (e: JobSucceededEvent) => void): void
  on(event: 'jobFailed', cb: (e: JobFailedEvent) => void): void
  on(event: 'jobCancelled', cb: (e: JobCancelledEvent) => void): void
  on(event: 'extractProgress', cb: (e: ExtractProgressEvent) => void): void
}

function asNumber(value: unknown): number | null {
  if (value == null) {
    return null
  }
  if (typeof value === 'bigint') {
    return Number(value)
  }
  if (typeof value === 'number' && Number.isFinite(value)) {
    return value
  }
  return null
}

function pick<T>(obj: Record<string, unknown>, camel: string, snake: string): T | undefined {
  if (camel in obj) {
    return obj[camel] as T
  }
  if (snake in obj) {
    return obj[snake] as T
  }
  return undefined
}

export function normalizeDirEnt(raw: unknown): DirEnt {
  const obj = (raw ?? {}) as Record<string, unknown>
  const mtime = asNumber(pick(obj, 'mtime', 'mtime'))
  const archiveOffset = asNumber(pick(obj, 'archiveOffset', 'archive_offset'))
  return {
    name: String(obj.name ?? ''),
    path: String(obj.path ?? ''),
    isDir: Boolean(pick(obj, 'isDir', 'is_dir')),
    size: asNumber(obj.size) ?? 0,
    mtime,
    mode: asNumber(obj.mode) ?? 0,
    ...(archiveOffset == null ? {} : { archiveOffset }),
  }
}

export function normalizeDirPage(raw: unknown): DirPage {
  const obj = (raw ?? {}) as Record<string, unknown>
  const next = pick<unknown>(obj, 'nextCursor', 'next_cursor')
  const hint = asNumber(pick(obj, 'totalHint', 'total_hint'))
  const entriesRaw = obj.entries
  const entries = Array.isArray(entriesRaw) ? entriesRaw.map(normalizeDirEnt) : []
  return {
    path: String(obj.path ?? '/'),
    entries,
    // Last page is JS null. Missing/undefined must not look like "more pages".
    nextCursor: next == null ? null : String(next),
    totalHint: hint,
  }
}

export function normalizeOpenResult(raw: unknown): OpenResult {
  const obj = (raw ?? {}) as Record<string, unknown>
  const sessionId = asNumber(pick(obj, 'sessionId', 'session_id'))
  const jobId = asNumber(pick(obj, 'jobId', 'job_id'))
  if (sessionId != null && jobId == null) {
    return { sessionId }
  }
  if (jobId != null && sessionId == null) {
    return { jobId }
  }
  if (sessionId != null) {
    return { sessionId }
  }
  if (jobId != null) {
    return { jobId }
  }
  throw new CommandError('Internal', 'open returned neither sessionId nor jobId', false)
}

export function normalizeJobSucceeded(raw: unknown): JobSucceededEvent {
  const obj = (raw ?? {}) as Record<string, unknown>
  const jobId = asNumber(pick(obj, 'jobId', 'job_id'))
  if (jobId == null) {
    throw new CommandError('Internal', 'jobSucceeded missing jobId', false)
  }
  return {
    jobId,
    sessionId: asNumber(pick(obj, 'sessionId', 'session_id')),
  }
}

export function normalizeJobFailed(raw: unknown): JobFailedEvent {
  const obj = (raw ?? {}) as Record<string, unknown>
  const jobId = asNumber(pick(obj, 'jobId', 'job_id'))
  return {
    jobId: jobId ?? 0,
    code: typeof obj.code === 'string' ? obj.code : 'Internal',
    message: typeof obj.message === 'string' ? obj.message : 'job failed',
    retryable: obj.retryable === true,
  }
}

export function normalizeJobCancelled(raw: unknown): JobCancelledEvent {
  const obj = (raw ?? {}) as Record<string, unknown>
  const jobId = asNumber(pick(obj, 'jobId', 'job_id'))
  return { jobId: jobId ?? 0 }
}

export function normalizeExtractProgress(raw: unknown): ExtractProgressEvent {
  const obj = (raw ?? {}) as Record<string, unknown>
  const jobId = asNumber(pick(obj, 'jobId', 'job_id'))
  const current = pick<unknown>(obj, 'current', 'current')
  return {
    jobId: jobId ?? 0,
    filesDone: asNumber(pick(obj, 'filesDone', 'files_done')) ?? 0,
    filesHint: asNumber(pick(obj, 'filesHint', 'files_hint')),
    bytesOut: asNumber(pick(obj, 'bytesOut', 'bytes_out')) ?? 0,
    current: current == null ? null : String(current),
  }
}

export function normalizeExtractConflict(raw: unknown): ExtractConflict {
  const obj = (raw ?? {}) as Record<string, unknown>
  return {
    member: String(obj.member ?? ''),
    destPath: String(pick(obj, 'destPath', 'dest_path') ?? ''),
  }
}

export function normalizeExtractPlan(raw: unknown): ExtractPlan {
  const obj = (raw ?? {}) as Record<string, unknown>
  const conflictsRaw = obj.conflicts
  const conflicts = Array.isArray(conflictsRaw)
    ? conflictsRaw.slice(0, 50).map(normalizeExtractConflict)
    : []
  return {
    files: asNumber(obj.files) ?? 0,
    bytes: asNumber(obj.bytes) ?? 0,
    conflictCount: asNumber(pick(obj, 'conflictCount', 'conflict_count')) ?? 0,
    conflicts,
    conflictsTruncated: Boolean(pick(obj, 'conflictsTruncated', 'conflicts_truncated')),
  }
}

export function normalizePreview(raw: unknown): PreviewResult {
  const obj = (raw ?? {}) as Record<string, unknown>
  const kind = String(obj.kind ?? 'skipped')
  if (kind === 'text') {
    return {
      kind: 'text',
      text: String(obj.text ?? ''),
      truncated: obj.truncated === true,
    }
  }
  if (kind === 'image') {
    return { kind: 'skipped', reason: 'unknown' }
  }
  const reason = String(obj.reason ?? 'unknown')
  if (reason === 'too-large' || reason === 'binary' || reason === 'unknown') {
    return { kind: 'skipped', reason }
  }
  return { kind: 'skipped', reason: 'unknown' }
}

export function normalizeConfig(raw: unknown): Config {
  const obj = (raw ?? {}) as Record<string, unknown>
  const extract = (pick(obj, 'extract', 'extract') ?? {}) as Record<string, unknown>
  const preview = (pick(obj, 'preview', 'preview') ?? {}) as Record<string, unknown>
  const overwriteRaw = String(pick(extract, 'overwrite', 'overwrite') ?? 'ask')
  const overwrite: ConfigOverwrite =
    overwriteRaw === 'skip' || overwriteRaw === 'replace' || overwriteRaw === 'ask'
      ? overwriteRaw
      : 'ask'
  return {
    extract: {
      overwrite,
      allowUnsafePaths: Boolean(pick(extract, 'allowUnsafePaths', 'allow_unsafe_paths')),
    },
    preview: {
      maxBytes: asNumber(pick(preview, 'maxBytes', 'max_bytes')) ?? 8 * 1024 * 1024,
      openLargeWithSystem: pick(preview, 'openLargeWithSystem', 'open_large_with_system') !== false,
    },
  }
}

function normalizeJobId(raw: unknown): { jobId: JobId } {
  const obj = (raw ?? {}) as Record<string, unknown>
  const jobId = asNumber(pick(obj, 'jobId', 'job_id'))
  if (jobId == null) {
    throw new CommandError('Internal', 'extract returned no jobId', false)
  }
  return { jobId }
}

type RawAddon = Record<string, unknown>

function fn(mod: RawAddon, camel: string, snake: string): (...args: unknown[]) => unknown {
  const value = mod[camel] ?? mod[snake]
  if (typeof value !== 'function') {
    throw new CommandError('Internal', `native addon missing ${camel}()`, false)
  }
  return value as (...args: unknown[]) => unknown
}

/** Map the generated napi module onto the 05 contract (camelCase, nulls, promises). */
export function wrapNativeModule(mod: unknown): NativeAddon {
  const raw = (mod ?? {}) as RawAddon
  const pickFileFn = fn(raw, 'pickFile', 'pick_file')
  const pickDirFn = fn(raw, 'pickDir', 'pick_dir')
  const openFn = fn(raw, 'open', 'open')
  const closeFn = fn(raw, 'close', 'close')
  const listFn = fn(raw, 'list', 'list')
  const lookupFn = fn(raw, 'lookup', 'lookup')
  const previewFn = fn(raw, 'preview', 'preview')
  const extractPlanFn = fn(raw, 'extractPlan', 'extract_plan')
  const extractFn = fn(raw, 'extract', 'extract')
  const cancelFn = fn(raw, 'cancel', 'cancel')
  const getConfigFn = fn(raw, 'getConfig', 'get_config')
  const onFn = fn(raw, 'on', 'on')

  return {
    async pickFile() {
      try {
        const value = await Promise.resolve(pickFileFn())
        return value == null ? null : String(value)
      } catch (err) {
        throw commandErrorFromUnknown(err)
      }
    },
    async pickDir() {
      try {
        const value = await Promise.resolve(pickDirFn())
        return value == null ? null : String(value)
      } catch (err) {
        throw commandErrorFromUnknown(err)
      }
    },
    async open(opts) {
      try {
        return normalizeOpenResult(await Promise.resolve(openFn(opts)))
      } catch (err) {
        throw commandErrorFromUnknown(err)
      }
    },
    async close(sessionId) {
      try {
        await Promise.resolve(closeFn(sessionId))
      } catch (err) {
        throw commandErrorFromUnknown(err)
      }
    },
    async list(opts) {
      try {
        return normalizeDirPage(await Promise.resolve(listFn(opts)))
      } catch (err) {
        throw commandErrorFromUnknown(err)
      }
    },
    async lookup(opts) {
      try {
        const ent = await Promise.resolve(lookupFn(opts))
        return ent == null ? null : normalizeDirEnt(ent)
      } catch (err) {
        throw commandErrorFromUnknown(err)
      }
    },
    async preview(opts) {
      try {
        return normalizePreview(await Promise.resolve(previewFn(opts)))
      } catch (err) {
        throw commandErrorFromUnknown(err)
      }
    },
    async extractPlan(opts) {
      try {
        return normalizeExtractPlan(await Promise.resolve(extractPlanFn(opts)))
      } catch (err) {
        throw commandErrorFromUnknown(err)
      }
    },
    async extract(opts) {
      try {
        if (opts.overwrite !== 'skip' && opts.overwrite !== 'replace') {
          throw new CommandError('Internal', "extract overwrite 'ask' is UI-only; pass 'skip' or 'replace'", false)
        }
        return normalizeJobId(await Promise.resolve(extractFn(opts)))
      } catch (err) {
        throw commandErrorFromUnknown(err)
      }
    },
    async cancel(jobId) {
      try {
        await Promise.resolve(cancelFn(jobId))
      } catch (err) {
        throw commandErrorFromUnknown(err)
      }
    },
    async getConfig() {
      try {
        return normalizeConfig(await Promise.resolve(getConfigFn()))
      } catch (err) {
        throw commandErrorFromUnknown(err)
      }
    },
    on(event, cb) {
      try {
        onFn(event, (payload: unknown) => {
          if (event === 'jobSucceeded') {
            ;(cb as (e: JobSucceededEvent) => void)(normalizeJobSucceeded(payload))
            return
          }
          if (event === 'jobFailed') {
            ;(cb as (e: JobFailedEvent) => void)(normalizeJobFailed(payload))
            return
          }
          if (event === 'jobCancelled') {
            ;(cb as (e: JobCancelledEvent) => void)(normalizeJobCancelled(payload))
            return
          }
          if (event === 'extractProgress') {
            ;(cb as (e: ExtractProgressEvent) => void)(normalizeExtractProgress(payload))
          }
        })
      } catch (err) {
        throw commandErrorFromUnknown(err)
      }
    },
  }
}
