# 07 — Acceptance and risks

## v1 “ship it” checklist

- [ ] Open a 4 GiB+ `.tar.zst` on a machine with enough RAM/disk **without** JS heap growth tracking the file size
- [ ] Cold open builds an index with progress + cancel; cancel leaves no corrupt sibling
- [ ] Warm open uses the sidecar and shows the first page in < 200 ms on SSD (local file)
- [ ] CLI `ratarmount archive mnt` accepts a GUI-built sidecar
- [ ] Extract 1 file from that archive to disk
- [ ] Preview refuses a member larger than cap
- [ ] Linux: .desktop Open + Extract here
- [ ] macOS arm64: Open with the .app
- [ ] Settings persist policy `sibling` / `user-cache`
- [ ] Unwritable sibling offers cache
- [ ] No `readAll` in the napi surface

Windows open/extract is a plus for v1, not a gate, until G6 is green.

## Risks

| Risk | Mitigation |
|---|---|
| Engine session API slips | Fake backend + `TODO(engine)`; do not invent indexes |
| GPUIX still young | Keep native self-test so engine work is not blocked on UI |
| Agent copies bytes into JS “just this once” | Contract + code review of `native/` public fns |
| Distro file conflict on `/usr/bin/ratarmount` | GUI deb/rpm depends on engine package |
| Huge directory of 2M files | Cap page at 500; virtual list; no full find dump |
| Preview decoder bombs on a crafted image | Decode in Rust with size cap; fail to `skipped` |
| FUSE button on machines without macFUSE | Hide action if CLI spawn/`fusermount` probe fails |
| Index next to archive on a photo SD card | user-cache policy + remember volume |
| Password archives | secrecy type; never write password to config |

## QA fixtures (check into this repository)

| Fixture | Purpose |
|---|---|
| `tiny.tar` (a few text files) | smoke |
| `1k.tar` | paging |
| `nested.tar.gz` | breadcrumbs |
| `unsafe.tar` (member `../escape`) | PathEscape |
| `small.png` inside zip | image preview |
| generated `100k.tar` script | perf, not stored |

A 4 GiB fixture is **manual only**.
