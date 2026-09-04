# QA — OS integration (W6)

Manual checklist for Open with / Extract here / Extract to / file associations.
Automated coverage: `cargo test -p native` argv destDir + PathEscape extract-here; `bun test` argv + settings.

Display name is **ratarmount**. Binary / desktop id is `ratarmount-gui`.

## Argv

- [ ] `ratarmount-gui archive.tar` opens that archive in a window titled `ratarmount`
- [ ] `ratarmount-gui a.tar b.tar` opens the first archive (v1: one window)
- [ ] `ratarmount-gui --extract-here archive.tar` extracts members next to the archive
- [ ] `ratarmount-gui --extract-to /tmp/out archive.tar` extracts into `/tmp/out`, not into the archive path
- [ ] `ratarmount-gui --extract-to -- archive.tar` opens the folder picker; the archive is **not** used as `destDir`
- [ ] Windows-style `ratarmount-gui.exe --extract-to -- "%1"` (archive in `%1`) never treats the archive file as the destination folder
- [ ] `ratarmount-gui --index-only archive.tar` builds/reuses a sidecar per policy and exits (no window, even without `--silent`)
- [ ] `ratarmount-gui --silent --extract-here archive.tar` does **not** show a window or overwrite dialog
- [ ] `--silent` with config `extract.overwrite = "ask"` maps to native `skip` (existing dest files kept; process does not hang)

## Extract here security

- [ ] Default destination is the directory containing the archive
- [ ] Member `../escape` (fixture `unsafe.tar`) surfaces `PathEscape` and does **not** write outside the dest
- [ ] Settings “Allow unsafe paths” defaults **off**
- [ ] Confirm summary still appears in-app when `files > 1000` or `bytes > 1 GiB` (`extractPlan`)

## Linux

Install fragments from `integrations/linux/` (`update-desktop-database` + `update-mime-database` in postinst / Settings → Register).

- [ ] `ratarmount-gui.desktop` `Name=ratarmount`, `Exec=ratarmount-gui %F`
- [ ] Desktop action **Extract here** → `ratarmount-gui --extract-here %F`
- [ ] MIME xml makes `.tar.zst` / `.tzst` / `.tgz` open the GUI
- [ ] Double-click a `.tar` / `.tar.gz` / `.zip` / `.7z` opens the GUI
- [ ] Right-click → Extract here writes next to the archive
- [ ] Does **not** steal `inode/directory`
- [ ] Settings → Register / Unregister associations is best-effort (user `~/.local/share`)

## macOS

`integrations/macos/Info.plist` (copied into the `.app` in W7).

- [ ] Document types include `.tar`, `.tar.gz`, `.tgz`, `.tar.bz2`, `.tar.xz`, `.tar.zst`, `.tzst`, `.zip`, `.7z`, `.iso`
- [ ] Role is **Viewer**, not Editor
- [ ] Open with the `.app` (until notarized: Right-click → Open)
- [ ] Drag an archive onto the app icon opens it

## Windows

`integrations/windows/ratarmount-gui.reg` (HKCU). Installer (W7) writes the same keys.

- [ ] Open With → ratarmount for `.tar` / `.tgz` / `.tar.zst` / `.zip` / `.7z`
- [ ] Context menu **Extract here** → `--extract-here "%1"`
- [ ] Context menu **Extract to…** → `--extract-to -- "%1"` (`%1` is the **archive**)
- [ ] Does **not** register `.exe` or `.msi`
- [ ] Settings → Register imports HKCU (command path = this `ratarmount-gui.exe`); Unregister deletes the ProgID and OpenWithProgids values this fragment added

## Hard rules (must stay true)

- [ ] No `readAll` napi command
- [ ] FUSE remains in-app only (“Reveal as folder”); not a desktop action
- [ ] `--silent` never hangs on a hidden overwrite-ask dialog
- [ ] `--extract-to -- archive.tar` never uses the archive as `destDir`
