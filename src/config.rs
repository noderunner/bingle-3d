//! What the client remembers between runs, in the platform's config directory: `config.toml`
//! (which server, and the certificates the player has trusted), and `keys.toml` beside it (the
//! guest key each server gave this client, and the session of the account signed in there).
//! A server's setup file fills both in at start (`setup.rs`).
//!
//! The keys are kept apart so that `config.toml` can be read, edited and shared without giving a
//! guest or an account away. On Linux the keys file is readable by its owner only.
//!
//! A file that cannot be read is never overwritten: the client says so and runs on defaults, so a
//! typo in a hand-edited config does not lose it.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::graphics::Graphics;

/// Where the client looks first, which is what `cargo run` on Bingle's defaults needs.
pub const DEFAULT_SERVER: &str = "http://127.0.0.1:8080";

/// The name the player is shown by at a table, unless `config.toml` gives one.
pub const DEFAULT_NAME: &str = "Player";

/// Overrides the directory, for tests and for keeping a development setup apart.
pub const DIR_VAR: &str = "BINGLE3D_CONFIG_DIR";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// The server to connect to, such as `https://games.example.com` or `localhost:8080`.
    pub server: Option<String>,
    /// The name the player is shown by at a table.
    pub name: Option<String>,
    /// Skip the login screen at start: connect to `server` with the session or guest key kept
    /// in `keys.toml`. Never a password: none is kept.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub auto_login: bool,
    /// Where the table stands: an environment's id (`assets/environments/`). The plain table
    /// when there is none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    /// The most frames a second the window draws, 0 for the monitor's pace
    /// (`frame_pace::DEFAULT_MAX_FPS` when there is none).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_fps: Option<u32>,
    /// Whether the window covers its monitor (`display.rs`); true when there is none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fullscreen: Option<bool>,
    /// The resolution, as `1920x1080`: in fullscreen the monitor's mode, in a window its size;
    /// the desktop's (in a window, the player's sizing) when there is none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    /// How much ambient occlusion darkens the creases: `off`, `low` or `high` (`graphics.rs`);
    /// `high` when there is none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ambient_occlusion: Option<String>,
    /// How edges are smoothed: `off`, `fxaa`, `smaa`, `taa` or `msaa`; `taa` when there is none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anti_aliasing: Option<String>,
    /// Whether the far side of the table is softened; true when there is none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth_of_field: Option<bool>,
    /// Whether the highlights glow; true when there is none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bloom: Option<bool>,
    /// How sharp the sun's shadows are: `off`, `low`, `medium` or `high`; `medium` when there
    /// is none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadows: Option<String>,
    /// Whether the shadows under a stone and in a cut line are drawn; true when there is none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact_shadows: Option<bool>,
    /// Whether a timed game of two seats has a chess clock beside the board (`chess_clock.rs`);
    /// true when there is none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chess_clock: Option<bool>,
    /// The display rate to ask a headset for, in Hz (72, 90, 120…), where its runtime lets the
    /// client choose; the runtime's own when there is none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vr_refresh_rate: Option<u32>,
    /// The private folder of games' looks this client may not ship (`assets::read_private`):
    /// a game the server marks private is shown only when the folder has its look.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_dir: Option<PathBuf>,
    /// Certificates the player has said are their server's, by fingerprint.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub trusted: Vec<TrustedCertificate>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrustedCertificate {
    /// The server's origin, `https://host:port`.
    pub server: String,
    /// The certificate's SHA-256, as `AB:CD:…`.
    pub sha256: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Keys {
    /// A guest key per server origin.
    pub guest: BTreeMap<String, String>,
    /// The session token (`bgs_…`) of the account signed in, per server origin.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub session: BTreeMap<String, String>,
}

/// The two files, read at start-up and written when something worth remembering changes.
#[derive(Debug)]
pub struct Store {
    dir: Option<PathBuf>,
    pub config: Config,
    pub keys: Keys,
    /// Files that could not be read, and are therefore left alone.
    problems: Vec<String>,
    config_writable: bool,
    keys_writable: bool,
}

impl Store {
    /// The platform's directory: `$XDG_CONFIG_HOME/bingle-3d` (or `~/.config/bingle-3d`) on Linux,
    /// `%APPDATA%\bingle-3d` on Windows, or the one `BINGLE3D_CONFIG_DIR` names.
    pub fn default_dir() -> Option<PathBuf> {
        let env = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
        if let Some(dir) = env(DIR_VAR) {
            return Some(PathBuf::from(dir));
        }
        let base = if cfg!(windows) {
            env("APPDATA").map(PathBuf::from)
        } else if cfg!(target_os = "macos") {
            env("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
        } else {
            env("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| env("HOME").map(|h| PathBuf::from(h).join(".config")))
        };
        base.map(|b| b.join("bingle-3d"))
    }

    /// Reads what is there. A missing file is the same as an empty one.
    pub fn load(dir: Option<PathBuf>) -> Self {
        let mut store = Store {
            dir,
            config: Config::default(),
            keys: Keys::default(),
            problems: Vec::new(),
            config_writable: true,
            keys_writable: true,
        };
        let Some(dir) = store.dir.clone() else {
            store
                .problems
                .push("no config directory was found, so nothing will be remembered".into());
            store.config_writable = false;
            store.keys_writable = false;
            return store;
        };
        match read_toml(&dir.join("config.toml")) {
            Ok(c) => store.config = c.unwrap_or_default(),
            Err(e) => {
                store.problems.push(e);
                store.config_writable = false;
            }
        }
        match read_toml(&dir.join("keys.toml")) {
            Ok(k) => store.keys = k.unwrap_or_default(),
            Err(e) => {
                store.problems.push(e);
                store.keys_writable = false;
            }
        }
        store
    }

    /// Files that could not be read, in words for the player.
    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    pub fn server(&self) -> &str {
        self.config.server.as_deref().unwrap_or(DEFAULT_SERVER)
    }

    /// Remembers the server to connect to, and whether to skip the login screen next time.
    /// `server` `None` leaves the one remembered.
    pub fn remember(&mut self, server: Option<&str>, auto_login: bool) -> io::Result<()> {
        let server = server
            .map(str::to_string)
            .or_else(|| self.config.server.clone());
        if self.config.server == server && self.config.auto_login == auto_login {
            return Ok(());
        }
        self.config.server = server;
        self.config.auto_login = auto_login;
        self.save_config()
    }

    /// Remembers where the table stands.
    pub fn remember_environment(&mut self, id: &str) -> io::Result<()> {
        if self.config.environment.as_deref() == Some(id) {
            return Ok(());
        }
        self.config.environment = Some(id.to_string());
        self.save_config()
    }

    /// Remembers how the window stands: fullscreen or not, and at which resolution.
    pub fn remember_display(
        &mut self,
        fullscreen: bool,
        resolution: Option<String>,
    ) -> io::Result<()> {
        if self.config.fullscreen == Some(fullscreen) && self.config.resolution == resolution {
            return Ok(());
        }
        self.config.fullscreen = Some(fullscreen);
        self.config.resolution = resolution;
        self.save_config()
    }

    /// Remembers which effects are on (`graphics.rs`). With `keep_bloom`, whether it blooms is
    /// left as it was kept: a run started with `--no-bloom` has it off for the run only.
    pub fn remember_graphics(&mut self, graphics: &Graphics, keep_bloom: bool) -> io::Result<()> {
        let bloom = if keep_bloom {
            self.config.bloom
        } else {
            Some(graphics.bloom)
        };
        let [ambient_occlusion, anti_aliasing, shadows] = graphics.names().map(|n| Some(n.into()));
        let kept = (
            &ambient_occlusion,
            &anti_aliasing,
            &Some(graphics.depth_of_field),
            &bloom,
            &shadows,
            &Some(graphics.contact_shadows),
        );
        let now = (
            &self.config.ambient_occlusion,
            &self.config.anti_aliasing,
            &self.config.depth_of_field,
            &self.config.bloom,
            &self.config.shadows,
            &self.config.contact_shadows,
        );
        if kept == now {
            return Ok(());
        }
        self.config.ambient_occlusion = ambient_occlusion;
        self.config.anti_aliasing = anti_aliasing;
        self.config.depth_of_field = Some(graphics.depth_of_field);
        self.config.bloom = bloom;
        self.config.shadows = shadows;
        self.config.contact_shadows = Some(graphics.contact_shadows);
        self.save_config()
    }

    /// Remembers whether a timed game of two seats has a chess clock on the table.
    pub fn remember_chess_clock(&mut self, on: bool) -> io::Result<()> {
        if self.config.chess_clock == Some(on) {
            return Ok(());
        }
        self.config.chess_clock = Some(on);
        self.save_config()
    }

    /// Remembers the most frames a second to draw, 0 for the monitor's pace.
    pub fn remember_max_fps(&mut self, fps: u32) -> io::Result<()> {
        if self.config.max_fps == Some(fps) {
            return Ok(());
        }
        self.config.max_fps = Some(fps);
        self.save_config()
    }

    /// Whether what is remembered can be written down: there is a config directory, and both
    /// files in it could be read (one that could not is never overwritten). A setup file is
    /// taken only then (`setup.rs`), or it would be used up and nothing kept of it.
    pub fn can_keep(&self) -> bool {
        self.dir.is_some() && self.config_writable && self.keys_writable
    }

    /// Remembers the name to sit down under.
    pub fn set_name(&mut self, name: &str) -> io::Result<()> {
        if self.config.name.as_deref() == Some(name) {
            return Ok(());
        }
        self.config.name = Some(name.to_string());
        self.save_config()
    }

    /// The name to sit down under: the configured one, if it has anything but spaces in it.
    pub fn player_name(&self) -> &str {
        self.config
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .unwrap_or(DEFAULT_NAME)
    }

    pub fn trusted(&self, origin: &str) -> impl Iterator<Item = &str> {
        self.config
            .trusted
            .iter()
            .filter(move |t| t.server == origin)
            .map(|t| t.sha256.as_str())
    }

    pub fn trust(&mut self, origin: &str, sha256: &str) -> io::Result<()> {
        if self.trusted(origin).any(|t| t == sha256) {
            return Ok(());
        }
        self.config.trusted.push(TrustedCertificate {
            server: origin.to_string(),
            sha256: sha256.to_string(),
        });
        self.save_config()
    }

    pub fn guest_key(&self, origin: &str) -> Option<&str> {
        self.keys.guest.get(origin).map(String::as_str)
    }

    pub fn set_guest_key(&mut self, origin: &str, key: Option<String>) -> io::Result<()> {
        match key {
            Some(k) => self.keys.guest.insert(origin.to_string(), k),
            None => self.keys.guest.remove(origin),
        };
        self.save_keys()
    }

    /// The token of the account signed in to this server, if one is.
    pub fn session_token(&self, origin: &str) -> Option<&str> {
        self.keys.session.get(origin).map(String::as_str)
    }

    pub fn set_session_token(&mut self, origin: &str, token: Option<String>) -> io::Result<()> {
        match token {
            Some(t) => self.keys.session.insert(origin.to_string(), t),
            None => self.keys.session.remove(origin),
        };
        self.save_keys()
    }

    fn save_config(&self) -> io::Result<()> {
        if !self.config_writable {
            return Ok(());
        }
        let Some(dir) = &self.dir else { return Ok(()) };
        let text = toml::to_string(&self.config).map_err(io::Error::other)?;
        write_atomically(&dir.join("config.toml"), &text, false)
    }

    fn save_keys(&self) -> io::Result<()> {
        if !self.keys_writable {
            return Ok(());
        }
        let Some(dir) = &self.dir else { return Ok(()) };
        let text = format!(
            "# Guest keys and sign-ins this client was given, one per server. Keep this file to \
             yourself.\n{}",
            toml::to_string(&self.keys).map_err(io::Error::other)?
        );
        write_atomically(&dir.join("keys.toml"), &text, true)
    }
}

fn read_toml<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>, String> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{} could not be read: {e}", path.display())),
    };
    toml::from_str(&text).map(Some).map_err(|e| {
        format!(
            "{} could not be read, and is left as it is: {}",
            path.display(),
            e.message()
        )
    })
}

/// Writes a sibling file and renames it over the old one, so a crash never leaves half a file.
fn write_atomically(path: &Path, text: &str, private: bool) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("toml.tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = private;
    {
        use std::io::Write;
        let mut file = options.open(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory under the system's temporary one, removed when dropped.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            // Only digits and dashes: Windows refuses some characters in names (`:`, `{`).
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let dir = std::env::temp_dir().join(format!(
                "bingle-3d-test-{name}-{}-{nanos}",
                std::process::id(),
            ));
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_environment_is_remembered() {
        let tmp = TempDir::new("environment");
        let mut store = Store::load(Some(tmp.0.clone()));
        assert_eq!(store.config.environment, None);
        store.remember_environment("garden").unwrap();
        let again = Store::load(Some(tmp.0.clone()));
        assert_eq!(again.config.environment.as_deref(), Some("garden"));
    }

    #[test]
    fn the_display_is_remembered() {
        let tmp = TempDir::new("display");
        let mut store = Store::load(Some(tmp.0.clone()));
        assert_eq!(store.config.fullscreen, None);
        store
            .remember_display(false, Some("1920x1080".into()))
            .unwrap();
        let mut again = Store::load(Some(tmp.0.clone()));
        assert_eq!(again.config.fullscreen, Some(false));
        assert_eq!(again.config.resolution.as_deref(), Some("1920x1080"));
        again.remember_display(true, None).unwrap();
        let again = Store::load(Some(tmp.0.clone()));
        assert_eq!(again.config.fullscreen, Some(true));
        assert_eq!(again.config.resolution, None);
    }

    #[test]
    fn the_graphics_and_the_frame_limit_are_remembered() {
        use crate::graphics::{AmbientOcclusion, AntiAliasing, Shadows};
        let tmp = TempDir::new("graphics");
        let mut store = Store::load(Some(tmp.0.clone()));
        assert_eq!(store.config.anti_aliasing, None);
        let graphics = Graphics {
            ambient_occlusion: AmbientOcclusion::Low,
            anti_aliasing: AntiAliasing::Smaa,
            depth_of_field: false,
            bloom: false,
            shadows: Shadows::High,
            contact_shadows: false,
        };
        store.remember_graphics(&graphics, false).unwrap();
        store.remember_max_fps(0).unwrap();
        let again = Store::load(Some(tmp.0.clone()));
        let c = &again.config;
        let (read, problems) = Graphics::read(
            c.ambient_occlusion.as_deref(),
            c.anti_aliasing.as_deref(),
            c.depth_of_field,
            c.bloom,
            c.shadows.as_deref(),
            c.contact_shadows,
        );
        assert!(problems.is_empty());
        assert_eq!(read, graphics);
        assert_eq!(c.max_fps, Some(0));
        let text = fs::read_to_string(tmp.0.join("config.toml")).unwrap();
        assert!(text.contains("anti_aliasing = \"smaa\""), "{text}");
        assert!(text.contains("bloom = false"), "{text}");
    }

    #[test]
    fn whether_the_chess_clock_stands_is_kept() {
        let tmp = TempDir::new("chess-clock");
        let mut store = Store::load(Some(tmp.0.clone()));
        assert_eq!(store.config.chess_clock, None, "on, when nothing is kept");
        store.remember_chess_clock(false).unwrap();
        let text = std::fs::read_to_string(tmp.0.join("config.toml")).unwrap();
        assert!(text.contains("chess_clock = false"), "{text}");
        assert_eq!(
            Store::load(Some(tmp.0.clone())).config.chess_clock,
            Some(false)
        );
    }

    #[test]
    fn a_bloom_held_back_is_left_as_it_was_kept() {
        let tmp = TempDir::new("bloom");
        let mut store = Store::load(Some(tmp.0.clone()));
        let off = Graphics {
            bloom: false,
            ..Graphics::default()
        };
        // Nothing kept: another row's change leaves bloom unwritten.
        store.remember_graphics(&off, true).unwrap();
        assert_eq!(Store::load(Some(tmp.0.clone())).config.bloom, None);
        assert_eq!(
            Store::load(Some(tmp.0.clone()))
                .config
                .anti_aliasing
                .as_deref(),
            Some("taa"),
            "the rest is kept"
        );
        // Kept on: stays on.
        store
            .remember_graphics(&Graphics::default(), false)
            .unwrap();
        store.remember_graphics(&off, true).unwrap();
        assert_eq!(Store::load(Some(tmp.0.clone())).config.bloom, Some(true));
        store.remember_graphics(&off, false).unwrap();
        assert_eq!(Store::load(Some(tmp.0.clone())).config.bloom, Some(false));
    }

    #[test]
    fn nothing_there_means_defaults() {
        let tmp = TempDir::new("empty");
        let store = Store::load(Some(tmp.0.join("missing")));
        assert_eq!(store.server(), DEFAULT_SERVER);
        assert!(store.problems().is_empty());
    }

    #[test]
    fn keys_and_trust_are_remembered_in_their_own_files() {
        let tmp = TempDir::new("roundtrip");
        let dir = tmp.0.join("bingle-3d");
        let mut store = Store::load(Some(dir.clone()));
        store
            .set_guest_key("http://127.0.0.1:8080", Some("bgg_abc".into()))
            .unwrap();
        store.trust("https://box:8443", "AB:CD").unwrap();
        store.trust("https://box:8443", "AB:CD").unwrap();
        store
            .set_session_token("https://box:8443", Some("bgs_xyz".into()))
            .unwrap();

        let again = Store::load(Some(dir.clone()));
        assert_eq!(again.guest_key("http://127.0.0.1:8080"), Some("bgg_abc"));
        assert_eq!(again.session_token("https://box:8443"), Some("bgs_xyz"));
        assert_eq!(again.session_token("http://127.0.0.1:8080"), None);
        assert_eq!(
            again.trusted("https://box:8443").collect::<Vec<_>>(),
            ["AB:CD"]
        );
        assert_eq!(again.trusted("https://other:8443").count(), 0);

        let config = fs::read_to_string(dir.join("config.toml")).unwrap();
        assert!(
            !config.contains("bgg_") && !config.contains("bgs_"),
            "no key in config.toml: {config}"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.join("keys.toml"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0, "keys.toml is private");
        }

        let mut again = again;
        again.set_guest_key("http://127.0.0.1:8080", None).unwrap();
        assert_eq!(
            Store::load(Some(dir)).guest_key("http://127.0.0.1:8080"),
            None
        );
    }

    #[test]
    fn the_server_and_auto_login_are_remembered_and_no_password() {
        let tmp = TempDir::new("remember");
        let mut store = Store::load(Some(tmp.0.clone()));
        assert!(
            !store.config.auto_login,
            "off unless the player turns it on"
        );
        store
            .remember(Some("https://games.example.com"), true)
            .unwrap();
        let mut again = Store::load(Some(tmp.0.clone()));
        assert_eq!(again.server(), "https://games.example.com");
        assert!(again.config.auto_login);
        // Logging out turns it off and keeps the server.
        again.remember(None, false).unwrap();
        let again = Store::load(Some(tmp.0.clone()));
        assert_eq!(again.server(), "https://games.example.com");
        assert!(!again.config.auto_login);
        let config = fs::read_to_string(tmp.0.join("config.toml")).unwrap();
        assert!(
            !config.contains("auto_login"),
            "off is the default: {config}"
        );
    }

    #[test]
    fn a_file_that_cannot_be_read_is_reported_and_left_alone() {
        let tmp = TempDir::new("broken");
        let broken = "server = \"https://games.example.com\n";
        fs::write(tmp.0.join("config.toml"), broken).unwrap();
        let mut store = Store::load(Some(tmp.0.clone()));
        assert_eq!(store.problems().len(), 1, "{:?}", store.problems());
        assert!(store.problems()[0].contains("config.toml"));
        assert_eq!(store.server(), DEFAULT_SERVER);

        store.trust("https://box:8443", "AB:CD").unwrap();
        assert_eq!(
            fs::read_to_string(tmp.0.join("config.toml")).unwrap(),
            broken
        );
    }

    #[test]
    fn a_hand_written_config_reads() {
        let tmp = TempDir::new("hand");
        fs::write(
            tmp.0.join("config.toml"),
            "server = \"https://games.example.com\"\nname = \" Ana \"\n\n[[trusted]]\nserver = \"https://box:8443\"\nsha256 = \"AB:CD\"\n",
        )
        .unwrap();
        let store = Store::load(Some(tmp.0.clone()));
        assert!(store.problems().is_empty(), "{:?}", store.problems());
        assert_eq!(store.server(), "https://games.example.com");
        assert_eq!(store.trusted("https://box:8443").count(), 1);
        assert_eq!(store.player_name(), "Ana");
        assert_eq!(Store::load(None).player_name(), DEFAULT_NAME);
    }
}
