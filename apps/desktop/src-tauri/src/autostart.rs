//! Cross-platform "start on login" integration, dependency-free.
//!
//! - **Linux:** writes/removes an XDG autostart entry at
//!   `~/.config/autostart/nexo.desktop` (honored by all major desktops on both
//!   X11 and Wayland). This is the standard, session-manager-agnostic mechanism.
//! - **Windows:** sets/clears a `HKCU\...\Run` value via `reg.exe`.
//! - **Other:** no-op.
//!
//! The core enable/disable/query logic is written against an injected config
//! directory so it is unit-testable without touching the real user profile.

use std::path::{Path, PathBuf};

const LINUX_ENTRY: &str = "nexo.desktop";
#[cfg(windows)]
const WINDOWS_RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const WINDOWS_VALUE: &str = "Nexo";

/// Enables or disables launch-on-login for the current user, using the path to
/// this executable. Returns `Ok(())` on success (including unsupported
/// platforms, which are a no-op).
pub fn set_enabled(enabled: bool, exe_path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let Some(dir) = linux_autostart_dir() else {
            return Ok(());
        };
        linux_set_enabled(enabled, exe_path, &dir)
    }
    #[cfg(windows)]
    {
        windows_set_enabled(enabled, exe_path)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = (enabled, exe_path);
        Ok(())
    }
}

/// Whether launch-on-login is currently enabled for the current user. Exposed
/// for callers that want to reconcile OS state with the stored preference.
#[allow(dead_code)]
pub fn is_enabled() -> bool {
    #[cfg(target_os = "linux")]
    {
        linux_autostart_dir()
            .map(|dir| dir.join(LINUX_ENTRY).is_file())
            .unwrap_or(false)
    }
    #[cfg(windows)]
    {
        windows_is_enabled()
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        false
    }
}

// ---- Linux ---------------------------------------------------------------

#[cfg(target_os = "linux")]
fn linux_autostart_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("autostart"))
}

/// Renders the XDG desktop entry contents for autostart.
fn linux_desktop_entry(exec: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Nexo\n\
         Comment=Encrypted peer-to-peer file transfer\n\
         Exec={exec} --hidden\n\
         Icon=nexo\n\
         Terminal=false\n\
         Categories=Utility;Network;FileTransfer;\n\
         X-GNOME-Autostart-enabled=true\n"
    )
}

/// Testable core: create or remove the entry in `dir`.
fn linux_apply(enabled: bool, exe_path: &Path, dir: &Path) -> std::io::Result<()> {
    let entry = dir.join(LINUX_ENTRY);
    if enabled {
        std::fs::create_dir_all(dir)?;
        std::fs::write(&entry, linux_desktop_entry(&exe_path.display().to_string()))?;
    } else if entry.exists() {
        std::fs::remove_file(&entry)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_set_enabled(enabled: bool, exe_path: &Path, dir: &Path) -> std::io::Result<()> {
    linux_apply(enabled, exe_path, dir)
}

// ---- Windows -------------------------------------------------------------

#[cfg(windows)]
fn windows_set_enabled(enabled: bool, exe_path: &Path) -> std::io::Result<()> {
    use std::process::Command;
    if enabled {
        let value = format!("\"{}\" --hidden", exe_path.display());
        Command::new("reg")
            .args([
                "add",
                WINDOWS_RUN_KEY,
                "/v",
                WINDOWS_VALUE,
                "/t",
                "REG_SZ",
                "/d",
                &value,
                "/f",
            ])
            .status()?;
    } else {
        // Ignore "not found" on delete.
        let _ = Command::new("reg")
            .args(["delete", WINDOWS_RUN_KEY, "/v", WINDOWS_VALUE, "/f"])
            .status();
    }
    Ok(())
}

#[cfg(windows)]
fn windows_is_enabled() -> bool {
    use std::process::Command;
    Command::new("reg")
        .args(["query", WINDOWS_RUN_KEY, "/v", WINDOWS_VALUE])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entry_has_required_keys() {
        let entry = linux_desktop_entry("/opt/nexo/nexo");
        assert!(entry.contains("[Desktop Entry]"));
        assert!(entry.contains("Type=Application"));
        assert!(entry.contains("Exec=/opt/nexo/nexo --hidden"));
        assert!(entry.contains("X-GNOME-Autostart-enabled=true"));
    }

    #[test]
    fn linux_apply_creates_and_removes_entry() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("nexo-autostart-{}-{unique}", std::process::id()));
        let exe = Path::new("/usr/bin/nexo-desktop");
        let entry = dir.join(LINUX_ENTRY);

        linux_apply(true, exe, &dir).expect("enable");
        assert!(entry.is_file(), "entry created on enable");
        let contents = std::fs::read_to_string(&entry).expect("read");
        assert!(contents.contains("Exec=/usr/bin/nexo-desktop --hidden"));

        // Idempotent enable.
        linux_apply(true, exe, &dir).expect("re-enable");
        assert!(entry.is_file());

        linux_apply(false, exe, &dir).expect("disable");
        assert!(!entry.exists(), "entry removed on disable");

        // Disable when absent is a no-op, not an error.
        linux_apply(false, exe, &dir).expect("disable-again");

        std::fs::remove_dir_all(&dir).ok();
    }
}
