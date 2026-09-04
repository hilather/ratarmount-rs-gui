# 05 — napi contract

This is the only API React may call. Agents implementing UI against anything else are wrong.

All commands are async. Large work returns `{ jobId }` and completes via events.

W1 stubs: the compiled addon is synchronous (`#[napi] fn`, not `async fn`). JS `await cmd()` still works. Long index/extract work in W2+ must return `{ jobId }` before running on a worker — do not block the GPUI/Bun thread.

There is **no** `readAll` command. Do not add one.

Native **must not** return a raw SQLite `offset: u64` (or rowid) to JS as a paging API. Paging uses an **opaque** `cursor: string` that native may encode as rowid, path, or both. JS treats the cursor as a black box and must not parse it.

## Types

```ts
type SessionId = number
type JobId = number

/** Opaque keyset. Native-encoded; JS must not parse. */
type Cursor = string

type IndexPolicy = 'sibling' | 'user-cache' | 'explicit' | 'temp' | 'memory'
// 'memory' is hidden/test-only (`:memory:`). UI settings must not offer it.
// Native rejects 'memory' unless RGUI_FAKE=1 or cfg(test) / native --self-test.

type Recreate = 'never' | 'if-invalid' | 'always'

/** Native extract only. Config may store 'ask' for the UI; see Overwrite protocol. */
type Overwrite = 'skip' | 'replace'

interface DirEnt {
  name: string
  path: string          // archive-relative, leading slash, no trailing slash (0.7.x files.path + name)
  isDir: boolean
  size: number          // bytes; 0 for directories unless the catalog says otherwise
  mtime: number | null  // unix seconds; null if unknown
  mode: number          // unix mode bits
  /** Optional catalog hint. UI must not use this to fetch bytes (no readAt / readAll). */
  archiveOffset?: number
}

interface DirPage {
  path: string
  entries: DirEnt[]
  nextCursor: Cursor | null
  totalHint: number | null  // cheap COUNT if available; do not block the frame on a full scan
}

interface FindPage {
  pattern: string
  mode: 'glob' | 'fts'
  entries: DirEnt[]
  nextCursor: Cursor | null
  totalHint: number | null
}

interface ExtractConflict {
  member: string
  destPath: string
}

/** Dest-side summary. Never an unbounded path dump (hard rule 3). */
interface ExtractPlan {
  files: number                 // COUNT from the index (extract-all) or members.length
  bytes: number                 // SUM of sizes from the index — not a JS catalog
  conflictCount: number         // conflicts found (may be a lower bound if scan truncated)
  conflicts: ExtractConflict[]  // sample only; length ≤ EXTRACT_PLAN_CONFLICT_SAMPLE (50)
  conflictsTruncated: boolean   // true if sample cap or scan cap stopped the list
}

/** Native caps for extractPlan (not configurable by the UI). */
// EXTRACT_PLAN_CONFLICT_SAMPLE = 50        — max conflict objects returned to JS
// EXTRACT_PLAN_CONFLICT_SCAN_ROWS = 10_000 — max dest-stat calls per plan
// EXTRACT_PLAN_CONFLICT_SCAN_MS = 250      — max wall time for dest-stat loop

interface Config {
  index: {
    policy: Exclude<IndexPolicy, 'memory'>  // persisted; never 'memory'
    explicitPath: string
    extraDirs: string[]
    recreate: Recreate
    localCacheBytes: number
    rememberUnwritableVolumes: boolean
  }
  preview: {
    maxBytes: number            // default 8 MiB; native clamps to 64 MiB
    openLargeWithSystem: boolean
  }
  extract: {
    overwrite: 'ask' | 'skip' | 'replace'  // 'ask' is UI-only (see Overwrite protocol)
    allowUnsafePaths: boolean              // default false
  }
  engine: {
    bundleCli: boolean
    cliPath: string             // empty = bundled then PATH
  }
  recent?: {
    paths: string[]             // W8; paths only, never passwords
  }
}
```

Engine G0 sketch `DirPage.offset` / `next_offset` stay **inside native**. The napi layer maps them to `cursor` / `nextCursor`.

## Commands

```ts
open(opts: {
  source: string
  policy: IndexPolicy
  explicitPath?: string
  recreate: Recreate
  password?: string
  recursive?: boolean          // v1 default false (non-recursive)
  recursionDepth?: number      // only if recursive; omit ⇒ engine default depth
}): Promise<{ sessionId: SessionId } | { jobId: JobId }>
// If an index must be built, returns jobId first; sessionId arrives on jobSucceeded.

close(sessionId: SessionId): Promise<void>

list(opts: {
  sessionId: SessionId
  path: string
  cursor?: Cursor
  limit?: number          // default 200, max 500
}): Promise<DirPage>

lookup(opts: { sessionId: SessionId; path: string }): Promise<DirEnt | null>

find(opts: {
  sessionId: SessionId
  pattern: string
  mode: 'glob' | 'fts'
  cursor?: Cursor
  limit?: number
}): Promise<FindPage>

preview(opts: {
  sessionId: SessionId
  path: string
}): Promise<
  | { kind: 'text'; text: string; truncated: boolean }
  | { kind: 'image'; png: Uint8Array }     // native-resized, ≤ preview cap
  | { kind: 'skipped'; reason: 'too-large' | 'binary' | 'unknown' }
>

/**
 * Dest-side probe. Does not write. Used for overwrite-ask UI and N/M confirm.
 * `files`/`bytes` come from SQLite aggregates (or the selected members), not a path dump.
 * Conflict dest-stats are **capped** (sample 50, scan ≤ 10_000 rows or 250 ms).
 * v1 stays a synchronous command because of those caps — not a `{ jobId }`.
 * A full conflict-enumeration job is out of scope for v1.
 */
extractPlan(opts: {
  sessionId: SessionId
  members: string[]       // empty = all
  destDir: string
}): Promise<ExtractPlan>

extract(opts: {
  sessionId: SessionId
  members: string[]       // empty = all
  destDir: string
  overwrite: Overwrite    // 'skip' | 'replace' only — never 'ask'
}): Promise<{ jobId: JobId }>

cancel(jobId: JobId): Promise<void>

pickFile(): Promise<string | null>
pickDir(): Promise<string | null>

getConfig(): Promise<Config>
setConfig(patch: Partial<Config>): Promise<Config>

registerAssociations(): Promise<void>
unregisterAssociations(): Promise<void>

fuseMount(sessionId: SessionId): Promise<{ mountpoint: string } | { error: string }>
fuseUnmount(sessionId: SessionId): Promise<void>

httpStart(sessionId: SessionId, bind?: string): Promise<{ url: string }>
httpStop(sessionId: SessionId): Promise<void>
```

`preview` image path: native decodes and resizes to ≤ 2048px on the long edge **in Rust**, then returns a PNG no larger than `preview.maxBytes` (already clamped). If that cannot be done, `skipped`.

`preview` text path: read min(file_size, preview.maxBytes), lossy UTF-8.

There is **no** `readAll(path)`.

### Overwrite protocol

`'ask'` is **UI-only**. It may appear in `config.toml` `[extract] overwrite = "ask"` and in `Config.extract.overwrite`. It is **not** a legal `extract()` argument.

1. UI calls `extractPlan` (counts, bytes, capped dest-conflict sample).
2. If `files > 1000` or `bytes > 1 GiB`, UI confirms.
3. If config is `ask` and `conflictCount > 0`, UI dialog shows the sample (and “and N more…” if `conflictsTruncated`) → skip or replace (or cancel).
4. UI calls `extract({ overwrite: 'skip' | 'replace' })`.
5. Native `extract` with `overwrite: 'ask'` **rejects** (error `Internal`, `retryable: false`).

Do **not** put `extractPlan.conflicts` into React list state as a catalog. The sample is dialog copy only.

**Argv / `--silent` must not hang on a hidden dialog:**

| Invocation | Window? | Config `ask` maps to |
|---|---|---|
| In-app Extract to… | yes | dialog via `extractPlan` then skip/replace |
| `--extract-here` / `--extract-to DIR` without `--silent` | yes (unless `--silent`) | same dialog |
| `--silent` (any extract argv) | no | **skip** (never replace, never dialog) |
| `--extract-to` with dir omitted | folder picker first, then same as in-app | |

### Recursion

v1 default: `recursive` omitted or `false` → non-recursive open (no nested archive auto-mount).  
`recursive: true` with `recursionDepth` omitted → engine default depth (whatever `Session::open` / CLI `-r` uses). Do not invent a GUI-side depth number.  
`policy: 'memory'` is rejected in the UI settings form; allowed only for tests / `RGUI_FAKE=1`.

### Password

`password` is accepted only on `open`. The string may exist in JS **only for that call**. Do not store it in React state, `Config`, `config.toml`, recent paths, or logs. Native zeroizes after constructing the engine `Secret` (G5.2). Password dialog: wave **W4**.

## Events

```ts
on('indexProgress', (e: { jobId: JobId; phase: string; bytesScanned: number; bytesHint?: number; entries: number }) => {})
on('extractProgress', (e: { jobId: JobId; filesDone: number; filesHint?: number; bytesOut: number; current?: string }) => {})
on('jobSucceeded', (e: { jobId: JobId; sessionId?: SessionId }) => {})
on('jobFailed', (e: { jobId: JobId; code: string; message: string; retryable: boolean }) => {})
on('jobCancelled', (e: { jobId: JobId }) => {})
```

Command-level errors use the same `{ code, message, retryable }` shape as `jobFailed` (minus `jobId`). Thrown JS `Error` objects have those own properties (`e.code`, `e.message`, `e.retryable`); do not parse a status string.

## Error codes

`NotFound` `NotWritable` `SiblingNotWritable` `BadPassword` `UnsupportedFormat` `CorruptIndex` `Cancelled` `PreviewTooLarge` `PathEscape` `Busy` `Internal`

| `retryable` | Codes |
|---|---|
| `true` | `Busy`, `NotWritable`, `SiblingNotWritable` |
| `false` | `PathEscape`, `BadPassword`, `UnsupportedFormat`, `NotFound`, `CorruptIndex`, `Cancelled`, `PreviewTooLarge`, `Internal` |

`SiblingNotWritable` is retryable after the user switches policy to `user-cache`. `Busy` is retryable after the in-flight job ends. Do not auto-retry `BadPassword` without a new `open` password.

## Handle table

Native keeps `HashMap<SessionId, SessionState>`. Drop on `close` and on process exit. Do not expose raw pointers to JS.

## Preview cap default

`8 * 1024 * 1024` bytes. Configurable. Hard ceiling in native: `64 * 1024 * 1024` even if the user types a larger number.
