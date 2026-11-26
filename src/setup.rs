//! A server's setup file (Bingle's ADR 0151): which server to connect to and who to be there,
//! downloaded from the server's own page, so that a client run for the first time starts as
//! that player with nothing to type.
//!
//! ```toml
//! format = 1
//! server = "https://games.example.com"
//! name = "ana"
//! sign_in_code = "bgc_…"             # an account: spent once, for a session
//! # guest_key = "bgg_…"              # or a guest: its key
//! # certificate_sha256 = "AB:CD:…"   # a server with a certificate it made for itself
//! ```
//!
//! It is found by `--setup FILE`, else as [`FILE_NAME`] beside the program, read once at start
//! and taken into the config store: the server, the name and the certificate to `config.toml`,
//! a guest key to `keys.toml`. A sign-in code is not kept anywhere: the network thread spends
//! it for a session as the run's first request (`Request::SetupSignIn`). Either way the client
//! logs in by itself from then on (`auto_login`): a guest at once, an account once signed in.
//!
//! The file holds a credential, so it is deleted once it is used up: at once when it has no
//! sign-in code, and after the code is spent or refused when it has one. A file that cannot be
//! taken (another format, a server that cannot be used, a config that cannot be written) is
//! left where it is, and the player is told. No Bevy in it.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::config::Store;
use crate::net::server_url::ServerUrl;
use crate::net::tls::Fingerprint;

/// The name a server gives the file, and the one looked for beside the program.
pub const FILE_NAME: &str = "bingle-3d-setup.toml";

/// The form of the file this client reads.
pub const FORMAT: i64 = 1;

/// What a setup file says. A key it does not have is empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SetupFile {
    pub format: i64,
    /// The server's address, as the browser that downloaded the file reached it.
    pub server: String,
    /// The account's name.
    pub name: String,
    /// Signs this client in to the account, once.
    pub sign_in_code: String,
    /// The guest to be.
    pub guest_key: String,
    /// The fingerprint of a certificate the server made for itself.
    pub certificate_sha256: String,
}

impl SetupFile {
    /// Reads a file's text, and says in words for the player why it cannot be used.
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: SetupFile = toml::from_str(text).map_err(|e| e.message().to_string())?;
        if file.format != FORMAT {
            return Err(format!(
                "it is of format {}, and this client reads format {FORMAT}: a newer client may \
                 be needed",
                file.format
            ));
        }
        file.origin()?;
        if !file.sign_in_code.is_empty() && !file.guest_key.is_empty() {
            return Err("it names both an account and a guest".into());
        }
        if !file.certificate_sha256.is_empty()
            && Fingerprint::parse(&file.certificate_sha256).is_none()
        {
            return Err("its certificate's fingerprint is not one".into());
        }
        Ok(file)
    }

    /// The server's origin, which the keys and the trusted certificates are kept by.
    fn origin(&self) -> Result<String, String> {
        ServerUrl::parse(&self.server)
            .map(|url| url.origin())
            .map_err(|e| format!("its server \"{}\" cannot be used: {e}", self.server))
    }
}

/// A sign-in code still to be spent, and the file to delete once it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub code: String,
    pub file: PathBuf,
}

/// What became of a run's setup file.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Taken {
    /// A file was taken in: its server is the run's.
    pub taken: bool,
    /// The sign-in code to spend before anything else.
    pub pending: Option<Pending>,
    /// What the player should know: why a file was not taken, or could not be deleted.
    pub notices: Vec<String>,
}

/// The file for this run: the one the command line names, else the one beside the program, if
/// there is one there.
pub fn find(named: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = named {
        return Some(path.to_path_buf());
    }
    let beside = std::env::current_exe().ok()?.parent()?.join(FILE_NAME);
    beside.is_file().then_some(beside)
}

/// Takes the run's setup file, if it has one, into `store`.
pub fn take(store: &mut Store, named: Option<&Path>) -> Taken {
    match find(named) {
        Some(path) => take_file(store, &path),
        None => Taken::default(),
    }
}

/// Takes the file at `path` into `store`, and deletes it unless it has a sign-in code still
/// to spend.
pub fn take_file(store: &mut Store, path: &Path) -> Taken {
    let refused = |why: String| Taken {
        notices: vec![format!(
            "The setup file {} was not used: {why}.",
            path.display()
        )],
        ..Taken::default()
    };
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => return refused(format!("it could not be read ({e})")),
    };
    let file = match SetupFile::parse(&text) {
        Ok(file) => file,
        Err(why) => return refused(why),
    };
    // Nothing is taken that could not be kept: the file would be gone and its player with it.
    if !store.can_keep() {
        return refused("this client's own files cannot be written".into());
    }
    if let Err(e) = keep(store, &file) {
        return refused(format!(
            "this client's own files could not be written ({e})"
        ));
    }
    let mut taken = Taken {
        taken: true,
        ..Taken::default()
    };
    if file.sign_in_code.is_empty() {
        taken.notices.extend(delete(path));
    } else {
        taken.pending = Some(Pending {
            code: file.sign_in_code,
            file: path.to_path_buf(),
        });
    }
    taken
}

/// Writes what the file says into the config and the keys.
fn keep(store: &mut Store, file: &SetupFile) -> std::io::Result<()> {
    let origin = file.origin().map_err(std::io::Error::other)?;
    // A guest's key is all there is to being that guest, so the next run goes straight in. A
    // sign-in code has still to be spent: logging in by itself is kept once it has signed in
    // (the login screen's, as after a sign-in typed there), or a code the server turns down
    // would leave the client going in as somebody else.
    let auto_login = store.config.auto_login || !file.guest_key.is_empty();
    store.remember(Some(&file.server), auto_login)?;
    if !file.name.trim().is_empty() {
        store.set_name(file.name.trim())?;
    }
    if !file.certificate_sha256.is_empty() {
        store.trust(&origin, &file.certificate_sha256)?;
    }
    if !file.guest_key.is_empty() {
        // The file says who to be: a session kept for this server would be somebody else.
        store.set_session_token(&origin, None)?;
        store.set_guest_key(&origin, Some(file.guest_key.clone()))?;
    }
    Ok(())
}

/// Deletes a file that is used up, and says so if it could not be.
pub fn delete(path: &Path) -> Option<String> {
    fs::remove_file(path).err().map(|e| {
        format!(
            "The setup file {} is used up but could not be deleted ({e}): delete it yourself, \
             since it says who you are.",
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory under the system's temporary one, removed when dropped.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let dir = std::env::temp_dir().join(format!(
                "bingle-3d-test-setup-{name}-{}-{nanos}",
                std::process::id(),
            ));
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        /// A config directory in it, and a setup file with this text beside it.
        fn with(name: &str, text: &str) -> (Self, PathBuf, PathBuf) {
            let tmp = TempDir::new(name);
            let config = tmp.0.join("config");
            let file = tmp.0.join(FILE_NAME);
            fs::write(&file, text).unwrap();
            (tmp, config, file)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const FINGERPRINT: &str = "AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89";

    #[test]
    fn an_accounts_file_is_kept_but_for_its_code_which_waits_to_be_spent() {
        let text = format!(
            "# as a server writes it\nformat = 1\nserver = \"https://games.example.com\"\n\
             name = \"ana\"\nsign_in_code = \"bgc_secret\"\ncertificate_sha256 = \"{FINGERPRINT}\"\n"
        );
        let (_tmp, config, file) = TempDir::with("account", &text);
        let mut store = Store::load(Some(config.clone()));
        let taken = take_file(&mut store, &file);
        assert!(taken.taken);
        assert_eq!(taken.notices, Vec::<String>::new());
        assert_eq!(
            taken.pending,
            Some(Pending {
                code: "bgc_secret".into(),
                file: file.clone()
            })
        );
        assert!(
            file.exists(),
            "the code is not spent yet, so the file stays"
        );

        let again = Store::load(Some(config.clone()));
        assert_eq!(again.server(), "https://games.example.com");
        assert_eq!(again.player_name(), "ana");
        assert!(
            !again.config.auto_login,
            "not before the code has signed in"
        );
        assert_eq!(
            again
                .trusted("https://games.example.com:443")
                .collect::<Vec<_>>(),
            [FINGERPRINT]
        );
        for kept in ["config.toml", "keys.toml"] {
            let text = fs::read_to_string(config.join(kept)).unwrap_or_default();
            assert!(!text.contains("bgc_secret"), "the code is in {kept}");
        }
    }

    #[test]
    fn a_guests_file_makes_this_client_that_guest_and_is_deleted() {
        let (_tmp, config, file) = TempDir::with(
            "guest",
            "format = 1\nserver = \"http://127.0.0.1:8080\"\nguest_key = \"bgg_theirs\"\n",
        );
        let origin = "http://127.0.0.1:8080";
        let mut store = Store::load(Some(config.clone()));
        store
            .set_session_token(origin, Some("bgs_someone".into()))
            .unwrap();
        store
            .set_session_token("https://other.example:443", Some("bgs_other".into()))
            .unwrap();
        store.set_name("Ana").unwrap();

        let taken = take_file(&mut store, &file);
        assert!(taken.taken && taken.pending.is_none() && taken.notices.is_empty());
        assert!(!file.exists(), "used up");
        let again = Store::load(Some(config));
        assert_eq!(again.guest_key(origin), Some("bgg_theirs"));
        assert_eq!(again.session_token(origin), None, "the file says who to be");
        assert_eq!(
            again.session_token("https://other.example:443"),
            Some("bgs_other"),
            "another server's is not its business"
        );
        assert!(again.config.auto_login);
        assert_eq!(again.player_name(), "Ana", "a file with no name leaves it");
    }

    #[test]
    fn a_file_that_names_nobody_sets_the_server_and_leaves_the_rest() {
        let (_tmp, config, file) = TempDir::with(
            "nobody",
            "format = 1\nserver = \"https://games.example.com\"\n",
        );
        fs::create_dir_all(&config).unwrap();
        fs::write(
            config.join("config.toml"),
            "server = \"https://old.example\"\nenvironment = \"garden\"\nmax_fps = 30\n\n\
             [[trusted]]\nserver = \"https://box:8443\"\nsha256 = \"AB:CD\"\n",
        )
        .unwrap();
        let mut store = Store::load(Some(config.clone()));
        let taken = take_file(&mut store, &file);
        assert!(taken.taken && taken.pending.is_none());
        assert!(!file.exists());
        let again = Store::load(Some(config));
        assert_eq!(again.server(), "https://games.example.com");
        assert!(!again.config.auto_login, "there is nobody to go in as");
        assert_eq!(again.config.environment.as_deref(), Some("garden"));
        assert_eq!(again.config.max_fps, Some(30));
        assert_eq!(again.trusted("https://box:8443").count(), 1);
    }

    #[test]
    fn a_file_that_cannot_be_used_is_left_where_it_is_and_nothing_is_taken() {
        for (name, text, why) in [
            (
                "format",
                "format = 2\nserver = \"https://a.example\"\n",
                "format 2",
            ),
            ("none", "server = \"https://a.example\"\n", "format 0"),
            (
                "toml",
                "format = 1\nserver = https://a.example\n",
                "was not used",
            ),
            ("server", "format = 1\n", "cannot be used"),
            (
                "plain",
                "format = 1\nserver = \"http://games.example.com\"\n",
                "cannot be used",
            ),
            (
                "both",
                "format = 1\nserver = \"https://a.example\"\nsign_in_code = \"bgc_x\"\nguest_key = \"bgg_y\"\n",
                "both an account and a guest",
            ),
            (
                "fingerprint",
                "format = 1\nserver = \"https://a.example\"\ncertificate_sha256 = \"nonsense\"\n",
                "fingerprint",
            ),
        ] {
            let (_tmp, config, file) = TempDir::with(name, text);
            let mut store = Store::load(Some(config.clone()));
            let taken = take_file(&mut store, &file);
            assert!(!taken.taken && taken.pending.is_none(), "{name}");
            assert_eq!(taken.notices.len(), 1, "{name}");
            assert!(
                taken.notices[0].contains(why),
                "{name}: {}",
                taken.notices[0]
            );
            assert!(file.exists(), "{name}: the file is left");
            assert!(!config.join("config.toml").exists(), "{name}: nothing kept");
        }
        // A file named on the command line that is not there.
        let tmp = TempDir::new("missing");
        let mut store = Store::load(Some(tmp.0.join("config")));
        let taken = take(&mut store, Some(&tmp.0.join("nowhere.toml")));
        assert!(!taken.taken && taken.notices[0].contains("could not be read"));
    }

    #[test]
    fn a_config_that_cannot_be_written_keeps_the_file_and_its_player() {
        let (_tmp, config, file) = TempDir::with(
            "broken",
            "format = 1\nserver = \"https://a.example\"\nguest_key = \"bgg_theirs\"\n",
        );
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join("config.toml"), "server = = broken").unwrap();
        let mut store = Store::load(Some(config.clone()));
        let taken = take_file(&mut store, &file);
        assert!(!taken.taken);
        assert!(taken.notices[0].contains("cannot be written"));
        assert!(file.exists());
        assert!(!config.join("keys.toml").exists());

        // And with no config directory at all.
        let mut nowhere = Store::load(None);
        assert!(!take_file(&mut nowhere, &file).taken);
        assert!(file.exists());
    }

    #[test]
    fn with_no_file_named_and_none_beside_the_program_nothing_happens() {
        let tmp = TempDir::new("none");
        let mut store = Store::load(Some(tmp.0.clone()));
        assert_eq!(take(&mut store, None), Taken::default());
    }
}
