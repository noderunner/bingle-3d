//! Fetching a game's look, on the network thread: the index first, then only the files the
//! cache lacks, each a range of bytes at a time ([`CHUNK`]), so that the thread answers
//! everything else in between, a download broken off resumes from where it stopped, and one
//! the player abandons stops at once.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Write};

use super::cache::Cache;
use super::check::{self, Kind};
use crate::net::api::{ClientFile, ClientIndex};
use crate::net::session::{Received, Session};

/// How many bytes of a file are asked for at a time.
pub const CHUNK: u64 = 1 << 20;

/// How many times running a file is asked for without a byte coming before it is given up.
const TRIES: u32 = 3;

/// What is fetched of a game's look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Only what the list shows: the appearance file and the picture on the game's card.
    Picture,
    /// Every file, to draw the game.
    Whole,
}

/// A file of a game's look, checked and here.
#[derive(Debug, Clone, PartialEq)]
pub struct File {
    /// Its path in the look's folder, as the look names it.
    pub path: String,
    pub sha256: String,
    pub kind: Kind,
    pub bytes: Vec<u8>,
}

/// A look fetched: the files that are here and checked, and what was left out, and why.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Fetched {
    pub files: Vec<File>,
    pub refused: Vec<String>,
}

/// What the fetcher has to say after a step.
#[derive(Debug, Clone, PartialEq)]
pub enum News {
    /// So many bytes of so many are here.
    Progress { game: String, done: u64, total: u64 },
    /// Done: what is here, or why there is nothing (no index).
    Done {
        game: String,
        scope: Scope,
        result: Result<Fetched, String>,
    },
}

struct Job {
    game: String,
    scope: Scope,
    stage: Stage,
}

enum Stage {
    /// The index is to be read.
    Index,
    Files {
        /// Still to be fetched, or found in the cache.
        todo: VecDeque<(ClientFile, Kind)>,
        /// Here, and checked.
        here: Vec<(ClientFile, Kind)>,
        refused: Vec<String>,
        /// Every file the index offers, for a picture's to be found among them.
        offered: Vec<(ClientFile, Kind)>,
        /// The picture is yet to be looked for (`Scope::Picture`), once the look is here.
        picture: bool,
        done: u64,
        total: u64,
        /// Times in a row the file at the front was asked for and nothing came.
        failed: u32,
    },
}

pub struct Fetcher {
    cache: Cache,
    jobs: VecDeque<Job>,
    /// The indexes read in this run, by server and game: a server's games do not change while it
    /// runs, so a game opened again asks for nothing.
    indexes: HashMap<(String, String), ClientIndex>,
    chunk: u64,
}

impl Fetcher {
    pub fn new(cache: Cache) -> Self {
        Fetcher {
            cache,
            jobs: VecDeque::new(),
            indexes: HashMap::new(),
            chunk: CHUNK,
        }
    }

    /// Whether there is anything to fetch.
    pub fn busy(&self) -> bool {
        !self.jobs.is_empty()
    }

    /// Fetch this of a game's look. A game to draw goes first; one already asked for is not
    /// asked for twice, though a picture asked for becomes the whole look.
    pub fn add(&mut self, game: String, scope: Scope) {
        if let Some(at) = self.jobs.iter().position(|j| j.game == game) {
            if scope == Scope::Whole && self.jobs[at].scope == Scope::Picture {
                self.jobs.remove(at);
            } else {
                return;
            }
        }
        let job = Job {
            game,
            scope,
            stage: Stage::Index,
        };
        match scope {
            Scope::Whole => self.jobs.push_front(job),
            Scope::Picture => self.jobs.push_back(job),
        }
    }

    /// Stop fetching a game's look. What was fetched of a file is kept, to go on from there.
    pub fn abandon(&mut self, game: &str) {
        self.jobs.retain(|j| j.game != game);
    }

    /// Does the next thing there is to do: reads an index, or a range of a file. Every step
    /// ends with news, so the game shows how far it has got.
    pub fn step(&mut self, session: &Session) -> Option<News> {
        let mut job = self.jobs.pop_front()?;
        let news = match job.stage {
            Stage::Index => self.read_index(session, &mut job),
            Stage::Files { .. } => self.fetch(session, &mut job),
        };
        if !matches!(news, Some(News::Done { .. })) {
            self.jobs.push_front(job);
        }
        news
    }

    fn read_index(&mut self, session: &Session, job: &mut Job) -> Option<News> {
        let origin = session.origin().unwrap_or_default();
        let key = (origin.clone(), job.game.clone());
        let index = match self.indexes.get(&key) {
            Some(index) => index.clone(),
            None => match session.client_index(&job.game) {
                Ok(index) => {
                    self.cache.keep_index(&origin, &job.game, &index);
                    self.indexes.insert(key, index.clone());
                    index
                }
                // The server cannot be asked now: the index it gave last time, if any.
                Err(problem) => match self.cache.kept_index(&origin, &job.game) {
                    Some(index) => {
                        bevy::log::warn!(
                            "the look of {}: {}; using its index from before",
                            job.game,
                            problem.message()
                        );
                        index
                    }
                    None => {
                        return Some(News::Done {
                            game: job.game.clone(),
                            scope: job.scope,
                            result: Err(problem.message()),
                        });
                    }
                },
            },
        };
        let (offered, refused) = check::index(&index.files);
        let todo: VecDeque<_> = match job.scope {
            Scope::Whole => offered.iter().cloned().collect(),
            Scope::Picture => offered
                .iter()
                .filter(|(_, kind)| *kind == Kind::Look)
                .cloned()
                .collect(),
        };
        let (done, total) = self.counts(todo.iter());
        job.stage = Stage::Files {
            todo,
            here: Vec::new(),
            refused,
            offered,
            picture: job.scope == Scope::Picture,
            done,
            total,
            failed: 0,
        };
        self.fetch(session, job)
    }

    /// How much of these is here already, and how much they come to, of those to fetch.
    fn counts<'a>(&self, files: impl Iterator<Item = &'a (ClientFile, Kind)>) -> (u64, u64) {
        files
            .filter(|(f, _)| !self.cache.has(&f.sha256))
            .fold((0, 0), |(done, total), (f, _)| {
                (
                    done + self.cache.part_len(&f.sha256).min(f.size),
                    total + f.size,
                )
            })
    }

    fn fetch(&mut self, session: &Session, job: &mut Job) -> Option<News> {
        let Stage::Files {
            todo,
            here,
            refused,
            offered,
            picture,
            done,
            total,
            failed,
        } = &mut job.stage
        else {
            return None;
        };
        loop {
            // What is kept already needs no asking.
            while let Some((file, kind)) = todo.front().cloned() {
                if !self.cache.has(&file.sha256) {
                    break;
                }
                todo.pop_front();
                here.push((file, kind));
            }
            if !todo.is_empty() {
                break;
            }
            // The look is here: the picture its card shows, if it is not.
            if std::mem::take(picture)
                && let Some(file) = card_picture(&self.cache, &job.game, here, offered)
            {
                let (d, t) = self.counts(std::iter::once(&file));
                (*done, *total) = (*done + d, *total + t);
                todo.push_back(file);
                continue;
            }
            return Some(self.done(job.game.clone(), job.scope, here, refused));
        }
        let (file, kind) = todo.front().cloned()?;
        let before = self.cache.part_len(&file.sha256);
        let result = if before >= file.size {
            Ok(())
        } else {
            self.fetch_range(session, &job.game, &file, before)
        };
        let after = self.cache.part_len(&file.sha256);
        *done = (*done + after).saturating_sub(before.min(file.size));
        match result {
            Ok(()) if after >= file.size => {
                todo.pop_front();
                *failed = 0;
                match self.cache.finish(&file.sha256, kind) {
                    Ok(()) => here.push((file, kind)),
                    Err(e) => refused.push(format!("{}: {e}", file.path)),
                }
            }
            Ok(()) => *failed = 0,
            Err(e) => {
                *failed = if after > before { 1 } else { *failed + 1 };
                bevy::log::info!("fetching {} of {}: {e}", file.path, job.game);
                if *failed >= TRIES {
                    todo.pop_front();
                    *failed = 0;
                    refused.push(format!("{}: {e}", file.path));
                }
            }
        }
        Some(News::Progress {
            game: job.game.clone(),
            done: *done,
            total: *total,
        })
    }

    /// Asks for the next range of a file, from `from`, and writes it to the end of what was
    /// fetched as it comes: a range broken off is kept as far as it came.
    fn fetch_range(
        &self,
        session: &Session,
        game: &str,
        file: &ClientFile,
        from: u64,
    ) -> Result<(), String> {
        let to = file.size.min(from + self.chunk);
        let (received, mut body) = session
            .client_file(game, file, from, to)
            .map_err(|p| p.message())?;
        // The whole file, from its start: the server will not send a part, or has another file.
        let (from_nothing, most) = match received {
            Received::Part => (false, to - from),
            Received::Whole => (true, file.size + 1),
        };
        let mut out = self
            .cache
            .open_part(&file.sha256, from_nothing)
            .map_err(|e| e.to_string())?;
        let mut buf = vec![0; 64 << 10];
        let mut left = most;
        while left > 0 {
            let room = left.min(buf.len() as u64) as usize;
            let n = body
                .read(&mut buf[..room])
                .map_err(|e| format!("broken off: {e}"))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            left -= n as u64;
        }
        if received == Received::Whole && left == 0 {
            self.cache.drop_part(&file.sha256);
            return Err("longer than the index said".into());
        }
        if received == Received::Part && left > 0 {
            return Err("broken off".into());
        }
        Ok(())
    }

    /// The files that are here, read from the cache and checked once more; the cache trimmed.
    fn done(
        &self,
        game: String,
        scope: Scope,
        here: &[(ClientFile, Kind)],
        refused: &mut Vec<String>,
    ) -> News {
        let mut files = Vec::new();
        for (file, kind) in here {
            match self.cache.read(&file.sha256, *kind) {
                Ok(bytes) => files.push(File {
                    path: file.path.clone(),
                    sha256: file.sha256.clone(),
                    kind: *kind,
                    bytes,
                }),
                Err(e) => refused.push(format!("{}: {e}", file.path)),
            }
        }
        let keep: HashSet<String> = files.iter().map(|f| f.sha256.clone()).collect();
        self.cache.trim(&keep);
        News::Done {
            game,
            scope,
            result: Ok(Fetched {
                files,
                refused: std::mem::take(refused),
            }),
        }
    }
}

/// The picture on the game's card, among the files offered, once its look is here.
fn card_picture(
    cache: &Cache,
    game: &str,
    here: &[(ClientFile, Kind)],
    offered: &[(ClientFile, Kind)],
) -> Option<(ClientFile, Kind)> {
    let looks: Vec<(String, String)> = here
        .iter()
        .filter(|(_, kind)| *kind == Kind::Look)
        .filter_map(|(f, kind)| {
            let bytes = cache.read(&f.sha256, *kind).ok()?;
            Some((f.path.clone(), String::from_utf8(bytes).ok()?))
        })
        .collect();
    let path = crate::appearance::Appearance::downloaded_picture(game, &looks)?;
    offered
        .iter()
        .find(|(f, kind)| f.path == path && matches!(kind, Kind::Png | Kind::Jpeg))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::super::cache::sha256;
    use super::super::cache::tests::scratch;
    use super::*;
    use crate::net::session::tests::session;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// One request a server was sent: its path, and the range it asked for.
    #[derive(Debug, Clone, PartialEq)]
    struct Asked {
        path: String,
        range: Option<String>,
    }

    /// A Bingle that offers these files for every game, served as it does: an index, and each
    /// file by its path, in ranges.
    fn offering(files: Vec<(&'static str, Vec<u8>)>) -> (String, Arc<Mutex<Vec<Asked>>>) {
        offering_with(files, None, &[])
    }

    /// The same, but the first answer with byte `at` of the file `cut` names in it is broken
    /// off there, and the index gives the files `lies` names a hash that is not theirs.
    fn offering_with(
        files: Vec<(&'static str, Vec<u8>)>,
        cut: Option<(&'static str, usize)>,
        lies: &[&str],
    ) -> (String, Arc<Mutex<Vec<Asked>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("http://{}", listener.local_addr().unwrap());
        let asked = Arc::new(Mutex::new(Vec::new()));
        let log = asked.clone();
        let index = serde_json::json!({
            "game": "x", "client": "bingle-3d",
            "files": files.iter().map(|(p, b)| serde_json::json!({
                "path": p, "size": b.len(), "content_type": "x",
                "sha256": if lies.contains(p) { sha256(b"not these") } else { sha256(b) },
            })).collect::<Vec<_>>(),
        })
        .to_string();
        std::thread::spawn(move || {
            let mut cut = cut;
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let path = line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_string();
                let mut range = None;
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    if header.trim().is_empty() {
                        break;
                    }
                    if let Some((name, value)) = header.split_once(':')
                        && name.eq_ignore_ascii_case("range")
                    {
                        range = Some(value.trim().to_string());
                    }
                }
                log.lock().unwrap().push(Asked {
                    path: path.clone(),
                    range: range.clone(),
                });
                let file = path.split_once("/files/").map(|(_, f)| f.to_string());
                let mut sent = None;
                let (status, extra, body) = match file {
                    None => (200, String::new(), index.clone().into_bytes()),
                    Some(f) => match files.iter().find(|(p, _)| *p == f) {
                        None => (404, String::new(), b"{\"error\":\"no\"}".to_vec()),
                        Some((_, bytes)) => {
                            let (a, b) = range
                                .as_deref()
                                .and_then(|r| r.strip_prefix("bytes="))
                                .and_then(|r| r.split_once('-'))
                                .map(|(a, b)| (a.parse().unwrap(), b.parse::<usize>().unwrap()))
                                .unwrap();
                            let b = b.min(bytes.len() - 1);
                            if let Some((name, at)) = cut
                                && f.ends_with(name)
                                && (a..=b).contains(&at)
                            {
                                sent = Some(at - a);
                                cut = None;
                            }
                            let extra = format!("Content-Range: bytes {a}-{b}/{}\r\n", bytes.len());
                            (206, extra, bytes[a..=b].to_vec())
                        }
                    },
                };
                let sent = sent.unwrap_or(body.len());
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(&body[..sent]);
            }
        });
        (addr, asked)
    }

    const LOOK: &str = "format = 1\n[card.pictures]\ng = \"pictures/g.png\"\n";

    fn picture() -> Vec<u8> {
        let mut b = check::tests::png(32, 32);
        b.extend([7; 3000]);
        b
    }

    /// Steps until the fetcher has nothing more to do, and returns the last news.
    fn run(fetcher: &mut Fetcher, session: &Session) -> News {
        let mut last = None;
        for _ in 0..1000 {
            match fetcher.step(session) {
                Some(news) => last = Some(news),
                None => return last.expect("some news"),
            }
        }
        panic!("the fetcher never finished");
    }

    fn fetched(news: News) -> Fetched {
        match news {
            News::Done { result, .. } => result.unwrap(),
            other => panic!("{other:?}"),
        }
    }

    fn paths(f: &Fetched) -> Vec<&str> {
        f.files.iter().map(|f| f.path.as_str()).collect()
    }

    fn files_asked(asked: &Mutex<Vec<Asked>>) -> Vec<Asked> {
        asked
            .lock()
            .unwrap()
            .iter()
            .filter(|a| a.path.contains("/files/"))
            .cloned()
            .collect()
    }

    #[test]
    fn a_look_is_fetched_once_and_opening_it_again_fetches_nothing() {
        let (addr, asked) = offering(vec![
            ("appearance/g.toml", LOOK.into()),
            ("pictures/g.png", picture()),
            ("README.md", b"notes".to_vec()),
        ]);
        let session = session(&addr);
        let mut fetcher = Fetcher::new(scratch("once"));
        fetcher.add("g".into(), Scope::Whole);
        let look = fetched(run(&mut fetcher, &session));
        assert_eq!(paths(&look), ["appearance/g.toml", "pictures/g.png"]);
        assert_eq!(look.files[0].bytes, LOOK.as_bytes());
        assert!(look.refused.is_empty());
        let asked_first = asked.lock().unwrap().clone();
        assert_eq!(
            asked_first.len(),
            3,
            "the index, then each file: {asked_first:?}"
        );
        assert_eq!(
            asked_first[0].path, "/api/game-defs/g/clients/bingle-3d",
            "the index first"
        );
        let to = LOOK.len() - 1;
        assert_eq!(asked_first[1].range, Some(format!("bytes=0-{to}")));

        // Again in this run: nothing is asked.
        fetcher.add("g".into(), Scope::Whole);
        assert_eq!(paths(&fetched(run(&mut fetcher, &session))), paths(&look));
        assert_eq!(asked.lock().unwrap().len(), 3);

        // In the next run, with the same cache: the index only.
        let mut next = Fetcher::new(Cache::new(
            fetcher.cache.dir().to_path_buf(),
            super::super::cache::CAP,
        ));
        next.add("g".into(), Scope::Whole);
        assert_eq!(paths(&fetched(run(&mut next, &session))), paths(&look));
        assert_eq!(asked.lock().unwrap().len(), 4);
        assert!(!asked.lock().unwrap()[3].path.contains("/files/"));
    }

    #[test]
    fn the_same_file_offered_by_two_servers_is_fetched_once() {
        let files = || {
            vec![
                ("appearance/g.toml", LOOK.as_bytes().to_vec()),
                ("pictures/g.png", picture()),
            ]
        };
        let (a, asked_a) = offering(files());
        let (b, asked_b) = offering(files());
        let cache = scratch("two");
        let dir = cache.dir().to_path_buf();
        let mut fetcher = Fetcher::new(cache);
        fetcher.add("g".into(), Scope::Whole);
        fetched(run(&mut fetcher, &session(&a)));
        assert_eq!(files_asked(&asked_a).len(), 2);
        let mut other = Fetcher::new(Cache::new(dir, super::super::cache::CAP));
        other.add("g".into(), Scope::Whole);
        let look = fetched(run(&mut other, &session(&b)));
        assert_eq!(look.files.len(), 2);
        assert!(
            files_asked(&asked_b).is_empty(),
            "{:?}",
            asked_b.lock().unwrap()
        );
    }

    #[test]
    fn a_download_broken_off_halfway_resumes_from_where_it_stopped() {
        let big = picture();
        let (addr, asked) = offering_with(
            vec![("pictures/g.png", big.clone())],
            Some(("g.png", 1500)),
            &[],
        );
        let session = session(&addr);
        let mut fetcher = Fetcher::new(scratch("resume"));
        fetcher.chunk = 1024;
        fetcher.add("g".into(), Scope::Whole);
        let mut progress = Vec::new();
        let look = loop {
            match fetcher.step(&session) {
                Some(News::Progress { done, total, .. }) => progress.push((done, total)),
                Some(done) => break fetched(done),
                None => panic!("no news"),
            }
        };
        assert_eq!(look.files[0].bytes, big);
        let ranges: Vec<String> = files_asked(&asked)
            .into_iter()
            .filter_map(|a| a.range)
            .collect();
        // The second range is broken off at byte 1,500, and the next goes on from there.
        let last = big.len() - 1;
        assert_eq!(
            ranges,
            [
                "bytes=0-1023".to_string(),
                "bytes=1024-2047".into(),
                "bytes=1500-2523".into(),
                format!("bytes=2524-{last}"),
            ]
        );
        assert_eq!(
            progress.iter().map(|p| p.0).collect::<Vec<_>>(),
            [1024, 1500, 2524, big.len() as u64]
        );
        assert!(progress.iter().all(|&(_, t)| t == big.len() as u64));
    }

    #[test]
    fn a_file_that_is_not_what_its_hash_says_or_the_client_takes_is_refused() {
        let (addr, _) = offering_with(
            vec![
                ("appearance/g.toml", LOOK.into()),
                ("models/m.glb", b"glTF not really".to_vec()),
                ("models/ring.glb", b"one of ours".to_vec()),
                ("pictures/lie.png", picture()),
            ],
            None,
            &["pictures/lie.png"],
        );
        let cache = scratch("refused");
        let dir = cache.dir().to_path_buf();
        let mut fetcher = Fetcher::new(cache);
        fetcher.add("g".into(), Scope::Whole);
        let look = fetched(run(&mut fetcher, &session(&addr)));
        assert_eq!(paths(&look), ["appearance/g.toml"], "the rest is kept");
        let refused = &look.refused;
        assert_eq!(refused.len(), 3, "{refused:?}");
        assert!(refused[0].starts_with("models/ring.glb: the client has its own"));
        assert!(refused[1].starts_with("models/m.glb: not a binary glTF"));
        assert!(refused[2].starts_with("pictures/lie.png: its bytes are not those its hash"));
        // Nothing refused is kept.
        let kept = std::fs::read_dir(dir.join("files")).unwrap().count();
        assert_eq!(kept, 1);
    }

    #[test]
    fn a_picture_alone_is_the_look_and_the_picture_its_card_shows() {
        let (addr, asked) = offering(vec![
            ("appearance/g.toml", LOOK.into()),
            ("models/m.glb", b"never asked for".to_vec()),
            ("pictures/g.png", picture()),
        ]);
        let mut fetcher = Fetcher::new(scratch("picture"));
        fetcher.add("g".into(), Scope::Picture);
        let look = fetched(run(&mut fetcher, &session(&addr)));
        assert_eq!(paths(&look), ["appearance/g.toml", "pictures/g.png"]);
        assert_eq!(files_asked(&asked).len(), 2);
    }

    #[test]
    fn an_abandoned_look_stops_and_a_game_to_draw_goes_first() {
        let (addr, asked) = offering(vec![("pictures/g.png", picture())]);
        let session = session(&addr);
        let mut fetcher = Fetcher::new(scratch("abandon"));
        fetcher.chunk = 1024;
        fetcher.add("list".into(), Scope::Picture);
        fetcher.add("g".into(), Scope::Whole);
        fetcher.add("g".into(), Scope::Picture);
        assert_eq!(fetcher.jobs.len(), 2);
        assert_eq!(fetcher.jobs[0].game, "g");
        assert!(matches!(
            fetcher.step(&session),
            Some(News::Progress { .. })
        ));
        fetcher.abandon("g");
        assert_eq!(fetcher.jobs.len(), 1);
        let before = asked.lock().unwrap().len();
        assert_eq!(before, 2, "the index, and one range");
        // What was fetched is kept, and the next time goes on from it.
        fetcher.abandon("list");
        fetcher.add("g".into(), Scope::Whole);
        fetched(run(&mut fetcher, &session));
        assert_eq!(
            files_asked(&asked)[1].range.as_deref(),
            Some("bytes=1024-2047")
        );
    }

    #[test]
    fn with_no_index_there_is_no_look() {
        let mut fetcher = Fetcher::new(scratch("none"));
        fetcher.add("g".into(), Scope::Whole);
        let session = session("http://127.0.0.1:9");
        match run(&mut fetcher, &session) {
            News::Done { result, .. } => assert!(result.is_err()),
            other => panic!("{other:?}"),
        }
    }
}
