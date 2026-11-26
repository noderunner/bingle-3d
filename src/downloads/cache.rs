//! Where downloaded looks are kept: a folder in the platform's cache directory, with each file
//! stored by its SHA-256, so that a file shared by two variants, two versions or two servers is
//! fetched once. Beside them, the index each server last gave for each game. The folder has a
//! cap, and what was used longest ago goes first.
//!
//! ```text
//! <cache>/files/<sha256>        a file, checked
//! <cache>/files/<sha256>.part   one being fetched, resumed from where it stopped
//! <cache>/indexes/<server>/<game>.json
//! ```
//!
//! A file is checked again each time it is read (its hash, and `check::contents`), so a file
//! changed on the disk, or kept by a client whose limits were looser, is never used.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::check::{self, Kind};
use crate::net::api::ClientIndex;

/// The variable that names the cache's folder, in place of the platform's.
pub const DIR_VAR: &str = "BINGLE3D_CACHE_DIR";

/// How many bytes the cache keeps before it drops what was used longest ago.
pub const CAP: u64 = 2 << 30;

pub struct Cache {
    dir: PathBuf,
    cap: u64,
}

/// The SHA-256 of some bytes, in lower-case hex.
pub fn sha256(bytes: &[u8]) -> String {
    hex(ring::digest::digest(&ring::digest::SHA256, bytes).as_ref())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether a string is a SHA-256 in lower-case hex, and so a file's name here.
fn is_hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Cache {
    /// The platform's cache directory: `$XDG_CACHE_HOME/bingle-3d` (or `~/.cache/bingle-3d`) on
    /// Linux, `%LOCALAPPDATA%\bingle-3d\cache` on Windows, or the one `BINGLE3D_CACHE_DIR` names;
    /// the system's temporary folder when there is none of those.
    pub fn default_dir() -> PathBuf {
        let env = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
        if let Some(dir) = env(DIR_VAR) {
            return PathBuf::from(dir);
        }
        let dir = if cfg!(windows) {
            env("LOCALAPPDATA").map(|d| PathBuf::from(d).join("bingle-3d").join("cache"))
        } else if cfg!(target_os = "macos") {
            env("HOME").map(|h| PathBuf::from(h).join("Library/Caches/bingle-3d"))
        } else {
            env("XDG_CACHE_HOME")
                .map(PathBuf::from)
                .or_else(|| env("HOME").map(|h| PathBuf::from(h).join(".cache")))
                .map(|d| d.join("bingle-3d"))
        };
        dir.unwrap_or_else(|| std::env::temp_dir().join("bingle-3d-cache"))
    }

    pub fn new(dir: PathBuf, cap: u64) -> Self {
        Cache { dir, cap }
    }

    /// Its folder.
    #[cfg(test)]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn file(&self, sha: &str) -> PathBuf {
        self.dir.join("files").join(sha)
    }

    fn part(&self, sha: &str) -> PathBuf {
        self.dir.join("files").join(format!("{sha}.part"))
    }

    /// Whether the file of this hash is kept (not yet checked: `read` does that).
    pub fn has(&self, sha: &str) -> bool {
        is_hash(sha) && self.file(sha).is_file()
    }

    /// The file of this hash, if it is kept and is still what its hash says and a file of this
    /// kind the client takes. It counts as used now. One that is not is dropped.
    pub fn read(&self, sha: &str, kind: Kind) -> Result<Vec<u8>, String> {
        if !is_hash(sha) {
            return Err("not a SHA-256".into());
        }
        let path = self.file(sha);
        let bytes = read_at_most(&path, kind.limit()).map_err(|e| e.to_string())?;
        let checked = if sha256(&bytes) != sha {
            Err("its bytes are not those its hash says".to_string())
        } else {
            check::contents(kind, &bytes)
        };
        if let Err(e) = checked {
            let _ = fs::remove_file(&path);
            return Err(e);
        }
        touch(&path);
        Ok(bytes)
    }

    /// How much of the file of this hash has been fetched so far.
    pub fn part_len(&self, sha: &str) -> u64 {
        fs::metadata(self.part(sha)).map_or(0, |m| m.len())
    }

    /// The part fetched so far, to write more to: at its end, or from nothing.
    pub fn open_part(&self, sha: &str, from_nothing: bool) -> io::Result<File> {
        fs::create_dir_all(self.dir.join("files"))?;
        let mut options = File::options();
        options.create(true);
        if from_nothing {
            options.write(true).truncate(true);
        } else {
            options.append(true);
        }
        options.open(self.part(sha))
    }

    /// Takes a part fetched to its end as the file of its hash, once its bytes are what the hash
    /// says and it is a file of this kind the client takes. A part that is not is dropped.
    pub fn finish(&self, sha: &str, kind: Kind) -> Result<(), String> {
        let part = self.part(sha);
        let bytes = read_at_most(&part, kind.limit()).map_err(|e| e.to_string());
        let checked = bytes.and_then(|bytes| {
            if sha256(&bytes) != sha {
                Err("its bytes are not those its hash says".to_string())
            } else {
                check::contents(kind, &bytes)
            }
        });
        match checked {
            Ok(()) => fs::rename(&part, self.file(sha)).map_err(|e| e.to_string()),
            Err(e) => {
                let _ = fs::remove_file(&part);
                Err(e)
            }
        }
    }

    /// Drops what was fetched of the file of this hash.
    pub fn drop_part(&self, sha: &str) {
        let _ = fs::remove_file(self.part(sha));
    }

    fn index_path(&self, origin: &str, game: &str) -> PathBuf {
        // Hashed, since neither is a name a system can be trusted to keep a file by.
        let key = |s: &str| sha256(s.as_bytes())[..16].to_string();
        self.dir
            .join("indexes")
            .join(key(origin))
            .join(format!("{}.json", key(game)))
    }

    /// Keeps the index a server gave for a game.
    pub fn keep_index(&self, origin: &str, game: &str, index: &ClientIndex) {
        let path = self.index_path(origin, game);
        let written = path
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| fs::write(&path, serde_json::to_vec(index).unwrap_or_default()));
        if let Err(e) = written {
            bevy::log::warn!("could not keep the look's index: {e}");
        }
    }

    /// The index this server last gave for the game, if it is kept.
    pub fn kept_index(&self, origin: &str, game: &str) -> Option<ClientIndex> {
        let bytes = read_at_most(&self.index_path(origin, game), 4 << 20).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Drops the files used longest ago (and parts fetched longest ago) until what is kept is
    /// within the cap; never those of `keep`, by hash.
    pub fn trim(&self, keep: &HashSet<String>) {
        let Ok(entries) = fs::read_dir(self.dir.join("files")) else {
            return;
        };
        let mut files: Vec<(SystemTime, u64, PathBuf)> = entries
            .flatten()
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                let name = e.file_name().to_string_lossy().into_owned();
                let sha = name.strip_suffix(".part").unwrap_or(&name);
                if !meta.is_file() || !is_hash(sha) || keep.contains(sha) {
                    return None;
                }
                Some((meta.modified().ok()?, meta.len(), e.path()))
            })
            .collect();
        let kept: u64 = keep
            .iter()
            .filter_map(|sha| fs::metadata(self.file(sha)).ok())
            .map(|m| m.len())
            .sum();
        let mut total = kept + files.iter().map(|(_, len, _)| len).sum::<u64>();
        files.sort();
        for (_, len, path) in files {
            if total <= self.cap {
                break;
            }
            if fs::remove_file(&path).is_ok() {
                total -= len;
            }
        }
    }
}

/// A file's bytes, but no more than `limit` of them.
fn read_at_most(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::other(format!("over {limit} bytes")));
    }
    Ok(bytes)
}

/// Marks a file as used now, which is what the cap drops by.
fn touch(path: &Path) {
    if let Ok(f) = File::options().write(true).open(path) {
        let _ = f.set_modified(SystemTime::now());
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;
    use std::time::Duration;

    /// A cache of its own, in a temporary folder that is emptied first.
    pub(crate) fn scratch(name: &str) -> Cache {
        let dir =
            std::env::temp_dir().join(format!("bingle-3d-cache-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Cache::new(dir, CAP)
    }

    const LOOK: &[u8] = b"format = 1\n";

    #[test]
    fn a_file_is_kept_by_its_hash_once_its_bytes_are_what_the_hash_says() {
        let cache = scratch("hash");
        let sha = sha256(LOOK);
        assert!(!cache.has(&sha));
        let mut part = cache.open_part(&sha, true).unwrap();
        part.write_all(&LOOK[..4]).unwrap();
        drop(part);
        assert_eq!(cache.part_len(&sha), 4);
        cache
            .open_part(&sha, false)
            .unwrap()
            .write_all(&LOOK[4..])
            .unwrap();
        cache.finish(&sha, Kind::Look).unwrap();
        assert!(cache.has(&sha));
        assert_eq!(cache.read(&sha, Kind::Look).unwrap(), LOOK);
        assert_eq!(cache.part_len(&sha), 0);

        // Bytes that are not those of the hash are never kept.
        let other = sha256(b"something else");
        cache
            .open_part(&other, true)
            .unwrap()
            .write_all(LOOK)
            .unwrap();
        assert!(
            cache
                .finish(&other, Kind::Look)
                .unwrap_err()
                .contains("hash")
        );
        assert!(!cache.has(&other));
        assert_eq!(cache.part_len(&other), 0);
        // Nor what is not a file of its kind.
        let not_text = b"\0\0";
        let sha = sha256(not_text);
        cache
            .open_part(&sha, true)
            .unwrap()
            .write_all(not_text)
            .unwrap();
        assert!(cache.finish(&sha, Kind::Look).is_err());

        // A kept file changed on the disk is dropped when it is read.
        let sha = sha256(LOOK);
        fs::write(cache.file(&sha), b"format = 2\n").unwrap();
        assert!(cache.read(&sha, Kind::Look).is_err());
        assert!(!cache.has(&sha));
        assert!(cache.read("../../etc/passwd", Kind::Look).is_err());
    }

    #[test]
    fn an_index_is_kept_per_server_and_game() {
        let cache = scratch("index");
        let index: ClientIndex = serde_json::from_str(
            r#"{"files":[{"path":"appearance/a.toml","size":3,"sha256":"x"}]}"#,
        )
        .unwrap();
        cache.keep_index("https://a:443", "g", &index);
        assert_eq!(cache.kept_index("https://a:443", "g"), Some(index));
        assert_eq!(cache.kept_index("https://b:443", "g"), None);
        assert_eq!(cache.kept_index("https://a:443", "../g"), None);
    }

    #[test]
    fn past_its_cap_the_cache_drops_what_was_used_longest_ago() {
        let mut cache = scratch("cap");
        cache.cap = 25;
        let put = |bytes: &[u8], age: u64| {
            let sha = sha256(bytes);
            fs::create_dir_all(cache.dir.join("files")).unwrap();
            let path = cache.file(&sha);
            fs::write(&path, bytes).unwrap();
            let when = SystemTime::now() - Duration::from_secs(age);
            File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(when)
                .unwrap();
            sha
        };
        let old = put(b"0123456789", 300);
        let older_but_kept = put(b"abcdefghij", 400);
        let new = put(b"ABCDEFGHIJ", 100);
        cache.trim(&HashSet::from([older_but_kept.clone()]));
        assert!(!cache.has(&old));
        assert!(cache.has(&older_but_kept) && cache.has(&new));
        // Reading a file uses it: the other goes first.
        cache.cap = 10;
        cache.read(&new, Kind::Look).unwrap();
        cache.trim(&HashSet::new());
        assert!(cache.has(&new) && !cache.has(&older_but_kept));
    }
}
