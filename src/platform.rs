//! OS integration: opening folders / URLs, "start with Windows" and the
//! magnet / .torrent handler registration (HKCU only, no admin rights).

use std::path::Path;

pub const WEBSITE: &str = "https://zenless-suite.vercel.app";
pub const DOWNLOAD_PAGE: &str = "https://zenless-suite.vercel.app/download";
pub const REPO: &str = "https://github.com/zenless-inc/zenless-torrent-client";

/// Opens a folder in Explorer (falls back to the closest existing parent).
pub fn open_folder(path: &Path) {
    let mut target = path;
    while !target.exists() {
        match target.parent() {
            Some(p) => target = p,
            None => return,
        }
    }
    spawn_opener(target.as_os_str());
}

/// Opens Explorer with `path` selected.
pub fn reveal(path: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        if path.exists() {
            let _ = std::process::Command::new("explorer")
                .raw_arg(format!("/select,\"{}\"", path.display()))
                .spawn();
            return;
        }
    }
    open_folder(path);
}

/// Opens a URL (https:, ms-settings:, …) with the default handler.
pub fn open_url(url: &str) {
    spawn_opener(std::ffi::OsStr::new(url));
}

fn spawn_opener(target: &std::ffi::OsStr) {
    #[cfg(windows)]
    let cmd = "explorer";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let cmd = "xdg-open";
    if let Err(e) = std::process::Command::new(cmd).arg(target).spawn() {
        tracing::warn!("could not open {target:?}: {e}");
    }
}

#[cfg(windows)]
mod win {
    use std::io;
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const RUN_VALUE: &str = "ZenlessTorrent";
    const PROG_ID: &str = "Zenless.Torrent";
    const MAGNET_PROG_ID: &str = "Zenless.Magnet";
    const CAPABILITIES: &str = r"Software\Zenless\Torrent\Capabilities";

    fn exe() -> io::Result<String> {
        Ok(std::env::current_exe()?.display().to_string())
    }

    fn hkcu() -> RegKey {
        RegKey::predef(HKEY_CURRENT_USER)
    }

    pub fn set_autostart(enable: bool, minimized: bool) -> io::Result<()> {
        let (key, _) = hkcu().create_subkey(RUN_KEY)?;
        if enable {
            let exe = exe()?;
            let value = if minimized {
                format!("\"{exe}\" --minimized")
            } else {
                format!("\"{exe}\"")
            };
            key.set_value(RUN_VALUE, &value)
        } else {
            match key.delete_value(RUN_VALUE) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            }
        }
    }

    pub fn autostart_enabled() -> bool {
        hkcu()
            .open_subkey(RUN_KEY)
            .and_then(|k| k.get_value::<String, _>(RUN_VALUE))
            .is_ok()
    }

    fn set_default(path: &str, value: &str) -> io::Result<RegKey> {
        let (key, _) = hkcu().create_subkey(path)?;
        key.set_value("", &value.to_owned())?;
        Ok(key)
    }

    /// Writes the magnet: protocol and .torrent file associations for this exe,
    /// plus "Capabilities" so Zenless Torrent is listed in Windows' Default apps.
    pub fn register_handlers() -> io::Result<()> {
        let exe = exe()?;
        let icon = format!("\"{exe}\",0");
        let command = format!("\"{exe}\" \"%1\"");

        // magnet: protocol
        let magnet = set_default(r"Software\Classes\magnet", "URL:Magnet Link")?;
        magnet.set_value("URL Protocol", &String::new())?;
        set_default(r"Software\Classes\magnet\DefaultIcon", &icon)?;
        set_default(r"Software\Classes\magnet\shell\open\command", &command)?;

        // .torrent files
        let ext = set_default(r"Software\Classes\.torrent", PROG_ID)?;
        ext.set_value("Content Type", &"application/x-bittorrent".to_owned())?;
        let (open_with, _) = hkcu().create_subkey(r"Software\Classes\.torrent\OpenWithProgids")?;
        open_with.set_value(PROG_ID, &String::new())?;
        set_default(&format!(r"Software\Classes\{PROG_ID}"), "Torrent file")?;
        set_default(&format!(r"Software\Classes\{PROG_ID}\DefaultIcon"), &icon)?;
        set_default(&format!(r"Software\Classes\{PROG_ID}\shell\open\command"), &command)?;

        // ProgID for the magnet protocol (used by Default apps).
        let mp = set_default(&format!(r"Software\Classes\{MAGNET_PROG_ID}"), "Magnet link")?;
        mp.set_value("URL Protocol", &String::new())?;
        set_default(&format!(r"Software\Classes\{MAGNET_PROG_ID}\DefaultIcon"), &icon)?;
        set_default(&format!(r"Software\Classes\{MAGNET_PROG_ID}\shell\open\command"), &command)?;

        // Registered application capabilities.
        let (caps, _) = hkcu().create_subkey(CAPABILITIES)?;
        caps.set_value("ApplicationName", &"Zenless Torrent".to_owned())?;
        caps.set_value("ApplicationDescription", &"A calm, fast BitTorrent client".to_owned())?;
        caps.set_value("ApplicationIcon", &icon)?;
        let (fa, _) = hkcu().create_subkey(format!(r"{CAPABILITIES}\FileAssociations"))?;
        fa.set_value(".torrent", &PROG_ID.to_owned())?;
        let (ua, _) = hkcu().create_subkey(format!(r"{CAPABILITIES}\URLAssociations"))?;
        ua.set_value("magnet", &MAGNET_PROG_ID.to_owned())?;
        let (reg, _) = hkcu().create_subkey(r"Software\RegisteredApplications")?;
        reg.set_value("Zenless Torrent", &CAPABILITIES.to_owned())?;
        Ok(())
    }

    /// True when the magnet: handler points at this exe.
    pub fn handlers_registered() -> bool {
        let Ok(exe) = exe() else { return false };
        hkcu()
            .open_subkey(r"Software\Classes\magnet\shell\open\command")
            .and_then(|k| k.get_value::<String, _>(""))
            .is_ok_and(|v| v.to_ascii_lowercase().contains(&exe.to_ascii_lowercase()))
    }
}

#[cfg(windows)]
pub use win::{autostart_enabled, handlers_registered, register_handlers, set_autostart};

#[cfg(not(windows))]
mod other {
    use std::io;
    fn unsupported() -> io::Error {
        io::Error::new(io::ErrorKind::Unsupported, "only supported on Windows")
    }
    pub fn set_autostart(_enable: bool, _minimized: bool) -> io::Result<()> {
        Err(unsupported())
    }
    pub fn autostart_enabled() -> bool {
        false
    }
    pub fn register_handlers() -> io::Result<()> {
        Err(unsupported())
    }
    pub fn handlers_registered() -> bool {
        false
    }
}

#[cfg(not(windows))]
pub use other::{autostart_enabled, handlers_registered, register_handlers, set_autostart};
