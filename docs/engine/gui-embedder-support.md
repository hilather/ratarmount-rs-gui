# GUI embedder support — snapshot

> **Canonical (2026-09-04):** engine `ratarmount-rs` **v0.1.30** ships `ratarmount-session`. Treat **`ratarmount-rs/docs/tasks/gui-embedder-support.md`** and **`docs/session-api.md`** as the source of truth. This snapshot is historical (2026-08-29 G0 sketch). GUI production open/list/lookup/close/index use the crate; do not implement against this sketch.
>
> Implement remaining G-list items in **ratarmount-rs**. Do not paste GPUI / napi code into the engine repo.

---

# GUI embedder support — task list for ratarmount-rs

**Status:** proposed 2026-08-29  
**Consumer:** `ratarmount-rs-gui` (GPUIX desktop explorer, in-process `MountSource`, no FUSE required)  
**Related:** `docs/crates-io-policy.md`, `docs/packaging.md`, `README.md` (index discovery), `--no-mount`, `--http`, `ratarmount find`

This file lives in **ratarmount-rs**. It is the engine-side work the GUI cannot fake from JS.

---

## 0. Why this exists

The GUI will link `ratarmount-core` / `ratarmount-index` / formats / compress / compositing / remote. It will **not** shell out to `ratarmount` for browse or extract.

Today those crates work, but they are shaped as a CLI factory + export adapters. The GUI needs:

- a supported, documented **session API** (open / list page / lookup / ranged read / close)
- **index build with progress + cancel** (the CLI `--no-mount -c` path, callable from another crate)
- **stable index resolution** (sibling vs cache vs explicit) shared with the CLI
- **no FUSE / no Unix-only types** on the session API so Windows can compile the library path
- cheap **paged dirents** and **glob/FTS find** that do not load the whole catalog into RAM

Effort key: **S** < 0.5d · **M** 0.5–2d · **L** multi-day.

---

## 1. Current state (what the GUI can already use)

| Capability | Today | GUI impact |
|---|---|---|
| `MountSource` trait | yes | core of the host |
| SQLite 0.7.x index | yes | listing backend |
| `--no-mount -c` | yes, CLI only | must become a library call |
| `ratarmount find` | yes, CLI + control socket | must become a library call |
| `--index-file` / `:memory:` / `--index-folders` | yes | GUI settings must map to the same knobs |
| Sibling `.index.ptr` / `.index.{id}.sqlite` | yes | default “next to archive” policy |
| `$XDG_CACHE_HOME/ratarmount/meta-v3/` | remote sidecar cache only | reuse for remote; do not invent a second remote cache |
| `--http` Range export | yes | optional “Share via HTTP” from the GUI |
| FUSE | Linux + macOS arm64 | optional “Reveal as folder”; **not** required for explorer |
| Windows | not a product target for the CLI | library crates must still compile without `fuser` |
| Index build progress | logs / debug | GUI needs structured events |
| Cancel in-flight index | process kill | GUI needs cooperative cancel |

---

## 2. Non-goals (engine)

- Shipping GPUI / React / napi in this repo
- A first-class Windows FUSE story
- Changing the 0.7.x schema for the GUI
- Replacing `--http` with a custom GUI protocol

---

## 3. Public surface the GUI is allowed to depend on

Target crate (new, small): **`ratarmount-session`**  
Or, if a new crate is too heavy for v1: a `session` module in `ratarmount-core` plus re-exports.

Do **not** make the GUI import `ratarmount` (the binary crate).

```text
ratarmount-session
  -> ratarmount-core
  -> ratarmount-index
  -> factory (formats + compress + compositing + remote)
```

### 3.1 Types (sketch — stabilize names in G1)

```rust
pub struct OpenRequest {
    pub source: SourceSpec,          // path | url
    pub index: IndexPolicy,          // sibling | cache | path | memory | temp
    pub password: Option<Secret>,
    pub recursive: bool,
    pub recursion_depth: Option<u32>,
    pub recreate_index: Recreate,
}

pub enum Recreate { Never, IfInvalid, Always }

pub struct Session { /* opaque; Send */ }

pub struct DirPage {
    pub path: String,
    pub offset: u64,                 // keyset or rowid
    pub limit: u32,
    pub entries: Vec<DirEnt>,
    pub next_offset: Option<u64>,
    pub total_hint: Option<u64>,     // cheap COUNT if available
}

pub struct DirEnt {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: Option<i64>,
    pub mode: u32,
    pub archive_offset: Option<u64>,
}

pub struct ReadRequest {
    pub path: String,
    pub offset: u64,
    pub length: u64,                 // hard cap enforced by caller
}

pub struct ExtractRequest {
    pub members: Vec<String>,
    pub dest_dir: PathBuf,
    pub overwrite: Overwrite,
}

pub struct IndexProgress {
    pub phase: IndexPhase,           // scan | write | fts | finalize
    pub bytes_scanned: u64,
    pub bytes_total_hint: Option<u64>,
    pub entries: u64,
    pub message: Option<String>,
}
```

Paging must be **keyset** (path or rowid), not “dump `list()` into a Vec.”

### 3.2 Index resolution order (must match CLI)

Document and test as one function: `resolve_index(source, policy) -> IndexLoc`.

1. Explicit `--index-file` / GUI “this file”
2. Sibling `{archive}.index.ptr` → `{archive}.index.{id}.sqlite`
3. Well-known sibling `{archive}.index.sqlite`
4. `--index-folders` / configured extra dirs
5. User cache (see GUI `02-index-storage.md`) for **local** archives when sibling is not writable
6. Remote sidecar cache `$XDG_CACHE_HOME/ratarmount/meta-v3/` (already exists; do not fork)
7. Build a new index at the location implied by policy

`:memory:` stays a debug/test path, not a GUI default.

---

## 4. Prioritized task list

### Phase G0 — Contract freeze

| ID | Task | Effort | Status |
|---|---|---|---|
| **G0.1** | Write `docs/session-api.md` from the sketch above; list every type that crosses the GUI boundary | S | proposed |
| **G0.2** | Decide crate home: `ratarmount-session` vs `ratarmount-core::session` | S | proposed |
| **G0.3** | Windows cfg audit: session + index + formats + compress compile with `--no-default-features` and **without** `ratarmount-fuse` | M | proposed |

### Phase G1 — Session API

| ID | Task | Effort | Status |
|---|---|---|---|
| **G1.1** | `Session::open(OpenRequest) -> Result<Session>` wrapping the existing factory | M | proposed |
| **G1.2** | `list_dirents_page(path, cursor, limit)` on top of cheap `list_dirents` / SQLite | M | proposed |
| **G1.3** | `lookup(path) -> Option<DirEnt>` | S | proposed |
| **G1.4** | `read_range(path, offset, len) -> impl Read + Send` with a **max len** argument so embedders cannot accidentally slurp | M | proposed |
| **G1.5** | `extract_to(ExtractRequest) -> ExtractHandle` streaming to disk, no full-member buffer | M | proposed |
| **G1.6** | `close()` / `Drop` releases file handles and SQLite | S | proposed |
| **G1.7** | Unit tests: 1k-entry TAR page size 50; extract 1 file; read 4 KiB from a 100 MiB member | M | proposed |

### Phase G2 — Index build as a library

| ID | Task | Effort | Status |
|---|---|---|---|
| **G2.1** | Extract CLI `--no-mount -c` into `IndexJob::start(OpenRequest)` | M | proposed |
| **G2.2** | `IndexJob` implements progress callback / channel of `IndexProgress` | M | proposed |
| **G2.3** | Cooperative cancel (`AtomicBool` / token); leave a valid old sidecar if cancel mid-write (write to `.tmp` then rename) | M | proposed |
| **G2.4** | `Recreate::IfInvalid` uses existing tarstats / mtime / size checks | S | proposed |
| **G2.5** | Library test: build index for fixture TAR, cancel at 50%, assert no corrupt sidecar | M | proposed |

### Phase G3 — Find / FTS for the GUI search box

| ID | Task | Effort | Status |
|---|---|---|---|
| **G3.1** | `Session::find(pattern, FindOpts) -> FindPage` (glob + `--fts` + `--offset-order`) | M | proposed |
| **G3.2** | Paged find (do not return 2M hits) | S | proposed |
| **G3.3** | Share implementation with `ratarmount find` so CLI and GUI cannot drift | S | proposed |

### Phase G4 — Index location helpers

| ID | Task | Effort | Status |
|---|---|---|---|
| **G4.1** | `resolve_index` as a public function with the order in §3.2 | M | proposed |
| **G4.2** | Detect “sibling dir not writable” and return a structured error the GUI can map to “use user cache?” | S | proposed |
| **G4.3** | Local-archive cache directory helper that is **not** `meta-v3` (that bucket is remote sidecars). Propose `…/ratarmount/local-index-v1/` | S | proposed |
| **G4.4** | Cache eviction policy for local-index-v1 (size cap env `RATARMOUNT_LOCAL_INDEX_CACHE_BYTES`, default 2 GiB) | M | proposed |
| **G4.5** | Keep remote sidecars in existing `meta-v3` (256 MiB default) | S | proposed |

### Phase G5 — Embedder ergonomics

| ID | Task | Effort | Status |
|---|---|---|---|
| **G5.1** | `Session` is `Send` (progress + extract on a worker thread) | S | proposed |
| **G5.2** | Passwords stay in `secrecy` / zeroize; never logged | S | proposed |
| **G5.3** | Feature flags: `session` does not pull `fuse`, `nfs`, `smb`, `http` unless asked | S | proposed |
| **G5.4** | Optional `http-export` feature: start/stop the existing `--http` server on a `Session` | M | proposed |
| **G5.5** | Workspace example `examples/session-list.rs` | S | proposed |
| **G5.6** | Document in `docs/crates-io-policy.md`: GUI is a first-class embedder of L0–L4 + session | S | proposed |

### Phase G6 — Windows library path (no FUSE)

| ID | Task | Effort | Status |
|---|---|---|---|
| **G6.1** | `cargo check -p ratarmount-session --target x86_64-pc-windows-msvc` (or gnu) in CI as best-effort | M | proposed |
| **G6.2** | Replace Unix-only paths in session/index (Unix sockets are CLI-only) | M | proposed |
| **G6.3** | File locking / exclusive index write on NTFS | M | proposed |

### Phase G7 — Compatibility gates

| ID | Task | Effort | Status |
|---|---|---|---|
| **G7.1** | Index written by GUI `IndexJob` mounts with CLI `ratarmount archive mnt` | S | proposed |
| **G7.2** | Index written by CLI is opened by `Session::open` | S | proposed |
| **G7.3** | Python ratarmount 0.7.x sidecar still opens (TAR/ZIP/7z subset) | M | proposed |

---

## 5. Suggested first PR slice (engine)

1. **G0.1 + G0.2** — write the contract, pick the crate.  
2. **G1.1 + G1.2 + G1.6 + G1.7** — open + paged list + tests.  
3. **G2.1 + G2.2 + G2.3** — index job with progress/cancel.  
4. **G4.1** — one resolver used by CLI and session.

Defer HTTP-on-session, Windows CI, FTS polish.

---

## 6. Acceptance (engine ready for GUI W2)

- [ ] A second crate can open a 1 GB compressed TAR, page 200 dirents, extract one member to disk, without linking `ratarmount-fuse`
- [ ] Index build reports progress ≥ 4 times on that fixture and is cancellable
- [ ] Sidecar is valid for the existing CLI
- [ ] No API requires the embedder to hold the member bytes in one `Vec<u8>`

---

## 7. File touch map (expected)

| Area | Change |
|---|---|
| `ratarmount-session/` or `ratarmount-core/src/session.rs` | **new** |
| `ratarmount/src/main.rs` | CLI `--no-mount` / `find` call session |
| `ratarmount-index` | paged queries, local-index-v1 helper |
| `docs/session-api.md` | **new** |
| `docs/crates-io-policy.md` | embedder note |
| `.github/workflows/ci.yml` | optional windows-lib job |

---

*Written for the 2026-08-29 ratarmount-rs-gui planning pack. Implement G0–G7 in **ratarmount-rs**; do not paste GPUI / napi code there.*
