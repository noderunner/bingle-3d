//! A link clicked while the client runs goes to the copy already running (#123).
//!
//! The copy started with `--link` (`hands_over`) hands the link's text to the running one and
//! quits; the running one (`listens`) reads it again with `link::Link::parse` and puts it
//! through the same questions as a link at start (`link_open::take_handed`). **Only the same
//! user can reach the channel, and only a link's text crosses it**, with one byte back meaning
//! it was taken.
//!
//! - **Linux**: a Unix socket, `links.sock`, made inside a directory of the client's own,
//!   `$XDG_RUNTIME_DIR/bingle-3d` (or `<config dir>/run` without a runtime directory), created
//!   0700 and refused when it is anybody else's or wider: another user cannot enter it, so
//!   cannot connect or put a socket there, not even before the socket is set to 0600. The
//!   process's umask is left alone. Which copy listens is decided by a lock beside it
//!   (`links.lock`, `File::try_lock`), held while it runs; a socket left by a crash is replaced
//!   by the next copy to take the lock, and a socket that answers is never taken over.
//! - **Windows**: a named pipe, `\\.\pipe\bingle-3d-links-<the user's SID>`, owned by the user
//!   and granting only the user (the default descriptor also gives Everyone and anonymous
//!   read), one instance, made with `FILE_FLAG_FIRST_PIPE_INSTANCE` and taking no remote
//!   client. The handing copy writes only to a pipe the user owns, since another user could
//!   make one of that name while no copy of ours holds it.
//!
//! The running copy accepts on a thread of its own and reads once, bounded (`read_message`:
//! the link's 1024 characters and a newline at the most, within `READ_WITHIN`), then closes
//! the connection whatever came. A link goes into a queue of `QUEUE`; one that finds it full
//! is dropped. A copy whose frames have stopped (`Heartbeat`) says not taken. The handing
//! copy gives up after `HAND_OVER_WITHIN` in all and starts on its own, exactly as without
//! a copy running. **Never log a link**: nothing here repeats what came.

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::prelude::*;

use crate::link::{self, Link};

/// The most a message can be: a link and its newline.
pub const MAX_MESSAGE: usize = link::MAX_LEN + 1;
/// How long the running copy reads one connection for, before closing it whatever came.
pub const READ_WITHIN: Duration = Duration::from_millis(250);
/// How long the handing copy waits in all (connecting, writing, the answer) before it starts
/// on its own.
pub const HAND_OVER_WITHIN: Duration = Duration::from_millis(500);
/// How long each of the handing copy's own reads and writes may take.
const EACH_WITHIN: Duration = Duration::from_millis(300);
/// A copy whose last frame is older than this does not take a link: the second copy opens it.
pub const STALLED: Duration = Duration::from_secs(3);
/// The links waiting for the next frame; more are dropped.
pub const QUEUE: usize = 4;
/// The answer that says the link was taken; anything else, or nothing, says it was not.
const TAKEN: u8 = b'+';
const NOT_TAKEN: u8 = b'-';

/// The text of the link a copy was started with, kept to hand over. Its `Debug` leaves it out:
/// it may hold a code.
#[derive(Clone, PartialEq, Eq)]
pub struct Handed(pub String);

impl fmt::Debug for Handed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Handed(…)")
    }
}

/// Whether a run hands its link to a copy already running: only one started with `--link`, a
/// link that parsed, in a run that no script limits.
pub fn hands_over(opts: &crate::cli::Options) -> Option<&Handed> {
    let scripted = opts.last_frame().is_some();
    match (&opts.link, &opts.link_text) {
        (Some(Ok(_)), Some(text)) if !scripted => Some(text),
        _ => None,
    }
}

/// Whether a run listens for the links clicked while it runs: a player's, not a script's nor a
/// special run's (`--frames`, `--screenshot`, `--watch` and its kin, `--new-game`, `--setup`,
/// `--showcase`, `--test-hand`), not in a headset, in a build that opens links. A second copy
/// started by hand finds the first listening, and starts as it would without it.
pub fn listens(opts: &crate::cli::Options, in_headset: bool) -> bool {
    !in_headset
        && crate::link_handler::supported()
        && opts.last_frame().is_none()
        && opts.watch.is_none()
        && opts.new_game.is_none()
        && opts.setup.is_none()
        && !opts.showcase
        && opts.test_hand.is_none()
}

/// A link's message: its text and a newline. None for text no link could be.
pub fn frame(text: &str) -> Option<Vec<u8>> {
    if text.len() > link::MAX_LEN || text.contains('\n') {
        return None;
    }
    let mut message = Vec::with_capacity(text.len() + 1);
    message.extend_from_slice(text.as_bytes());
    message.push(b'\n');
    Some(message)
}

/// A connection read with a bound.
pub trait TimedRead {
    /// Reads what has come, waiting at most `within`: `Ok(0)` once the other end has closed, a
    /// `TimedOut` or `WouldBlock` error when nothing came in time.
    fn read_within(&mut self, buf: &mut [u8], within: Duration) -> std::io::Result<usize>;
}

/// Why what came was not a message. None repeats it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    TooLong,
    /// Closed before its newline.
    Unended,
    /// Something after the newline.
    Trailing,
    /// The newline did not come in time.
    Late,
    NotText,
    Failed,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Refused::TooLong => "it was longer than a link can be",
            Refused::Unended => "it ended before the link did",
            Refused::Trailing => "something followed the link",
            Refused::Late => "it did not come in time",
            Refused::NotText => "it was not text",
            Refused::Failed => "it could not be read",
        })
    }
}

/// Reads one message, until `deadline` at the latest: the text before its newline.
pub fn read_message(conn: &mut impl TimedRead, deadline: Instant) -> Result<String, Refused> {
    let mut buf = [0u8; MAX_MESSAGE];
    let mut n = 0;
    loop {
        if let Some(end) = buf[..n].iter().position(|&b| b == b'\n') {
            if end + 1 != n {
                return Err(Refused::Trailing);
            }
            return String::from_utf8(buf[..end].to_vec()).map_err(|_| Refused::NotText);
        }
        if n == MAX_MESSAGE {
            return Err(Refused::TooLong);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(Refused::Late);
        }
        match conn.read_within(&mut buf[n..], left) {
            Ok(0) => return Err(Refused::Unended),
            Ok(k) => n += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                return Err(Refused::Late);
            }
            Err(_) => return Err(Refused::Failed),
        }
    }
}

/// When the running copy last drew a frame, for the accepting thread to tell a copy that has
/// stopped.
#[derive(Debug)]
pub struct Heartbeat {
    start: Instant,
    last_ms: AtomicU64,
}

impl Heartbeat {
    pub fn new() -> Self {
        Heartbeat {
            start: Instant::now(),
            last_ms: AtomicU64::new(0),
        }
    }

    fn now_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    pub fn beat(&self) {
        self.last_ms.store(self.now_ms(), Ordering::Relaxed);
    }

    /// A frame was drawn within `STALLED`.
    pub fn alive(&self) -> bool {
        self.now_ms()
            .saturating_sub(self.last_ms.load(Ordering::Relaxed))
            < STALLED.as_millis() as u64
    }
}

/// What the running copy does with one connection: read it once, take a link that parses
/// while there is room and its frames go on, and answer.
fn receive<C: TimedRead + Answer>(conn: &mut C, beat: &Heartbeat, queue: &SyncSender<Link>) {
    let taken = match read_message(conn, Instant::now() + READ_WITHIN) {
        Err(why) => {
            info!("game links: what another copy sent was dropped: {why}");
            false
        }
        Ok(text) => match Link::parse(&text) {
            Err(why) => {
                info!("game links: a link from another copy was refused: {why}");
                false
            }
            Ok(_) if !beat.alive() => {
                info!("game links: a link from another copy was left to it: no frame for a while");
                false
            }
            Ok(link) => match queue.try_send(link) {
                Ok(()) => {
                    info!("game links: a link came from another copy");
                    true
                }
                Err(TrySendError::Full(_)) => {
                    info!("game links: a link from another copy was dropped: too many at once");
                    false
                }
                Err(TrySendError::Disconnected(_)) => false,
            },
        },
    };
    conn.answer(if taken { TAKEN } else { NOT_TAKEN });
}

/// The running copy's answer, and the end of the connection.
trait Answer {
    fn answer(&mut self, byte: u8);
}

/// Why this copy does not listen.
#[derive(Debug)]
pub enum ListenError {
    /// Another copy listens.
    Held,
    /// No place for the channel: no runtime or config directory.
    Nowhere,
    /// Its directory is somebody else's, or others may enter it.
    NotPrivate,
    Io(std::io::Error),
}

impl fmt::Display for ListenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ListenError::Held => f.write_str("another copy takes them"),
            ListenError::Nowhere => f.write_str("there is no directory to listen in"),
            ListenError::NotPrivate => {
                f.write_str("the directory to listen in is not this user's alone")
            }
            ListenError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl From<std::io::Error> for ListenError {
    fn from(e: std::io::Error) -> Self {
        ListenError::Io(e)
    }
}

/// Why a link was not handed over: the copy starts on its own.
#[derive(Debug)]
pub enum NotHanded {
    /// No copy listens.
    NoneRunning,
    /// The running copy did not take it.
    NotTaken,
    /// The channel is not this user's alone.
    NotPrivate,
    /// No answer in time.
    TooSlow,
    Io(std::io::Error),
}

impl fmt::Display for NotHanded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotHanded::NoneRunning => f.write_str("no copy is running"),
            NotHanded::NotTaken => f.write_str("the copy running did not take the link"),
            NotHanded::NotPrivate => f.write_str("the copy running is not this user's alone"),
            NotHanded::TooSlow => f.write_str("the copy running did not answer in time"),
            NotHanded::Io(e) => write!(f, "{e}"),
        }
    }
}

/// Hands a link's text to the copy running, within `HAND_OVER_WITHIN` in all. `Ok` once it
/// was taken; on anything else this copy starts on its own.
pub fn hand_over(text: &Handed) -> Result<(), NotHanded> {
    hand_over_to(sys::Place::default_for_user(), text)
}

fn hand_over_to(place: Option<sys::Place>, text: &Handed) -> Result<(), NotHanded> {
    let place = place.ok_or(NotHanded::NoneRunning)?;
    let message = frame(&text.0).ok_or(NotHanded::NotTaken)?;
    let abandoned = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let gave_up = abandoned.clone();
    // On a thread, so that nothing it waits on (a connect to a full backlog) can hold up the
    // start: past the bound it is left, and it writes nothing once it is.
    std::thread::Builder::new()
        .name("link hand-over".into())
        .spawn(move || {
            let _ = tx.send(sys::send(&place, &message, &gave_up));
        })
        .map_err(NotHanded::Io)?;
    match rx.recv_timeout(HAND_OVER_WITHIN) {
        Ok(Ok(true)) => Ok(()),
        Ok(Ok(false)) => Err(NotHanded::NotTaken),
        Ok(Err(e)) => Err(e),
        Err(_) => {
            abandoned.store(true, Ordering::SeqCst);
            Err(NotHanded::TooSlow)
        }
    }
}

/// Starts listening, when no other copy does: the accepting thread, and the queue it fills.
pub fn listen(beat: Arc<Heartbeat>) -> Result<Receiver<Link>, ListenError> {
    listen_at(sys::Place::default_for_user(), beat)
}

fn listen_at(
    place: Option<sys::Place>,
    beat: Arc<Heartbeat>,
) -> Result<Receiver<Link>, ListenError> {
    let listener = sys::Listener::new(&place.ok_or(ListenError::Nowhere)?)?;
    let (tx, rx) = mpsc::sync_channel(QUEUE);
    std::thread::Builder::new()
        .name("link channel".into())
        .spawn(move || listener.serve(|conn| receive(conn, &beat, &tx)))?;
    Ok(rx)
}

/// Whether a socket found where the channel goes is in use, as a connect to it said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    /// Nothing there.
    Nothing,
    /// Left by a copy that is gone (the connect was refused): replaced.
    Stale,
    /// Answered, or could not be told: left alone, and this copy does not listen.
    Live,
}

/// What a connect to the socket found there says of it.
pub fn found(connect: Result<(), std::io::ErrorKind>) -> Found {
    match connect {
        Err(std::io::ErrorKind::NotFound) => Found::Nothing,
        Err(std::io::ErrorKind::ConnectionRefused) => Found::Stale,
        _ => Found::Live,
    }
}

/// Whether a directory is fit for the channel: a directory (not a link to one), the user's own,
/// and nobody else's to enter (`mode`'s group and other bits all clear).
pub fn is_private(is_dir: bool, owner: u32, user: u32, mode: u32) -> bool {
    is_dir && owner == user && mode & 0o077 == 0
}

/// Where the channel goes on Linux: the runtime directory's `bingle-3d`, else the config
/// directory's `run`.
pub fn dir_from(
    runtime: Option<std::ffi::OsString>,
    config: Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    match runtime
        .filter(|r| !r.is_empty())
        .map(std::path::PathBuf::from)
    {
        Some(r) if r.is_absolute() => Some(r.join("bingle-3d")),
        _ => config.map(|c| c.join("run")),
    }
}

#[cfg(target_os = "linux")]
mod sys {
    use std::fs::{self, DirBuilder, File, OpenOptions, Permissions};
    use std::io::{self, Read, Write};
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use super::{EACH_WITHIN, Found, ListenError, NotHanded, READ_WITHIN, TAKEN, TimedRead};

    const SOCKET: &str = "links.sock";
    const LOCK: &str = "links.lock";

    /// The directory the socket and its lock are in.
    #[derive(Debug, Clone)]
    pub struct Place(pub PathBuf);

    impl Place {
        pub fn default_for_user() -> Option<Place> {
            super::dir_from(
                std::env::var_os("XDG_RUNTIME_DIR"),
                crate::config::Store::default_dir(),
            )
            .map(Place)
        }
    }

    /// The user this process runs as: `/proc/self` is owned by it.
    fn user() -> io::Result<u32> {
        Ok(fs::metadata("/proc/self")?.uid())
    }

    /// Checks the directory is the user's alone, making it (0700) first when asked.
    fn private(dir: &Path, make: bool) -> Result<(), ListenError> {
        if make {
            if let Some(parent) = dir.parent() {
                fs::create_dir_all(parent)?;
            }
            match DirBuilder::new().mode(0o700).create(dir) {
                Err(e) if e.kind() != io::ErrorKind::AlreadyExists => return Err(e.into()),
                _ => {}
            }
        }
        let meta = fs::symlink_metadata(dir)?;
        if super::is_private(meta.is_dir(), meta.uid(), user()?, meta.mode()) {
            Ok(())
        } else {
            Err(ListenError::NotPrivate)
        }
    }

    pub struct Listener {
        socket: UnixListener,
        /// Held while this copy runs: the copy that holds it is the one that listens.
        _lock: File,
    }

    impl Listener {
        pub fn new(place: &Place) -> Result<Listener, ListenError> {
            let dir = &place.0;
            private(dir, true)?;
            let lock = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(false)
                .mode(0o600)
                .open(dir.join(LOCK))?;
            match lock.try_lock() {
                Ok(()) => {}
                Err(fs::TryLockError::WouldBlock) => return Err(ListenError::Held),
                Err(fs::TryLockError::Error(e)) => return Err(e.into()),
            }
            let path = dir.join(SOCKET);
            match super::found(UnixStream::connect(&path).map(drop).map_err(|e| e.kind())) {
                Found::Nothing => {}
                Found::Stale => fs::remove_file(&path)?,
                Found::Live => return Err(ListenError::Held),
            }
            let socket = UnixListener::bind(&path)?;
            // The directory keeps everyone else out already; this is the socket's own.
            fs::set_permissions(&path, Permissions::from_mode(0o600))?;
            Ok(Listener {
                socket,
                _lock: lock,
            })
        }

        pub fn serve(self, mut each: impl FnMut(&mut Conn)) {
            for stream in self.socket.incoming() {
                match stream {
                    Ok(stream) => each(&mut Conn(stream)),
                    Err(e) => {
                        bevy::log::warn!("game links: {e}");
                        std::thread::sleep(Duration::from_millis(50));
                    }
                }
            }
        }
    }

    pub struct Conn(UnixStream);

    impl TimedRead for Conn {
        fn read_within(&mut self, buf: &mut [u8], within: Duration) -> io::Result<usize> {
            self.0
                .set_read_timeout(Some(within.max(Duration::from_millis(1))))?;
            self.0.read(buf)
        }
    }

    impl super::Answer for Conn {
        fn answer(&mut self, byte: u8) {
            let _ = self.0.set_write_timeout(Some(READ_WITHIN));
            let _ = self.0.write_all(&[byte]);
            let _ = self.0.shutdown(std::net::Shutdown::Both);
        }
    }

    /// Sends a message to the copy listening at `place`, and reads whether it was taken.
    pub fn send(place: &Place, message: &[u8], abandoned: &AtomicBool) -> Result<bool, NotHanded> {
        let dir = &place.0;
        match private(dir, false) {
            Ok(()) => {}
            Err(ListenError::Io(e)) if e.kind() == io::ErrorKind::NotFound => {
                return Err(NotHanded::NoneRunning);
            }
            Err(ListenError::Io(e)) => return Err(NotHanded::Io(e)),
            Err(_) => return Err(NotHanded::NotPrivate),
        }
        let mut stream = UnixStream::connect(dir.join(SOCKET)).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => NotHanded::NoneRunning,
            _ => NotHanded::Io(e),
        })?;
        if abandoned.load(Ordering::SeqCst) {
            return Err(NotHanded::TooSlow);
        }
        stream
            .set_write_timeout(Some(EACH_WITHIN))
            .and_then(|()| stream.set_read_timeout(Some(EACH_WITHIN)))
            .and_then(|()| stream.write_all(message))
            .and_then(|()| stream.shutdown(std::net::Shutdown::Write))
            .map_err(NotHanded::Io)?;
        let mut answer = [0u8; 1];
        Ok(matches!(stream.read(&mut answer), Ok(1) if answer[0] == TAKEN))
    }
}

#[cfg(windows)]
mod sys {
    use std::io;
    use std::ptr::{null, null_mut};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, ERROR_BROKEN_PIPE, ERROR_FILE_NOT_FOUND,
        ERROR_IO_PENDING, ERROR_NO_DATA, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GENERIC_READ,
        GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE, LocalFree, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        GetSecurityInfo, SDDL_REVISION_1, SE_KERNEL_OBJECT,
    };
    use windows_sys::Win32::Security::{
        EqualSid, GetTokenInformation, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
        SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, OPEN_EXISTING,
        PIPE_ACCESS_DUPLEX, READ_CONTROL, ReadFile, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
        WriteFile,
    };
    use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeServerProcessId,
        PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT, WaitNamedPipeW,
    };
    use windows_sys::Win32::System::Threading::{
        CreateEventW, GetCurrentProcess, OpenProcessToken, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

    use super::{EACH_WITHIN, ListenError, MAX_MESSAGE, NotHanded, READ_WITHIN, TAKEN, TimedRead};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// A handle, closed when dropped.
    struct Owned(HANDLE);

    // SAFETY: a kernel handle may be used from any thread.
    unsafe impl Send for Owned {}

    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: the handle is open and owned here alone.
            unsafe { CloseHandle(self.0) };
        }
    }

    /// The current user's `TOKEN_USER`, in a buffer aligned for it.
    struct User(Vec<u64>);

    impl User {
        fn current() -> io::Result<User> {
            let mut token: HANDLE = null_mut();
            // SAFETY: the current process's pseudo-handle; `token` receives a handle we close.
            if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let token = Owned(token);
            let mut len = 0u32;
            // SAFETY: asks only for the size.
            unsafe { GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut len) };
            let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
            // SAFETY: `buf` holds at least `len` bytes, aligned for `TOKEN_USER`.
            let ok = unsafe {
                GetTokenInformation(token.0, TokenUser, buf.as_mut_ptr().cast(), len, &mut len)
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(User(buf))
        }

        fn sid(&self) -> PSID {
            // SAFETY: the buffer holds a `TOKEN_USER` filled by the system.
            unsafe { (*self.0.as_ptr().cast::<TOKEN_USER>()).User.Sid }
        }

        /// The SID written out (`S-1-5-21-…`): the pipe's name and its descriptor say it.
        fn sid_string(&self) -> io::Result<String> {
            let mut text = null_mut();
            // SAFETY: a valid SID; the string it gives is freed with `LocalFree`.
            if unsafe { ConvertSidToStringSidW(self.sid(), &mut text) } == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: a NUL-terminated string from the system.
            let s = unsafe {
                let len = (0..).take_while(|&i| *text.add(i) != 0).count();
                String::from_utf16_lossy(std::slice::from_raw_parts(text, len))
            };
            // SAFETY: allocated by the call above.
            unsafe { LocalFree(text.cast()) };
            Ok(s)
        }
    }

    /// The pipe's name, the user's own: nothing in it is secret.
    #[derive(Debug, Clone)]
    pub struct Place(pub String);

    impl Place {
        pub fn default_for_user() -> Option<Place> {
            let sid = User::current().and_then(|u| u.sid_string()).ok()?;
            Some(Place(format!(r"\\.\pipe\bingle-3d-links-{sid}")))
        }
    }

    pub struct Listener {
        pipe: Owned,
        event: Owned,
    }

    impl Listener {
        pub fn new(place: &Place) -> Result<Listener, ListenError> {
            let user = User::current()?;
            let sid = user.sid_string()?;
            // Owned by the user, and a protected list granting only the user: the default
            // descriptor would also let Everyone and anonymous read.
            let sddl = wide(&format!("O:{sid}D:P(A;;GA;;;{sid})"));
            let mut sd: PSECURITY_DESCRIPTOR = null_mut();
            // SAFETY: a NUL-terminated descriptor string; `sd` is freed below.
            let ok = unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut sd,
                    null_mut(),
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error().into());
            }
            let attributes = SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: sd,
                bInheritHandle: 0,
            };
            let name = wide(&place.0);
            // SAFETY: the name and the attributes live across the call.
            let pipe = unsafe {
                CreateNamedPipeW(
                    name.as_ptr(),
                    PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    1,
                    64,
                    2 * MAX_MESSAGE as u32,
                    0,
                    &attributes,
                )
            };
            // SAFETY: the system has copied the descriptor into the pipe.
            let error = unsafe { GetLastError() };
            unsafe { LocalFree(sd) };
            if pipe == INVALID_HANDLE_VALUE {
                // A pipe of that name is there already: another copy's, or a program's that
                // must not be mistaken for ours.
                return Err(match error {
                    ERROR_ACCESS_DENIED | ERROR_PIPE_BUSY => ListenError::Held,
                    e => io::Error::from_raw_os_error(e as i32).into(),
                });
            }
            let pipe = Owned(pipe);
            // SAFETY: a manual-reset event with no name.
            let event = unsafe { CreateEventW(null(), 1, 0, null()) };
            if event.is_null() {
                return Err(io::Error::last_os_error().into());
            }
            Ok(Listener {
                pipe,
                event: Owned(event),
            })
        }

        pub fn serve(self, mut each: impl FnMut(&mut Conn)) {
            loop {
                // SAFETY: an OVERLAPPED is plain data, zero is its empty state.
                let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
                ov.hEvent = self.event.0;
                // SAFETY: `ov` lives until the connect is over (waited for below).
                let connected = unsafe { ConnectNamedPipe(self.pipe.0, &mut ov) } != 0 || {
                    match unsafe { GetLastError() } {
                        ERROR_PIPE_CONNECTED => true,
                        ERROR_IO_PENDING => {
                            let mut n = 0;
                            // SAFETY: waits for the connect `ov` describes.
                            unsafe { GetOverlappedResult(self.pipe.0, &ov, &mut n, 1) != 0 }
                        }
                        // Connected and gone again.
                        ERROR_NO_DATA => false,
                        e => {
                            bevy::log::warn!(
                                "game links: {}",
                                io::Error::from_raw_os_error(e as i32)
                            );
                            std::thread::sleep(Duration::from_millis(50));
                            false
                        }
                    }
                };
                if connected {
                    each(&mut Conn {
                        pipe: self.pipe.0,
                        event: self.event.0,
                    });
                }
                // SAFETY: the pipe is ours; this ends the connection, if any.
                unsafe { DisconnectNamedPipe(self.pipe.0) };
            }
        }
    }

    /// The connection the pipe has now.
    pub struct Conn {
        pipe: HANDLE,
        event: HANDLE,
    }

    impl Conn {
        /// Reads or writes, overlapped, waiting at most `within`; cancelled past it.
        fn io(
            &mut self,
            within: Duration,
            start: impl FnOnce(*mut OVERLAPPED) -> i32,
        ) -> io::Result<usize> {
            // SAFETY: plain data.
            let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
            ov.hEvent = self.event;
            if start(&mut ov) == 0 {
                match unsafe { GetLastError() } {
                    ERROR_IO_PENDING => {
                        let ms = within.as_millis().clamp(1, u32::MAX as u128 - 1) as u32;
                        // SAFETY: the event is ours, set when the operation ends.
                        if unsafe { WaitForSingleObject(self.event, ms) } != WAIT_OBJECT_0 {
                            let mut n = 0;
                            // SAFETY: cancels this operation, and waits for it to be over, so
                            // `ov` and the buffer are not used after.
                            unsafe {
                                CancelIoEx(self.pipe, &ov);
                                GetOverlappedResult(self.pipe, &ov, &mut n, 1);
                            }
                            return Err(io::ErrorKind::TimedOut.into());
                        }
                    }
                    ERROR_BROKEN_PIPE => return Ok(0),
                    e => return Err(io::Error::from_raw_os_error(e as i32)),
                }
            }
            let mut n = 0;
            // SAFETY: the operation is over.
            if unsafe { GetOverlappedResult(self.pipe, &ov, &mut n, 0) } == 0 {
                return match unsafe { GetLastError() } {
                    ERROR_BROKEN_PIPE => Ok(0),
                    e => Err(io::Error::from_raw_os_error(e as i32)),
                };
            }
            Ok(n as usize)
        }
    }

    impl TimedRead for Conn {
        fn read_within(&mut self, buf: &mut [u8], within: Duration) -> io::Result<usize> {
            let pipe = self.pipe;
            let (ptr, len) = (buf.as_mut_ptr(), buf.len() as u32);
            // SAFETY: `buf` outlives the read, which `io` waits for or cancels.
            self.io(within, |ov| unsafe {
                ReadFile(pipe, ptr, len, null_mut(), ov)
            })
        }
    }

    impl super::Answer for Conn {
        fn answer(&mut self, byte: u8) {
            let pipe = self.pipe;
            let answer = [byte];
            // SAFETY: `answer` outlives the write, which `io` waits for or cancels.
            let _ = self.io(READ_WITHIN, |ov| unsafe {
                WriteFile(pipe, answer.as_ptr(), 1, null_mut(), ov)
            });
            // Disconnecting drops what the other end has not read: wait, bounded, for it to
            // close first.
            let mut rest = [0u8; 16];
            let deadline = std::time::Instant::now() + READ_WITHIN;
            loop {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() || !matches!(self.read_within(&mut rest, left), Ok(n) if n > 0) {
                    break;
                }
            }
        }
    }

    /// Whether a pipe the user opened is the user's own: its owner is the user.
    fn owned_by_user(pipe: HANDLE, user: &User) -> bool {
        let mut owner: PSID = null_mut();
        let mut sd: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: the handle was opened with READ_CONTROL; `sd` is freed below and `owner`
        // points into it.
        let error = unsafe {
            GetSecurityInfo(
                pipe,
                SE_KERNEL_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                null_mut(),
                null_mut(),
                &mut sd,
            )
        };
        if error != 0 {
            return false;
        }
        // SAFETY: both SIDs are valid while `sd` lives.
        let same = unsafe { EqualSid(owner, user.sid()) } != 0;
        unsafe { LocalFree(sd) };
        same
    }

    /// Sends a message to the copy listening at `place`, and reads whether it was taken.
    pub fn send(place: &Place, message: &[u8], abandoned: &AtomicBool) -> Result<bool, NotHanded> {
        let user = User::current().map_err(NotHanded::Io)?;
        let name = wide(&place.0);
        let open = || {
            // SAFETY: a NUL-terminated name. Identification only: the copy listening cannot
            // act as this user.
            unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE | READ_CONTROL,
                    0,
                    null(),
                    OPEN_EXISTING,
                    SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION | FILE_FLAG_OVERLAPPED,
                    null_mut(),
                )
            }
        };
        let mut pipe = open();
        if pipe == INVALID_HANDLE_VALUE && unsafe { GetLastError() } == ERROR_PIPE_BUSY {
            // Another link is being read: wait for it, bounded, once.
            let ms = EACH_WITHIN.as_millis() as u32;
            // SAFETY: a NUL-terminated name.
            if unsafe { WaitNamedPipeW(name.as_ptr(), ms) } != 0 {
                pipe = open();
            }
        }
        if pipe == INVALID_HANDLE_VALUE {
            return Err(match unsafe { GetLastError() } {
                ERROR_FILE_NOT_FOUND | ERROR_PIPE_BUSY => NotHanded::NoneRunning,
                ERROR_ACCESS_DENIED => NotHanded::NotPrivate,
                e => NotHanded::Io(io::Error::from_raw_os_error(e as i32)),
            });
        }
        let pipe = Owned(pipe);
        if !owned_by_user(pipe.0, &user) {
            return Err(NotHanded::NotPrivate);
        }
        // This copy was started by the program in front (the browser): it may let the running
        // copy come forward.
        let mut server = 0u32;
        // SAFETY: an open pipe handle.
        if unsafe { GetNamedPipeServerProcessId(pipe.0, &mut server) } != 0 {
            unsafe { AllowSetForegroundWindow(server) };
        }
        if abandoned.load(Ordering::SeqCst) {
            return Err(NotHanded::TooSlow);
        }
        // SAFETY: a manual-reset event with no name.
        let event = unsafe { CreateEventW(null(), 1, 0, null()) };
        if event.is_null() {
            return Err(NotHanded::Io(io::Error::last_os_error()));
        }
        let event = Owned(event);
        let mut conn = Conn {
            pipe: pipe.0,
            event: event.0,
        };
        let p = pipe.0;
        let (ptr, len) = (message.as_ptr(), message.len() as u32);
        // SAFETY: `message` outlives the write, which `io` waits for or cancels.
        let written = conn
            .io(EACH_WITHIN, |ov| unsafe {
                WriteFile(p, ptr, len, null_mut(), ov)
            })
            .map_err(NotHanded::Io)?;
        if written != message.len() {
            return Err(NotHanded::NotTaken);
        }
        let mut answer = [0u8; 1];
        Ok(matches!(conn.read_within(&mut answer, EACH_WITHIN), Ok(1) if answer[0] == TAKEN))
    }
}

/// No channel on any other system: every copy starts on its own.
#[cfg(not(any(target_os = "linux", windows)))]
mod sys {
    use std::sync::atomic::AtomicBool;

    use super::{ListenError, NotHanded, TimedRead};

    #[derive(Debug, Clone)]
    pub struct Place;

    impl Place {
        pub fn default_for_user() -> Option<Place> {
            None
        }
    }

    pub struct Listener;

    impl Listener {
        pub fn new(_: &Place) -> Result<Listener, ListenError> {
            Err(ListenError::Nowhere)
        }

        pub fn serve(self, _: impl FnMut(&mut Conn)) {}
    }

    pub struct Conn;

    impl TimedRead for Conn {
        fn read_within(&mut self, _: &mut [u8], _: std::time::Duration) -> std::io::Result<usize> {
            Ok(0)
        }
    }

    impl super::Answer for Conn {
        fn answer(&mut self, _: u8) {}
    }

    pub fn send(_: &Place, _: &[u8], _: &AtomicBool) -> Result<bool, NotHanded> {
        Err(NotHanded::NoneRunning)
    }
}

/// Listens, in a run `listens` allows, and keeps the heartbeat.
pub struct LinkChannelPlugin {
    pub listen: bool,
}

/// The links handed over by other copies, waiting for the next frame (`link_open::take_handed`).
#[derive(Resource)]
pub struct Inbox(pub Mutex<Receiver<Link>>);

/// Asks for the window to come forward, next frame (`bring_forward`).
#[derive(Resource, Default)]
pub struct Forward(pub bool);

#[derive(Resource)]
struct Beat(Arc<Heartbeat>);

impl Plugin for LinkChannelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Forward>()
            .add_systems(Last, bring_forward);
        if !self.listen {
            return;
        }
        let beat = Arc::new(Heartbeat::new());
        beat.beat();
        match listen(beat.clone()) {
            Ok(links) => {
                info!("game links: links clicked while this copy runs come here");
                app.insert_resource(Inbox(Mutex::new(links)))
                    .insert_resource(Beat(beat))
                    .add_systems(First, |beat: Res<Beat>| beat.0.beat());
            }
            Err(why) => info!("game links: this copy does not listen for links: {why}"),
        }
    }
}

/// Brings the window forward as far as the system lets a program do unasked: unminimised,
/// focus asked for (X11, and Windows once the handing copy allowed it), and the player's
/// attention asked for (a flash of the taskbar button, an urgent hint, Wayland's
/// xdg-activation), where focus is not given.
fn bring_forward(
    mut forward: ResMut<Forward>,
    mut windows: Query<(Entity, &mut Window), With<bevy::window::PrimaryWindow>>,
    _main_thread: bevy::ecs::system::NonSendMarker,
) {
    if !forward.0 {
        return;
    }
    forward.0 = false;
    let Ok((entity, mut window)) = windows.single_mut() else {
        return;
    };
    window.set_minimized(false);
    window.focused = true;
    bevy::winit::WINIT_WINDOWS.with_borrow(|winit| {
        if let Some(w) = winit.get_window(entity) {
            w.request_user_attention(Some(Default::default()));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    const LINK: &str = "bingle-3d://join?server=https%3A%2F%2Fgames.example.com&room=K7MQ2X";

    /// A connection that gives its chunks one read at a time, then stalls or closes.
    struct Chunks {
        chunks: Vec<Vec<u8>>,
        stall: bool,
    }

    impl TimedRead for Chunks {
        fn read_within(&mut self, buf: &mut [u8], within: Duration) -> io::Result<usize> {
            if self.chunks.is_empty() {
                if self.stall {
                    std::thread::sleep(within);
                    return Err(io::ErrorKind::TimedOut.into());
                }
                return Ok(0);
            }
            let mut chunk = self.chunks.remove(0);
            let n = chunk.len().min(buf.len());
            buf[..n].copy_from_slice(&chunk[..n]);
            if n < chunk.len() {
                self.chunks.insert(0, chunk.split_off(n));
            }
            Ok(n)
        }
    }

    fn read(chunks: &[&[u8]], stall: bool) -> Result<String, Refused> {
        let mut conn = Chunks {
            chunks: chunks.iter().map(|c| c.to_vec()).collect(),
            stall,
        };
        read_message(&mut conn, Instant::now() + Duration::from_millis(50))
    }

    #[test]
    fn a_message_is_a_link_and_its_newline() {
        let message = frame(LINK).unwrap();
        assert_eq!(message.last(), Some(&b'\n'));
        assert_eq!(read(&[&message], false).unwrap(), LINK);
        // However it comes apart on the way.
        let (a, b) = message.split_at(10);
        assert_eq!(read(&[a, b], true).unwrap(), LINK);
        assert!(frame(&"a".repeat(link::MAX_LEN + 1)).is_none());
        assert!(frame("bingle-3d://x\nmore").is_none());
        assert!(frame(&"a".repeat(link::MAX_LEN)).is_some());
    }

    #[test]
    fn the_read_is_bounded_and_takes_nothing_else() {
        let long = vec![b'a'; MAX_MESSAGE + 10];
        assert_eq!(read(&[&long], true), Err(Refused::TooLong));
        assert_eq!(read(&[b"bingle-3d://join"], false), Err(Refused::Unended));
        assert_eq!(
            read(&[b"bingle-3d://join\nmore"], false),
            Err(Refused::Trailing)
        );
        assert_eq!(read(&[&[0xff, 0xfe, b'\n']], false), Err(Refused::NotText));
        let started = Instant::now();
        assert_eq!(read(&[b"bingle-3d://"], true), Err(Refused::Late));
        assert_eq!(read(&[], true), Err(Refused::Late));
        assert!(started.elapsed() < Duration::from_millis(500), "bounded");
        // None of the reasons repeats what came.
        for why in [Refused::TooLong, Refused::Trailing, Refused::Late] {
            assert!(!why.to_string().contains("bingle"));
        }
    }

    /// A connection that records the answer.
    struct Recorded {
        conn: Chunks,
        answer: Option<u8>,
    }

    impl TimedRead for Recorded {
        fn read_within(&mut self, buf: &mut [u8], within: Duration) -> io::Result<usize> {
            self.conn.read_within(buf, within)
        }
    }

    impl Answer for Recorded {
        fn answer(&mut self, byte: u8) {
            self.answer = Some(byte);
        }
    }

    fn received(message: &[u8], beat: &Heartbeat, queue: &SyncSender<Link>) -> Option<u8> {
        let mut conn = Recorded {
            conn: Chunks {
                chunks: vec![message.to_vec()],
                stall: false,
            },
            answer: None,
        };
        receive(&mut conn, beat, queue);
        conn.answer
    }

    #[test]
    fn only_a_link_that_parses_is_taken_and_the_queue_is_bounded() {
        let beat = Heartbeat::new();
        beat.beat();
        let (tx, rx) = mpsc::sync_channel(QUEUE);
        assert_eq!(received(&frame(LINK).unwrap(), &beat, &tx), Some(TAKEN));
        assert_eq!(rx.try_recv().unwrap().room, "K7MQ2X");
        // Not a link the contract has: dropped.
        let other = frame("bingle-3d://open?server=x&room=ABCD").unwrap();
        assert_eq!(received(&other, &beat, &tx), Some(NOT_TAKEN));
        assert_eq!(received(b"--private-dir\n", &beat, &tx), Some(NOT_TAKEN));
        assert!(rx.try_recv().is_err());
        // A flood fills the queue, and the rest is dropped, never waited on.
        for _ in 0..QUEUE {
            assert_eq!(received(&frame(LINK).unwrap(), &beat, &tx), Some(TAKEN));
        }
        let started = Instant::now();
        assert_eq!(received(&frame(LINK).unwrap(), &beat, &tx), Some(NOT_TAKEN));
        assert!(started.elapsed() < Duration::from_millis(100));
        assert_eq!(rx.try_iter().count(), QUEUE);
    }

    #[test]
    fn a_copy_whose_frames_stopped_takes_nothing() {
        let beat = Heartbeat {
            start: Instant::now() - STALLED * 2,
            last_ms: AtomicU64::new(0),
        };
        assert!(!beat.alive());
        let (tx, rx) = mpsc::sync_channel(QUEUE);
        assert_eq!(received(&frame(LINK).unwrap(), &beat, &tx), Some(NOT_TAKEN));
        assert!(rx.try_recv().is_err());
        beat.beat();
        assert!(beat.alive());
    }

    #[test]
    fn a_stale_socket_is_replaced_and_a_live_one_left_alone() {
        use io::ErrorKind::*;
        assert_eq!(found(Err(NotFound)), Found::Nothing);
        assert_eq!(found(Err(ConnectionRefused)), Found::Stale);
        assert_eq!(found(Ok(())), Found::Live);
        // Anything else cannot be told: left alone.
        assert_eq!(found(Err(PermissionDenied)), Found::Live);
    }

    #[test]
    fn the_directory_is_the_users_alone() {
        assert!(is_private(true, 1000, 1000, 0o40700));
        assert!(!is_private(true, 1000, 1000, 0o40750));
        assert!(!is_private(true, 1000, 1000, 0o40701));
        assert!(!is_private(true, 0, 1000, 0o40700), "somebody else's");
        assert!(!is_private(false, 1000, 1000, 0o120777), "a link to one");
    }

    #[test]
    fn the_runtime_directory_first_then_the_configs() {
        let config = Some(std::path::PathBuf::from("/c/bingle-3d"));
        assert_eq!(
            dir_from(Some("/run/user/1000".into()), config.clone()),
            Some("/run/user/1000/bingle-3d".into())
        );
        assert_eq!(
            dir_from(None, config.clone()),
            Some("/c/bingle-3d/run".into())
        );
        assert_eq!(
            dir_from(Some("".into()), config.clone()),
            Some("/c/bingle-3d/run".into())
        );
        // A relative runtime directory is not one.
        assert_eq!(
            dir_from(Some("run".into()), config),
            Some("/c/bingle-3d/run".into())
        );
        assert_eq!(dir_from(None, None), None);
    }

    fn opts(args: &[&str]) -> crate::cli::Options {
        crate::cli::Options::parse(args.iter().copied()).unwrap()
    }

    #[test]
    fn only_a_link_hands_over_and_only_a_players_start_listens() {
        assert!(hands_over(&opts(&["--link", LINK])).is_some());
        assert!(hands_over(&opts(&[])).is_none());
        assert!(hands_over(&opts(&["--frames", "5", "--link", LINK])).is_none());
        assert!(hands_over(&opts(&["--link", "bingle-3d://nothing"])).is_none());
        let players = |args: &[&str]| listens(&opts(args), false);
        let supported = crate::link_handler::supported();
        assert_eq!(players(&[]), supported);
        assert_eq!(players(&["--link", LINK]), supported);
        assert_eq!(players(&["--sign-in"]), supported);
        for scripted in [
            &["--frames", "10"][..],
            &["--screenshot", "x.png"],
            &["--watch", "ABCD"],
            &["--join", "ABCD"],
            &["--watch-bots", "go"],
            &["--setup", "x.toml"],
            &["--new-game", "go"],
            &["--showcase"],
            &["--test-hand", "5"],
        ] {
            assert!(!players(scripted), "{scripted:?}");
        }
        assert!(!listens(&opts(&[]), true), "not in a headset");
    }

    #[test]
    fn a_handed_link_is_never_shown() {
        let handed = Handed(format!(
            "{LINK}&code=bgc_AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8"
        ));
        assert!(!format!("{handed:?}").contains("bgc_"));
        assert!(!format!("{:?}", opts(&["--link", &handed.0])).contains("bgc_"));
    }

    /// A directory of the test's own, short enough for a socket's path.
    #[cfg(target_os = "linux")]
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("b3d-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn one_listener_at_a_time_and_a_stale_socket_replaced() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("listen");
        let place = sys::Place(root.join("bingle-3d"));
        let beat = Arc::new(Heartbeat::new());
        beat.beat();
        // A socket left by a copy that is gone.
        std::fs::create_dir(&place.0).unwrap();
        std::fs::set_permissions(&place.0, std::fs::Permissions::from_mode(0o700)).unwrap();
        drop(std::os::unix::net::UnixListener::bind(place.0.join("links.sock")).unwrap());
        let links = listen_at(Some(place.clone()), beat.clone()).unwrap();
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode();
        assert_eq!(mode(&place.0) & 0o777, 0o700);
        assert_eq!(mode(&place.0.join("links.sock")) & 0o777, 0o600);
        // A second listener never takes the name.
        assert!(matches!(
            listen_at(Some(place.clone()), beat.clone()),
            Err(ListenError::Held)
        ));
        hand_over_to(Some(place.clone()), &Handed(LINK.into())).unwrap();
        let link = links.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(link.room, "K7MQ2X");
        // A directory others may enter is not listened in, nor written to.
        let open = sys::Place(root.join("open"));
        std::fs::create_dir(&open.0).unwrap();
        std::fs::set_permissions(&open.0, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            listen_at(Some(open.clone()), beat),
            Err(ListenError::NotPrivate)
        ));
        assert!(matches!(
            hand_over_to(Some(open), &Handed(LINK.into())),
            Err(NotHanded::NotPrivate)
        ));
        // Nobody listening: the start goes on at once.
        let started = Instant::now();
        assert!(matches!(
            hand_over_to(Some(sys::Place(root.join("none"))), &Handed(LINK.into())),
            Err(NotHanded::NoneRunning)
        ));
        assert!(started.elapsed() < HAND_OVER_WITHIN);
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_copy_that_does_not_answer_holds_nothing_up() {
        let root = scratch("silent");
        let place = sys::Place(root.join("bingle-3d"));
        std::fs::create_dir(&place.0).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&place.0, std::fs::Permissions::from_mode(0o700)).unwrap();
        // A socket that is listened on but never read.
        let _silent = std::os::unix::net::UnixListener::bind(place.0.join("links.sock")).unwrap();
        let started = Instant::now();
        let result = hand_over_to(Some(place), &Handed(LINK.into()));
        assert!(
            matches!(result, Err(NotHanded::NotTaken | NotHanded::TooSlow)),
            "{result:?}"
        );
        assert!(started.elapsed() < HAND_OVER_WITHIN + Duration::from_millis(100));
        let _ = std::fs::remove_dir_all(root);
    }

    /// Two real processes: this test binary started twice, one listening, one handing a link
    /// over and quitting, each with a runtime and config directory of its own.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_second_process_hands_its_link_to_the_first() {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};
        let root = scratch("two");
        let exe = std::env::current_exe().unwrap();
        let child = |role: &str| {
            let mut c = Command::new(&exe);
            c.args([
                "--exact",
                "link_channel::tests::process",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("B3D_CHANNEL_ROLE", role)
            .env("XDG_RUNTIME_DIR", &root)
            .env("BINGLE3D_CONFIG_DIR", root.join("config"))
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
            c
        };
        let mut first = child("listen").spawn().unwrap();
        // libtest's own words may come first on a line.
        let mut lines = BufReader::new(first.stdout.take().unwrap()).lines();
        let ready = lines
            .by_ref()
            .map_while(Result::ok)
            .any(|l| l.ends_with("listening"));
        assert!(ready, "the first process listens");
        let started = Instant::now();
        let second = child("hand").output().unwrap();
        let took = started.elapsed();
        let said = String::from_utf8_lossy(&second.stdout);
        assert!(second.status.success(), "{said}");
        assert!(said.contains("handed over"), "{said}");
        let got = lines
            .map_while(Result::ok)
            .any(|l| l.ends_with("received K7MQ2X"));
        let _ = first.kill();
        let _ = first.wait();
        let _ = std::fs::remove_dir_all(&root);
        assert!(got, "the first process received the link");
        // The child is a whole test binary starting; the hand-over itself is bounded.
        assert!(took < Duration::from_secs(10), "{took:?}");
    }

    /// One side of `a_second_process_hands_its_link_to_the_first`, as its role says.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "started by a_second_process_hands_its_link_to_the_first"]
    fn process() {
        match std::env::var("B3D_CHANNEL_ROLE").as_deref() {
            Ok("listen") => {
                let beat = Arc::new(Heartbeat::new());
                beat.beat();
                let links = listen(beat.clone()).unwrap();
                println!("listening");
                let deadline = Instant::now() + Duration::from_secs(20);
                while Instant::now() < deadline {
                    beat.beat();
                    if let Ok(link) = links.recv_timeout(Duration::from_millis(50)) {
                        println!("received {}", link.room);
                        return;
                    }
                }
            }
            Ok("hand") => {
                let started = Instant::now();
                hand_over(&Handed(LINK.into())).unwrap();
                assert!(started.elapsed() <= HAND_OVER_WITHIN);
                println!("handed over");
            }
            _ => {}
        }
    }

    /// The pipe made, a link passed as the same user, and a second listener refused the name.
    #[cfg(windows)]
    #[test]
    fn the_pipe_takes_a_link_from_the_same_user_and_keeps_its_name() {
        let place = sys::Place(format!(
            r"\\.\pipe\bingle-3d-links-test-{}",
            std::process::id()
        ));
        let beat = Arc::new(Heartbeat::new());
        beat.beat();
        let links = listen_at(Some(place.clone()), beat.clone()).unwrap();
        assert!(matches!(
            listen_at(Some(place.clone()), beat.clone()),
            Err(ListenError::Held)
        ));
        let started = Instant::now();
        hand_over_to(Some(place.clone()), &Handed(LINK.into())).unwrap();
        assert!(started.elapsed() <= HAND_OVER_WITHIN);
        assert_eq!(
            links.recv_timeout(Duration::from_secs(1)).unwrap().room,
            "K7MQ2X"
        );
        // Again, on the same instance.
        hand_over_to(Some(place.clone()), &Handed(LINK.into())).unwrap();
        assert!(links.recv_timeout(Duration::from_secs(1)).is_ok());
        assert!(matches!(
            hand_over_to(
                Some(sys::Place(r"\\.\pipe\bingle-3d-links-none".into())),
                &Handed(LINK.into())
            ),
            Err(NotHanded::NoneRunning)
        ));
        // The user's own pipe's name is per user.
        let mine = sys::Place::default_for_user().unwrap();
        assert!(
            mine.0.starts_with(r"\\.\pipe\bingle-3d-links-S-1-"),
            "{mine:?}"
        );
    }
}
