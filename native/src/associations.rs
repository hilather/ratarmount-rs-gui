use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::error::{ApiError, Result};

pub const DESKTOP_FILE: &str = include_str!("../../integrations/linux/ratarmount-gui.desktop");
pub const MIME_FILE: &str = include_str!("../../integrations/linux/ratarmount-gui.xml");
pub const PLIST_FILE: &str = include_str!("../../integrations/macos/Info.plist");
pub const REG_FILE: &str = include_str!("../../integrations/windows/ratarmount-gui.reg");

pub const DESKTOP_NAME: &str = "ratarmount-gui.desktop";
pub const MIME_NAME: &str = "ratarmount-gui.xml";
pub const REG_NAME: &str = "ratarmount-gui.reg";
pub const UNINSTALL_REG_NAME: &str = "uninstall.reg";

pub const INSTALLER_EXE: &str = r"C:\Program Files\ratarmount-gui\ratarmount-gui.exe";

/// Extensions this fragment adds under `HKCU\Software\Classes\<ext>\OpenWithProgids`.
pub const WINDOWS_OPENWITH_EXTS: &[&str] = &[".tar", ".tar.gz", ".tgz", ".tar.zst", ".zip", ".7z"];

pub const WINDOWS_PROGID: &str = "ratarmount-gui.Archive";

pub fn user_data_home() -> PathBuf {
    if cfg!(windows) {
        return windows_data_home(
            std::env::var_os("APPDATA").as_deref(),
            std::env::var_os("USERPROFILE").as_deref(),
        );
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg);
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".local/share");
    }
    PathBuf::from(".")
}

pub fn windows_data_home(
    appdata: Option<&std::ffi::OsStr>,
    userprofile: Option<&std::ffi::OsStr>,
) -> PathBuf {
    if let Some(appdata) = appdata {
        if !appdata.is_empty() {
            return PathBuf::from(appdata).join("ratarmount-gui");
        }
    }
    if let Some(profile) = userprofile {
        if !profile.is_empty() {
            return PathBuf::from(profile)
                .join("AppData")
                .join("Roaming")
                .join("ratarmount-gui");
        }
    }
    PathBuf::from(".")
}

pub fn gui_executable() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        let name = exe
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if name.contains("ratarmount-gui") {
            return exe;
        }
    }
    default_gui_exe()
}

fn default_gui_exe() -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(INSTALLER_EXE)
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("ratarmount-gui")
    }
}

/// `.reg` escaping: a path `D:\gui\ratarmount-gui.exe` becomes `D:\\gui\\ratarmount-gui.exe`.
pub fn reg_escape_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .replace('\\', "\\\\")
}

pub fn substitute_reg_exe(template: &str, exe: &Path) -> String {
    let placeholder = reg_escape_path(Path::new(INSTALLER_EXE));
    template.replace(&placeholder, &reg_escape_path(exe))
}

/// Uninstall fragment: drop our OpenWithProgids values and the ProgID tree.
/// Does not delete the `.tar` (etc.) keys themselves.
pub fn windows_uninstall_reg() -> String {
    let mut out = String::from("Windows Registry Editor Version 5.00\n\n");
    for ext in WINDOWS_OPENWITH_EXTS {
        out.push_str(&format!(
            "[HKEY_CURRENT_USER\\Software\\Classes\\{ext}\\OpenWithProgids]\n\"{WINDOWS_PROGID}\"=-\n\n"
        ));
    }
    out.push_str(&format!(
        "[-HKEY_CURRENT_USER\\Software\\Classes\\{WINDOWS_PROGID}]\n"
    ));
    out
}

pub fn reg_status_to_result(op: &str, success: bool) -> Result<()> {
    if success {
        Ok(())
    } else {
        Err(ApiError::internal(format!("reg {op} failed")))
    }
}

pub fn write_association_files(data_home: &Path, exe: &Path) -> Result<()> {
    write_linux_files(data_home)?;
    write_bundle_fragments(data_home, exe)
}

pub fn register_in(data_home: &Path) -> Result<()> {
    write_association_files(data_home, &gui_executable())?;
    refresh_linux_databases(data_home);
    apply_windows_reg(&data_home.join("ratarmount-gui").join(REG_NAME), "import")
}

pub fn unregister_in(data_home: &Path) -> Result<()> {
    let uninstall_path = data_home.join("ratarmount-gui").join(UNINSTALL_REG_NAME);
    if let Some(parent) = uninstall_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| ApiError::not_writable(format!("create assoc dir: {err}")))?;
    }
    fs::write(&uninstall_path, windows_uninstall_reg())
        .map_err(|err| ApiError::not_writable(format!("write uninstall.reg: {err}")))?;
    apply_windows_reg(&uninstall_path, "delete")?;
    let _ = fs::remove_file(data_home.join("applications").join(DESKTOP_NAME));
    let _ = fs::remove_file(data_home.join("mime/packages").join(MIME_NAME));
    let _ = fs::remove_file(data_home.join("ratarmount-gui").join(REG_NAME));
    let _ = fs::remove_file(data_home.join("ratarmount-gui").join("Info.plist"));
    let _ = fs::remove_file(&uninstall_path);
    refresh_linux_databases(data_home);
    Ok(())
}

fn apply_windows_reg(reg_path: &Path, op: &str) -> Result<()> {
    #[cfg(windows)]
    {
        import_windows_registry(reg_path, op)
    }
    #[cfg(not(windows))]
    {
        let _ = reg_path;
        reg_status_to_result(op, true)
    }
}

fn write_linux_files(data_home: &Path) -> Result<()> {
    let applications = data_home.join("applications");
    let mime_packages = data_home.join("mime/packages");
    fs::create_dir_all(&applications)
        .map_err(|err| ApiError::not_writable(format!("create applications dir: {err}")))?;
    fs::create_dir_all(&mime_packages)
        .map_err(|err| ApiError::not_writable(format!("create mime dir: {err}")))?;
    fs::write(applications.join(DESKTOP_NAME), DESKTOP_FILE)
        .map_err(|err| ApiError::not_writable(format!("write desktop file: {err}")))?;
    fs::write(mime_packages.join(MIME_NAME), MIME_FILE)
        .map_err(|err| ApiError::not_writable(format!("write MIME file: {err}")))?;
    Ok(())
}

fn write_bundle_fragments(data_home: &Path, exe: &Path) -> Result<()> {
    let dir = data_home.join("ratarmount-gui");
    fs::create_dir_all(&dir)
        .map_err(|err| ApiError::not_writable(format!("create assoc dir: {err}")))?;
    fs::write(dir.join(REG_NAME), substitute_reg_exe(REG_FILE, exe))
        .map_err(|err| ApiError::not_writable(format!("write registry fragment: {err}")))?;
    fs::write(dir.join("Info.plist"), PLIST_FILE)
        .map_err(|err| ApiError::not_writable(format!("write Info.plist: {err}")))?;
    Ok(())
}

fn refresh_linux_databases(data_home: &Path) {
    let applications = data_home.join("applications");
    let mime = data_home.join("mime");
    let _ = Command::new("update-desktop-database")
        .arg(&applications)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = Command::new("update-mime-database")
        .arg(&mime)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(windows)]
fn import_windows_registry(reg_path: &Path, op: &str) -> Result<()> {
    let status = Command::new("reg")
        .args(["import"])
        .arg(reg_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|err| ApiError::internal(format!("reg {op}: {err}")))?;
    reg_status_to_result(op, status.success())
}
