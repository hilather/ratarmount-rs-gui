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
  open(opts: OpenOpts): Promise<OpenResult>
  close(sessionId: SessionId): Promise<void>
  list(opts: ListOpts): Promise<DirPage>
  lookup(opts: LookupOpts): Promise<DirEnt | null>
  on(event: 'jobSucceeded', cb: (e: JobSucceededEvent) => void): void
  on(event: 'jobFailed', cb: (e: JobFailedEvent) => void): void
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
  const openFn = fn(raw, 'open', 'open')
  const closeFn = fn(raw, 'close', 'close')
  const listFn = fn(raw, 'list', 'list')
  const lookupFn = fn(raw, 'lookup', 'lookup')
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
    on(event, cb) {
      try {
        onFn(event, (payload: unknown) => {
          if (event === 'jobSucceeded') {
            ;(cb as (e: JobSucceededEvent) => void)(normalizeJobSucceeded(payload))
            return
          }
          if (event === 'jobFailed') {
            ;(cb as (e: JobFailedEvent) => void)(normalizeJobFailed(payload))
          }
        })
      } catch (err) {
        throw commandErrorFromUnknown(err)
      }
    },
  }
}
