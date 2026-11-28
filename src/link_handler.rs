//! The client registered as the program that opens `bingle-3d://` links (`link.rs`), for the
//! current user only, with no administrator's rights; and that undone.
//!
//! - **Windows**: the scheme under the user's classes, `HKCU\Software\Classes\bingle-3d`, its
//!   command `"<program>" --link "%1"` ([`registry_entries`]).
//! - **Linux**: a `.desktop` entry in the user's applications, `Exec="<program>" --link %u`,
//!   made the scheme's default with `xdg-mime` ([`desktop_entry`]).
//!
//! The link is always one argument after `--link`, quoted so it cannot split, and the client
//! refuses a run with anything after it (`cli.rs`); nothing here or in a link passes through a
//! shell. The Steam Frame has no page to click a link on, so it registers nothing
//! ([`supported`]). What each system is given is a plain function, tested on any system; only
//! the writes are the system's (on Windows, tested in CI).

use std::fmt;
use std::path::{Path, PathBuf};

use crate::link::SCHEME;

/// Whether this build registers links: Windows, and Linux on a PC (not the Steam Frame, whose
/// build is for ARM).
pub const fn supported() -> bool {
    cfg!(windows) || cfg!(all(target_os = "linux", target_arch = "x86_64"))
}

/// Why a registration could not be made or read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    /// The program's own path is not one a registration can name safely.
    Path(String),
    /// The system said no.
    System(String),
    #[cfg_attr(
        any(windows, all(target_os = "linux", target_arch = "x86_64")),
        allow(dead_code, reason = "only a build that registers nothing says so")
    )]
    Unsupported,
}

impl fmt::Display for RegisterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegisterError::Path(why) => {
                write!(f, "this program's path cannot be registered: {why}")
            }
            RegisterError::System(why) => f.write_str(why),
            RegisterError::Unsupported => f.write_str("links are not opened by this build"),
        }
    }
}

/// A program's path as a registration may name it: absolute, and with nothing a quoted command
/// line could read otherwise (a quote, a newline; on Linux also what a `.desktop` entry escapes).
fn checked(exe: &Path, desktop: bool) -> Result<String, RegisterError> {
    if !exe.is_absolute() {
        return Err(RegisterError::Path("it is not absolute".into()));
    }
    let text = exe
        .to_str()
        .ok_or_else(|| RegisterError::Path("it is not Unicode".into()))?;
    let forbidden: &[char] = if desktop {
        &['"', '`', '$', '\\', '%']
    } else {
        &['"', '%']
    };
    if text
        .chars()
        .any(|c| c.is_control() || forbidden.contains(&c))
    {
        return Err(RegisterError::Path(
            "it has a quote, a newline or another character a command line would read".into(),
        ));
    }
    Ok(text.to_string())
}

// ---- Linux ----

/// The `.desktop` entry's file name.
#[cfg_attr(
    not(all(target_os = "linux", target_arch = "x86_64")),
    allow(dead_code, reason = "how Linux registers, tested everywhere")
)]
pub const DESKTOP_FILE: &str = "bingle-3d.desktop";

/// The `.desktop` entry that opens links with `exe`. `%u` is one URL, given as one argument;
/// the path is quoted, and refused if it has anything the entry's quoting escapes.
#[cfg_attr(
    not(all(target_os = "linux", target_arch = "x86_64")),
    allow(dead_code, reason = "how Linux registers, tested everywhere")
)]
pub fn desktop_entry(exe: &Path) -> Result<String, RegisterError> {
    let exe = checked(exe, true)?;
    Ok(format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Bingle 3D\n\
         Comment=Opens {SCHEME}:// game links in {exe}\n\
         Exec=\"{exe}\" --link %u\n\
         NoDisplay=true\n\
         Terminal=false\n\
         MimeType=x-scheme-handler/{SCHEME};\n"
    ))
}

/// The program a `.desktop` entry of ours opens links with, as [`desktop_entry`] writes it.
#[cfg_attr(
    not(all(target_os = "linux", target_arch = "x86_64")),
    allow(dead_code, reason = "how Linux registers, tested everywhere")
)]
pub fn desktop_target(entry: &str) -> Option<PathBuf> {
    entry.lines().find_map(|line| {
        let rest = line.strip_prefix("Exec=\"")?;
        let exe = rest.strip_suffix("\" --link %u")?;
        Some(PathBuf::from(exe))
    })
}

/// The user's applications folder: `$XDG_DATA_HOME/applications`, or
/// `~/.local/share/applications`.
#[cfg_attr(
    not(all(target_os = "linux", target_arch = "x86_64")),
    allow(dead_code, reason = "how Linux registers, tested everywhere")
)]
pub fn applications_dir() -> Option<PathBuf> {
    let env = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    env("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .map(|d| d.join("applications"))
}

// ---- Windows ----

/// One value the registration writes: under `HKEY_CURRENT_USER`, the key, the value's name
/// (`None` the key's default) and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "how Windows registers, tested everywhere")
)]
pub struct RegistryValue {
    pub key: String,
    pub name: Option<&'static str>,
    pub data: String,
}

/// The key a scheme is registered under, for the current user.
#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "how Windows registers, tested everywhere")
)]
pub fn scheme_key(scheme: &str) -> String {
    format!(r"Software\Classes\{scheme}")
}

/// The key that holds the command a link is opened with.
#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "how Windows registers, tested everywhere")
)]
pub fn command_key(scheme: &str) -> String {
    format!(r"{}\shell\open\command", scheme_key(scheme))
}

/// What registering `scheme` to open with `exe` writes. `"%1"` is the link, quoted, so a space
/// in it cannot split it.
#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "how Windows registers, tested everywhere")
)]
pub fn registry_entries(exe: &Path, scheme: &str) -> Result<Vec<RegistryValue>, RegisterError> {
    let exe = checked(exe, false)?;
    let value = |key: String, name, data: String| RegistryValue { key, name, data };
    Ok(vec![
        value(scheme_key(scheme), None, "URL:Bingle 3D game link".into()),
        value(scheme_key(scheme), Some("URL Protocol"), String::new()),
        value(
            format!(r"{}\DefaultIcon", scheme_key(scheme)),
            None,
            format!("\"{exe}\",0"),
        ),
        value(command_key(scheme), None, command(&exe)),
    ])
}

#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "how Windows registers, tested everywhere")
)]
fn command(exe: &str) -> String {
    format!("\"{exe}\" --link \"%1\"")
}

/// The program a command of ours opens links with, as [`registry_entries`] writes it.
#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "how Windows registers, tested everywhere")
)]
pub fn command_target(command: &str) -> Option<PathBuf> {
    let rest = command.strip_prefix('"')?;
    let exe = rest.strip_suffix("\" --link \"%1\"")?;
    (!exe.contains('"')).then(|| PathBuf::from(exe))
}

// ---- Either ----

/// Registers `exe` to open the client's links.
pub fn register(exe: &Path) -> Result<(), RegisterError> {
    register_scheme(exe, SCHEME)
}

/// Takes the registration away.
pub fn unregister() -> Result<(), RegisterError> {
    unregister_scheme(SCHEME)
}

/// The program links open with now, if they are registered to one of ours.
pub fn registered() -> Result<Option<PathBuf>, RegisterError> {
    registered_scheme(SCHEME)
}

/// What a start does about a registration while links are on (`open_links`): a copy moved, or
/// newer and unpacked elsewhere, points the links at itself. Says what it did, if anything.
pub fn keep_pointing_at(exe: &Path) -> Result<Option<String>, RegisterError> {
    let now = registered()?;
    if now.as_deref() == Some(exe) {
        return Ok(None);
    }
    register(exe)?;
    Ok(Some(format!("Game links now open {}.", exe.display())))
}

#[cfg(windows)]
fn register_scheme(exe: &Path, scheme: &str) -> Result<(), RegisterError> {
    for value in registry_entries(exe, scheme)? {
        windows::set(&value)?;
    }
    Ok(())
}

#[cfg(windows)]
fn unregister_scheme(scheme: &str) -> Result<(), RegisterError> {
    windows::delete_tree(&scheme_key(scheme))
}

#[cfg(windows)]
fn registered_scheme(scheme: &str) -> Result<Option<PathBuf>, RegisterError> {
    Ok(windows::get(&command_key(scheme))?.and_then(|c| command_target(&c)))
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn register_scheme(exe: &Path, scheme: &str) -> Result<(), RegisterError> {
    debug_assert_eq!(
        scheme, SCHEME,
        "the entry is written for the client's own scheme"
    );
    let entry = desktop_entry(exe)?;
    let dir = applications_dir()
        .ok_or_else(|| RegisterError::System("no folder for applications was found".into()))?;
    let system = |e: std::io::Error| RegisterError::System(format!("{}: {e}", dir.display()));
    std::fs::create_dir_all(&dir).map_err(system)?;
    let file = dir.join(DESKTOP_FILE);
    let new = dir.join(format!("{DESKTOP_FILE}.new"));
    std::fs::write(&new, entry).map_err(system)?;
    std::fs::rename(&new, &file).map_err(system)?;
    // Fixed arguments, run as a program: no shell, and nothing of a link in it.
    let made_default = std::process::Command::new("xdg-mime")
        .args([
            "default",
            DESKTOP_FILE,
            &format!("x-scheme-handler/{SCHEME}"),
        ])
        .status();
    match made_default {
        Ok(status) if status.success() => {}
        Ok(status) => {
            return Err(RegisterError::System(format!(
                "xdg-mime could not make it the default ({status})"
            )));
        }
        Err(e) => {
            return Err(RegisterError::System(format!(
                "xdg-mime could not be run ({e}): is xdg-utils installed?"
            )));
        }
    }
    // Some desktops read their cache of entries rather than the folder.
    let _ = std::process::Command::new("update-desktop-database")
        .arg(&dir)
        .status();
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn unregister_scheme(scheme: &str) -> Result<(), RegisterError> {
    debug_assert_eq!(scheme, SCHEME);
    let Some(dir) = applications_dir() else {
        return Ok(());
    };
    match std::fs::remove_file(dir.join(DESKTOP_FILE)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(RegisterError::System(format!("{}: {e}", dir.display()))),
    }
    let _ = std::process::Command::new("update-desktop-database")
        .arg(&dir)
        .status();
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn registered_scheme(scheme: &str) -> Result<Option<PathBuf>, RegisterError> {
    debug_assert_eq!(scheme, SCHEME);
    let Some(dir) = applications_dir() else {
        return Ok(None);
    };
    match std::fs::read_to_string(dir.join(DESKTOP_FILE)) {
        Ok(text) => Ok(desktop_target(&text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(RegisterError::System(format!("{}: {e}", dir.display()))),
    }
}

#[cfg(not(any(windows, all(target_os = "linux", target_arch = "x86_64"))))]
fn register_scheme(_: &Path, _: &str) -> Result<(), RegisterError> {
    Err(RegisterError::Unsupported)
}

#[cfg(not(any(windows, all(target_os = "linux", target_arch = "x86_64"))))]
fn unregister_scheme(_: &str) -> Result<(), RegisterError> {
    Err(RegisterError::Unsupported)
}

#[cfg(not(any(windows, all(target_os = "linux", target_arch = "x86_64"))))]
fn registered_scheme(_: &str) -> Result<Option<PathBuf>, RegisterError> {
    Ok(None)
}

/// The registry, under `HKEY_CURRENT_USER` only.
#[cfg(windows)]
mod windows {
    use super::{RegisterError, RegistryValue};
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteTreeW, RegGetValueW, RegSetKeyValueW,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn failed(what: &str, key: &str, code: u32) -> RegisterError {
        RegisterError::System(format!(
            "the registry refused to {what} HKEY_CURRENT_USER\\{key} (error {code})"
        ))
    }

    /// Writes one value, making its key if it is not there.
    pub fn set(value: &RegistryValue) -> Result<(), RegisterError> {
        let key = wide(&value.key);
        let name = value.name.map(wide);
        let data = wide(&value.data);
        let bytes = u32::try_from(data.len() * 2).map_err(|_| failed("write", &value.key, 0))?;
        // SAFETY: every pointer is to a NUL-terminated UTF-16 buffer that lives across the
        // call, and `bytes` is the data's size in bytes, its NUL included.
        let code = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()),
                REG_SZ,
                data.as_ptr().cast(),
                bytes,
            )
        };
        if code == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(failed("write", &value.key, code))
        }
    }

    /// A key's default value, or `None` if it is not there.
    pub fn get(key: &str) -> Result<Option<String>, RegisterError> {
        let wkey = wide(key);
        let mut buf = vec![0u16; 2048];
        let mut bytes = u32::try_from(buf.len() * 2).unwrap_or(u32::MAX);
        // SAFETY: `buf` holds `bytes` bytes, and the key is NUL-terminated; a null value name
        // is the key's default.
        let code = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                wkey.as_ptr(),
                std::ptr::null(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        match code {
            ERROR_SUCCESS => {
                let len = (bytes as usize / 2).min(buf.len());
                let text = &buf[..len];
                let text = text.split(|&c| c == 0).next().unwrap_or_default();
                Ok(Some(String::from_utf16_lossy(text)))
            }
            ERROR_FILE_NOT_FOUND => Ok(None),
            other => Err(failed("read", key, other)),
        }
    }

    /// Deletes a key and everything under it; one not there is already gone.
    pub fn delete_tree(key: &str) -> Result<(), RegisterError> {
        let wkey = wide(key);
        // SAFETY: the key is NUL-terminated, and under HKEY_CURRENT_USER.
        let code = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wkey.as_ptr()) };
        match code {
            ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
            other => Err(failed("delete", key, other)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    const EXE: &str = r"C:\Games\bingle 3d\bingle-3d.exe";
    #[cfg(not(windows))]
    const EXE: &str = "/home/ana/Games/bingle 3d/bingle-3d";

    #[cfg(not(windows))]
    #[test]
    fn the_desktop_entry_quotes_the_program_and_takes_one_url() {
        let exe = Path::new("/home/ana/Games/bingle 3d/bingle-3d");
        let entry = desktop_entry(exe).unwrap();
        assert!(entry.starts_with("[Desktop Entry]\n"));
        assert!(entry.contains("\nExec=\"/home/ana/Games/bingle 3d/bingle-3d\" --link %u\n"));
        assert!(entry.contains("\nMimeType=x-scheme-handler/bingle-3d;\n"));
        assert!(entry.contains("\nNoDisplay=true\n"));
        // One Exec, one field code, the URL's, unquoted as the specification asks: it is
        // given as one argument whatever is in it.
        assert_eq!(entry.matches("Exec=").count(), 1);
        assert_eq!(entry.matches('%').count(), 1);
        assert_eq!(desktop_target(&entry), Some(exe.to_path_buf()));
    }

    #[cfg(not(windows))]
    #[test]
    fn a_path_the_entry_would_have_to_escape_is_refused() {
        for bad in [
            "relative/bingle-3d",
            "/home/ana/\"x\"/bingle-3d",
            "/home/ana/$HOME/bingle-3d",
            "/home/ana/`id`/bingle-3d",
            "/home/ana/back\\slash/bingle-3d",
            "/home/ana/100%/bingle-3d",
            "/home/ana/new\nline/bingle-3d",
        ] {
            assert!(
                matches!(desktop_entry(Path::new(bad)), Err(RegisterError::Path(_))),
                "{bad:?}"
            );
        }
        assert_eq!(desktop_target("Exec=bingle-3d %u"), None, "not one of ours");
    }

    #[test]
    fn the_registry_entries_quote_the_link_as_one_argument() {
        let exe = Path::new(EXE);
        let entries = registry_entries(exe, "bingle-3d").unwrap();
        let get = |key: &str, name: Option<&str>| {
            entries
                .iter()
                .find(|v| v.key == key && v.name == name)
                .map(|v| v.data.clone())
        };
        assert_eq!(
            get(r"Software\Classes\bingle-3d", Some("URL Protocol")),
            Some(String::new())
        );
        assert!(
            get(r"Software\Classes\bingle-3d", None)
                .unwrap()
                .starts_with("URL:")
        );
        let command = get(r"Software\Classes\bingle-3d\shell\open\command", None).unwrap();
        assert_eq!(command, format!("\"{EXE}\" --link \"%1\""));
        assert_eq!(command_target(&command), Some(exe.to_path_buf()));
        assert_eq!(
            get(r"Software\Classes\bingle-3d\DefaultIcon", None),
            Some(format!("\"{EXE}\",0"))
        );
        // Every key is the user's own classes: nothing machine-wide.
        assert!(
            entries
                .iter()
                .all(|v| v.key.starts_with(r"Software\Classes\"))
        );
        assert_eq!(
            command_target("\"a\" --link %1"),
            None,
            "%1 unquoted is not ours"
        );
        assert_eq!(command_target(r#""a" b" --link "%1""#), None);
    }

    #[test]
    fn a_path_with_a_quote_is_refused_in_the_registry_too() {
        #[cfg(windows)]
        let bad = r#"C:\a"b\bingle-3d.exe"#;
        #[cfg(not(windows))]
        let bad = "/a\"b/bingle-3d";
        assert!(matches!(
            registry_entries(Path::new(bad), "bingle-3d"),
            Err(RegisterError::Path(_))
        ));
        assert!(registry_entries(Path::new("bingle-3d.exe"), "bingle-3d").is_err());
    }

    #[test]
    fn the_steam_frame_registers_nothing() {
        assert_eq!(
            supported(),
            cfg!(windows) || cfg!(all(target_os = "linux", target_arch = "x86_64"))
        );
    }

    /// The registry itself, under the current user, with a scheme of the test's own so a real
    /// registration is never touched: written, read back, and taken away again.
    #[cfg(windows)]
    #[test]
    fn the_registry_is_written_read_back_and_cleaned_up() {
        let scheme = format!("bingle-3d-test-{}", std::process::id());
        let exe = std::env::current_exe().unwrap();
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = unregister_scheme(&self.0);
            }
        }
        let _cleanup = Cleanup(scheme.clone());
        assert_eq!(registered_scheme(&scheme).unwrap(), None);
        register_scheme(&exe, &scheme).unwrap();
        assert_eq!(registered_scheme(&scheme).unwrap(), Some(exe.clone()));
        let command = windows::get(&command_key(&scheme)).unwrap().unwrap();
        assert_eq!(command, format!("\"{}\" --link \"%1\"", exe.display()));
        assert_eq!(
            windows::get(&scheme_key(&scheme)).unwrap().as_deref(),
            Some("URL:Bingle 3D game link")
        );
        // Again, pointing somewhere else: the newer registration wins.
        let elsewhere = exe.with_file_name("moved-bingle-3d.exe");
        register_scheme(&elsewhere, &scheme).unwrap();
        assert_eq!(registered_scheme(&scheme).unwrap(), Some(elsewhere));
        unregister_scheme(&scheme).unwrap();
        assert_eq!(registered_scheme(&scheme).unwrap(), None);
        assert_eq!(
            windows::get(&scheme_key(&scheme)).unwrap(),
            None,
            "all of it"
        );
        unregister_scheme(&scheme).unwrap();
    }
}
