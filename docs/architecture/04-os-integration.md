# 04 — OS integration (Open with / right-click)

Goal: behave like 7-Zip / File Roller / The Unarchiver, not like a random unsigned binary.

## Actions

| Action | How the app is invoked | Behavior |
|---|---|---|
| Open | `ratarmount-gui <archive>` | Open window, resolve index, browse |
| Open with… | same | same |
| Extract here | `ratarmount-gui --extract-here <archive>` | Index if needed, extract all members next to archive, no window if `--silent` |
| Extract to… | `ratarmount-gui --extract-to <dir> <archive>` | Native folder picker if dir omitted |
| Index only | `ratarmount-gui --index-only <archive>` | Build sidecar per policy, then exit (no window; `--silent` is implied) |
| Reveal as folder | in-app only | spawn CLI FUSE |
| Drop archive onto window | in-app, Linux X11 | napi `fileDrop` via `startFileDropWatch` |

Multiple files: v1 opens one window per archive (or tabs if cheap). `--extract-here` may take many paths.

### Drag-and-drop (v1)

GPUIX 0.6 does not expose React `onDrop`. The GUI watches **Linux X11** `XdndSelection` while the pointer is over a window owned by this process (`_NET_WM_PID`) and emits `fileDrop`. The watcher’s Xlib connection installs a no-op `XSetErrorHandler` so a stale `BadWindow` cannot abort the process. Wayland, macOS, and Windows have no drop watcher in v1 (open via picker, recent list, or argv). The X11 loop is not run in CI.

## Linux

Shipped fragments (W7 copies them into the installer): [`integrations/linux/`](../../integrations/linux/), [`integrations/macos/Info.plist`](../../integrations/macos/Info.plist), [`integrations/windows/ratarmount-gui.reg`](../../integrations/windows/ratarmount-gui.reg). Manual QA: [`docs/qa-os-integration.md`](../qa-os-integration.md).

### Desktop file

`integrations/linux/ratarmount-gui.desktop`

```
[Desktop Entry]
Name=ratarmount
Comment=Browse and extract archives
Exec=ratarmount-gui %F
Icon=ratarmount-gui
Type=Application
Terminal=false
MimeType=application/x-tar;application/x-gtar;application/gzip;application/x-bzip2;application/x-xz;application/zstd;application/zip;application/x-7z-compressed;application/vnd.rar;application/x-iso9660-image;
Categories=Utility;Archiving;Compression;
StartupNotify=true
```

### Extra actions (Nautilus / Dolphin / Thunar via desktop actions or Nautilus python later)

```
[Desktop Action extract-here]
Name=Extract here
Exec=ratarmount-gui --extract-here %F
```

### MIME

Register the compressed-TAR types that some desktops only know as `application/zstd`. Ship `integrations/linux/ratarmount-gui.xml` so `.tar.zst` / `.tzst` / `.tgz` open the GUI.

Installer postinst: `update-desktop-database` + `update-mime-database`.  
Do **not** silently steal `inode/directory`.  
Settings checkbox: “Become default handler for TAR/ZIP/7z.”

### argv

Use a tiny native wrapper that forwards argv into the already-running app if one exists (optional v1.1). v1: new process per invocation is acceptable.

## macOS

`integrations/macos/Info.plist` `CFBundleDocumentTypes` + exported UTIs for `.tar`, `.tar.gz`, `.tgz`, `.tar.bz2`, `.tar.xz`, `.tar.zst`, `.tzst`, `.zip`, `.7z`, `.iso` (subset matching engine support).

Role: `Viewer` (not Editor) in v1.

Services / Finder Quick Actions (later): Extract Here. v1 can ship a `.workflow` or just document drag-onto-app.

Gatekeeper: signed + notarized `.app` (engine already signs tarballs with cosign; GUI needs Apple signing if distributed as .app). Until cert exists, document “Right-click → Open” like the engine macOS tarball. `packaging/build-macos-app.sh` stamps `CFBundleShortVersionString` / `CFBundleVersion` from `packaging/engine-pin` and sets `CFBundleIconFile=ratarmount-gui`.

## Windows

Installer writes (`integrations/windows/ratarmount-gui.reg`; WiX `RegistryValue` in `packaging/windows/ratarmount-gui.wxs` uses `[INSTALLFOLDER]ratarmount-gui.exe`):

```
HKCU\Software\Classes\.tar\OpenWithProgids\ratarmount-gui.Archive
HKCU\Software\Classes\.tar.gz
HKCU\Software\Classes\.tgz
HKCU\Software\Classes\.tar.zst
HKCU\Software\Classes\.zip
HKCU\Software\Classes\.7z
HKCU\Software\Classes\ratarmount-gui.Archive\shell\open\command
    "C:\Program Files\ratarmount-gui\ratarmount-gui.exe" "%1"
HKCU\Software\Classes\ratarmount-gui.Archive\shell\ExtractHere
    command = ... --extract-here "%1"
HKCU\Software\Classes\ratarmount-gui.Archive\shell\ExtractTo
    command = "C:\Program Files\ratarmount-gui\ratarmount-gui.exe" --extract-to -- "%1"
```

`--extract-to` takes an optional `<dir>` then the archive. Windows `%1` is the **archive**, not the destination. Omitting `<dir>` (the `--` form above) makes native open the folder picker, then extract. **Do not** write `--extract-to "%1"` — that treats the archive path as `destDir`. W6 argv unit test: ExtractTo / `--extract-to -- archive.tar` does not interpret the archive as `destDir`.

Also add a cascading context menu if WiX/MSIX makes it cheap.  
Do not register as handler for `.exe` or `.msi`.

Settings: “Register file associations” / “Unregister.”

## Security of “Extract here”

- Refuse to extract paths that escape the destination (`../`, absolute members) unless the user enables “allow unsafe paths.”
- Default destination for Extract here = directory containing the archive.
- Show a summary (`N files, M bytes`) before extract when `N > 1000` or `M > 1 GiB` (`extractPlan`).
- Overwrite `'ask'` is UI-only (see [05-napi-contract.md](05-napi-contract.md)). `--silent` maps `ask` → `skip` and must not hang on a hidden dialog.

## Protocol URL (optional v1.1)

`ratarmount://open?path=` — skip in v1.
