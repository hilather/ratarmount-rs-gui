//! OS file-drop onto the GPUIX window.
//!
//! GPUIX 0.6 never delivers `onDrop` to React (`EVENT_PROPS` has no drop).
//! v1 drop watch is **Linux X11 only**: poll XdndSelection while the pointer
//! is over a window owned by this PID. Wayland/macOS/Windows have no watcher
//! (GPUIX does not emit `fileDrop`). The X11 `Display` installs a no-op
//! `XSetErrorHandler` so `BadWindow` cannot abort the GUI.

#[cfg(feature = "napi-addon")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(feature = "napi-addon")]
use std::thread;
#[cfg(feature = "napi-addon")]
use std::time::Duration;

#[cfg(feature = "napi-addon")]
static STARTED: AtomicBool = AtomicBool::new(false);

pub fn parse_uri_list(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let path = if let Some(rest) = line.strip_prefix("file://") {
                let rest = rest.strip_prefix("localhost").unwrap_or(rest);
                percent_decode(rest)
            } else {
                percent_decode(line)
            };
            if path.is_empty() {
                None
            } else {
                Some(path)
            }
        })
        .collect()
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Emit only when a drag owner disappears *while* the pointer is still over us
/// and we already cached URIs from a convert that ran over us.
pub fn should_emit_x11_drop(
    pointer_over_us: bool,
    had_drag_owner: bool,
    owner_cleared: bool,
    has_cached_uris: bool,
) -> bool {
    pointer_over_us && had_drag_owner && owner_cleared && has_cached_uris
}

/// Start a best-effort OS drop watcher. Safe to call more than once.
#[cfg(feature = "napi-addon")]
pub fn start(on_paths: impl Fn(Vec<String>) + Send + Sync + 'static) {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let on_paths = std::sync::Arc::new(on_paths);
    thread::Builder::new()
        .name("rgui-file-drop".into())
        .spawn(move || watch_loop(on_paths))
        .ok();
}

#[cfg(feature = "napi-addon")]
fn watch_loop(on_paths: std::sync::Arc<dyn Fn(Vec<String>) + Send + Sync>) {
    #[cfg(target_os = "linux")]
    linux::watch(&*on_paths);
    #[cfg(not(target_os = "linux"))]
    {
        let _ = on_paths;
        // macOS/Windows: GPUI still receives FileDropEvent internally; without a
        // window handle we cannot subclass. Linux X11 is the v1 path.
        loop {
            thread::sleep(Duration::from_secs(60));
        }
    }
}

#[cfg(all(feature = "napi-addon", target_os = "linux"))]
mod linux {
    use super::*;
    use std::ffi::{c_char, c_int, c_uint, c_ulong, c_void};
    use std::ptr;

    type XDisplay = c_void;
    type XWindow = c_ulong;
    type XAtom = c_ulong;
    type XStatus = c_int;

    #[repr(C)]
    struct XEvent {
        type_: c_int,
        pad: [u8; 192],
    }

    #[repr(C)]
    #[allow(dead_code)]
    struct XErrorEvent {
        type_: c_int,
        display: *mut XDisplay,
        resourceid: c_ulong,
        serial: c_ulong,
        error_code: u8,
        request_code: u8,
        minor_code: u8,
    }

    type XErrorHandler = unsafe extern "C" fn(*mut XDisplay, *mut XErrorEvent) -> c_int;

    unsafe extern "C" fn ignore_x_error(_dpy: *mut XDisplay, _ev: *mut XErrorEvent) -> c_int {
        0
    }

    #[repr(C)]
    struct XSelectionEvent {
        type_: c_int,
        serial: c_ulong,
        send_event: c_int,
        display: *mut XDisplay,
        requestor: XWindow,
        selection: XAtom,
        target: XAtom,
        property: XAtom,
        time: c_ulong,
    }

    const SELECTION_NOTIFY: c_int = 31;
    const CURRENT_TIME: c_ulong = 0;

    struct X11 {
        _lib: *mut c_void,
        display: *mut XDisplay,
        root: XWindow,
        sink: XWindow,
        xdnd_selection: XAtom,
        uri_list: XAtom,
        prop: XAtom,
        pid_atom: XAtom,
        x_pending: unsafe extern "C" fn(*mut XDisplay) -> c_int,
        x_next_event: unsafe extern "C" fn(*mut XDisplay, *mut XEvent) -> c_int,
        x_get_selection_owner: unsafe extern "C" fn(*mut XDisplay, XAtom) -> XWindow,
        x_convert_selection:
            unsafe extern "C" fn(*mut XDisplay, XAtom, XAtom, XAtom, XWindow, c_ulong) -> c_int,
        x_get_window_property: unsafe extern "C" fn(
            *mut XDisplay,
            XWindow,
            XAtom,
            c_ulong,
            c_ulong,
            c_int,
            c_ulong,
            *mut XAtom,
            *mut c_int,
            *mut c_ulong,
            *mut c_ulong,
            *mut *mut u8,
        ) -> c_int,
        x_free: unsafe extern "C" fn(*mut c_void) -> c_int,
        x_query_tree: unsafe extern "C" fn(
            *mut XDisplay,
            XWindow,
            *mut XWindow,
            *mut XWindow,
            *mut *mut XWindow,
            *mut c_uint,
        ) -> XStatus,
        x_query_pointer: unsafe extern "C" fn(
            *mut XDisplay,
            XWindow,
            *mut XWindow,
            *mut XWindow,
            *mut c_int,
            *mut c_int,
            *mut c_int,
            *mut c_int,
            *mut c_uint,
        ) -> c_int,
        x_flush: unsafe extern "C" fn(*mut XDisplay) -> c_int,
        our_pid: u32,
    }

    impl X11 {
        fn open() -> Option<Self> {
            unsafe {
                let lib = dlopen(c"libX11.so.6".as_ptr(), 1);
                if lib.is_null() {
                    return None;
                }
                let open_display: unsafe extern "C" fn(*const c_char) -> *mut XDisplay =
                    load(lib, b"XOpenDisplay\0")?;
                let default_screen: unsafe extern "C" fn(*mut XDisplay) -> c_int =
                    load(lib, b"XDefaultScreen\0")?;
                let root_window: unsafe extern "C" fn(*mut XDisplay, c_int) -> XWindow =
                    load(lib, b"XRootWindow\0")?;
                let intern: unsafe extern "C" fn(*mut XDisplay, *const c_char, c_int) -> XAtom =
                    load(lib, b"XInternAtom\0")?;
                let set_error: unsafe extern "C" fn(XErrorHandler) -> XErrorHandler =
                    load(lib, b"XSetErrorHandler\0")?;
                let create: unsafe extern "C" fn(
                    *mut XDisplay,
                    XWindow,
                    c_int,
                    c_int,
                    c_uint,
                    c_uint,
                    c_uint,
                    c_ulong,
                    c_ulong,
                ) -> XWindow = load(lib, b"XCreateSimpleWindow\0")?;
                let display = open_display(ptr::null());
                if display.is_null() {
                    return None;
                }
                // Process-global in Xlib; without this, BadWindow on a stale
                // pointer-target window prints and exit()s the GUI.
                let _prev = set_error(ignore_x_error);
                let root = root_window(display, default_screen(display));
                let sink = create(display, root, 0, 0, 1, 1, 0, 0, 0);
                Some(Self {
                    _lib: lib,
                    display,
                    root,
                    sink,
                    xdnd_selection: intern(display, c"XdndSelection".as_ptr(), 0),
                    uri_list: intern(display, c"text/uri-list".as_ptr(), 0),
                    prop: intern(display, c"RGUI_DROP".as_ptr(), 0),
                    pid_atom: intern(display, c"_NET_WM_PID".as_ptr(), 0),
                    x_pending: load(lib, b"XPending\0")?,
                    x_next_event: load(lib, b"XNextEvent\0")?,
                    x_get_selection_owner: load(lib, b"XGetSelectionOwner\0")?,
                    x_convert_selection: load(lib, b"XConvertSelection\0")?,
                    x_get_window_property: load(lib, b"XGetWindowProperty\0")?,
                    x_free: load(lib, b"XFree\0")?,
                    x_query_tree: load(lib, b"XQueryTree\0")?,
                    x_query_pointer: load(lib, b"XQueryPointer\0")?,
                    x_flush: load(lib, b"XFlush\0")?,
                    our_pid: std::process::id(),
                })
            }
        }

        fn pointer_over_us(&self) -> bool {
            unsafe {
                let mut root = 0;
                let mut child = 0;
                let mut rx = 0;
                let mut ry = 0;
                let mut wx = 0;
                let mut wy = 0;
                let mut mask = 0;
                (self.x_query_pointer)(
                    self.display,
                    self.root,
                    &mut root,
                    &mut child,
                    &mut rx,
                    &mut ry,
                    &mut wx,
                    &mut wy,
                    &mut mask,
                );
                if child == 0 {
                    return false;
                }
                self.window_is_ours(child)
            }
        }

        fn window_is_ours(&self, mut window: XWindow) -> bool {
            for _ in 0..8 {
                if self.pid_of(window) == Some(self.our_pid) {
                    return true;
                }
                let parent = self.parent_of(window);
                if parent == 0 || parent == window || parent == self.root {
                    break;
                }
                window = parent;
            }
            false
        }

        fn pid_of(&self, window: XWindow) -> Option<u32> {
            unsafe {
                let mut actual_type = 0;
                let mut actual_format = 0;
                let mut nitems = 0;
                let mut bytes_after = 0;
                let mut prop: *mut u8 = ptr::null_mut();
                let status = (self.x_get_window_property)(
                    self.display,
                    window,
                    self.pid_atom,
                    0,
                    1,
                    0,
                    6, // XA_CARDINAL
                    &mut actual_type,
                    &mut actual_format,
                    &mut nitems,
                    &mut bytes_after,
                    &mut prop,
                );
                if status != 0 || prop.is_null() || nitems == 0 {
                    if !prop.is_null() {
                        (self.x_free)(prop.cast());
                    }
                    return None;
                }
                let pid = *(prop as *const u32);
                (self.x_free)(prop.cast());
                Some(pid)
            }
        }

        fn parent_of(&self, window: XWindow) -> XWindow {
            unsafe {
                let mut root = 0;
                let mut parent = 0;
                let mut children = ptr::null_mut();
                let mut n = 0;
                if (self.x_query_tree)(
                    self.display,
                    window,
                    &mut root,
                    &mut parent,
                    &mut children,
                    &mut n,
                ) == 0
                {
                    return 0;
                }
                if !children.is_null() {
                    (self.x_free)(children.cast());
                }
                parent
            }
        }

        fn read_uri_list(&self) -> Option<Vec<String>> {
            unsafe {
                (self.x_convert_selection)(
                    self.display,
                    self.xdnd_selection,
                    self.uri_list,
                    self.prop,
                    self.sink,
                    CURRENT_TIME,
                );
                (self.x_flush)(self.display);
                let deadline = std::time::Instant::now() + Duration::from_millis(80);
                let mut ev = XEvent {
                    type_: 0,
                    pad: [0; 192],
                };
                while std::time::Instant::now() < deadline {
                    if (self.x_pending)(self.display) <= 0 {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    (self.x_next_event)(self.display, &mut ev);
                    if ev.type_ != SELECTION_NOTIFY {
                        continue;
                    }
                    let sel = &ev as *const XEvent as *const XSelectionEvent;
                    if (*sel).property == 0 {
                        return None;
                    }
                    return self.read_prop((*sel).property);
                }
                None
            }
        }

        fn read_prop(&self, atom: XAtom) -> Option<Vec<String>> {
            unsafe {
                let mut actual_type = 0;
                let mut actual_format = 0;
                let mut nitems = 0;
                let mut bytes_after = 0;
                let mut prop: *mut u8 = ptr::null_mut();
                let status = (self.x_get_window_property)(
                    self.display,
                    self.sink,
                    atom,
                    0,
                    0xFFFF,
                    0,
                    0, // AnyPropertyType
                    &mut actual_type,
                    &mut actual_format,
                    &mut nitems,
                    &mut bytes_after,
                    &mut prop,
                );
                if status != 0 || prop.is_null() {
                    if !prop.is_null() {
                        (self.x_free)(prop.cast());
                    }
                    return None;
                }
                let slice = std::slice::from_raw_parts(prop, nitems as usize);
                let text = String::from_utf8_lossy(slice).into_owned();
                (self.x_free)(prop.cast());
                let paths = parse_uri_list(&text);
                if paths.is_empty() {
                    None
                } else {
                    Some(paths)
                }
            }
        }
    }

    unsafe fn load<T>(lib: *mut c_void, name: &[u8]) -> Option<T> {
        let sym = dlsym(lib, name.as_ptr() as *const c_char);
        if sym.is_null() {
            return None;
        }
        Some(std::mem::transmute_copy::<*mut c_void, T>(&sym))
    }

    extern "C" {
        fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    pub fn watch(on_paths: &dyn Fn(Vec<String>)) {
        let Some(x11) = X11::open() else {
            loop {
                thread::sleep(Duration::from_secs(60));
            }
        };
        let mut last_owner: XWindow = 0;
        let mut cached: Option<Vec<String>> = None;
        loop {
            unsafe {
                let owner = (x11.x_get_selection_owner)(x11.display, x11.xdnd_selection);
                let over = x11.pointer_over_us();
                if owner != 0 {
                    if over && (cached.is_none() || owner != last_owner) {
                        cached = x11.read_uri_list();
                    }
                    if !over {
                        cached = None;
                    }
                } else if should_emit_x11_drop(over, last_owner != 0, true, cached.is_some()) {
                    if let Some(paths) = cached.take() {
                        on_paths(paths);
                    }
                } else {
                    cached = None;
                }
                last_owner = owner;
            }
            thread::sleep(Duration::from_millis(40));
        }
    }
}
