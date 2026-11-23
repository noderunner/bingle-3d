//! Command-line options.

use std::path::PathBuf;

pub const USAGE: &str = "\
Usage: bingle-3d [options]

Options:
  --server URL       the Bingle server for this run, in place of the configured one
                     (localhost:8080, https://games.example.com)
  --watch ROOM       watch the game in this room, once connected
  --watch-bots GAME  start a game between two of the server's bots and watch it
                     (GAME is a game definition's id, such as one the panel lists)
  --play-bot GAME    start a game against the server's bots and play the first seat
  --host GAME        start a game, play the first seat and wait for people to join it
  --join ROOM        take an open seat in the game in this room (or watch it, if it
                     has none)
  --new-game GAME    open the form that sets up a new game of GAME
  --sign-in          start on the login screen, even with auto-login on
  --friends          open the friends panel (signed in)
  --talk HANDLE      open the conversation with this friend (signed in)
  --call HANDLE      ring this friend once they are online, or answer them if they
                     are ringing (signed in)
  --voice            join the voice chat of the game you play, once seated
  --no-audio-devices open no microphone and no speaker in a call (for tests)
  --relay-only       connect calls only through the server's relay (TURN), as a
                     browser's iceTransportPolicy \"relay\": to try the relay, or to
                     keep your address from the others
  --voice-check      hear your microphone back through your speaker, over a voice
                     connection on this machine, for ten seconds (no window)
  --environment NAME where the table stands, for this run: room (a plain table) or
                     garden
  --view Y,P,R[,X,Z] where the camera starts: degrees round the table (0 is your
                     side), degrees above it, metres from what it looks at; that is
                     the table's middle, or the place X metres to its right and Z
                     towards you (for pictures)
  --showcase         lay every piece's model, some cards and the dice on the table,
                     with nothing else: to look at an asset (with --view, --frames,
                     --screenshot)
  --card-face FILE   in the showcase, a picture (JPEG or PNG, 5 to 7) for the cards'
                     faces
  --card-back FILE   and one for their backs, in place of the client's own
  --vr               draw the table in a headset too, through OpenXR (only in a build
                     with the vr feature, which does so by itself when SteamVR is
                     already running). Home sits you at the table again
  --desktop          stay in the window, even with SteamVR running
  --legal-moves      start with every position a click would be taken on lit (L)
  --no-bloom         start with the camera's bloom off (B turns it on), as a headset
                     sees the table
  --frames N         quit after N frames (for scripts and CI)
  --screenshot PATH  save a PNG of the window at the last frame, then quit
                     (at frame 60 unless --frames says otherwise)
  -h, --help         show this help";

/// The frame a screenshot is taken at when `--frames` does not say: late enough for the
/// first shadows and the window's first resize to have settled.
pub const DEFAULT_SCREENSHOT_FRAME: u32 = 60;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Options {
    /// Quit after this many frames.
    pub frames: Option<u32>,
    /// Save a screenshot here at the last frame, then quit.
    pub screenshot: Option<PathBuf>,
    /// A server for this run, in place of the configured one.
    pub server: Option<String>,
    /// What to watch once connected.
    pub watch: Option<Watch>,
    /// The game to set up a new game of, once connected.
    pub new_game: Option<String>,
    /// Start on the login screen, whatever the config says.
    pub sign_in: bool,
    /// Open the friends panel at start.
    pub friends: bool,
    /// Open the conversation with this friend at start.
    pub talk: Option<String>,
    /// Ring this friend at start.
    pub call: Option<String>,
    /// Join the voice chat of the game played.
    pub voice: bool,
    /// Calls open no microphone and no speaker.
    pub no_audio_devices: bool,
    /// Calls connect only through the server's relay.
    pub relay_only: bool,
    /// Try the microphone and the speaker, then quit.
    pub voice_check: bool,
    /// Where the table stands, for this run: an environment's id.
    pub environment: Option<String>,
    /// Where the camera starts, as `YAW,PITCH,RADIUS[,X,Z]`.
    pub view: Option<String>,
    /// Lay the piece models, cards and dice out on the table, to look at.
    pub showcase: bool,
    /// Pictures for the showcase's cards, from files.
    pub card_face: Option<PathBuf>,
    pub card_back: Option<PathBuf>,
    /// Draw in a headset too.
    pub vr: bool,
    /// Stay in the window, even with a headset waiting.
    pub desktop: bool,
    /// `--legal-moves`: the positions a click would be taken on start lit.
    pub legal_moves: bool,
    /// `--no-bloom`: the camera starts without bloom.
    pub no_bloom: bool,
    pub help: bool,
}

impl Options {
    /// Where the run starts: an option that does something on the server goes straight in,
    /// with what is kept; `--sign-in` shows the login screen; else as the config says.
    pub fn start(&self) -> crate::net::Start {
        if self.sign_in {
            crate::net::Start::Login
        } else if self.watch.is_some()
            || self.new_game.is_some()
            || self.friends
            || self.talk.is_some()
            || self.call.is_some()
        {
            crate::net::Start::StraightIn
        } else {
            crate::net::Start::AsConfigured
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Watch {
    Room(String),
    Bots(String),
    /// Play against the server's bots.
    PlayBots(String),
    /// Start a game for people to join.
    Host(String),
    /// Take a seat in a game, or watch it.
    Join(String),
}

impl Options {
    /// Parses the arguments after the program name. Both `--frames 10` and `--frames=10` work.
    pub fn parse<I, S>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut opts = Options::default();
        let mut args = args.into_iter().map(Into::into);
        while let Some(arg) = args.next() {
            let (name, inline) = match arg.split_once('=') {
                Some((n, v)) if n.starts_with("--") => (n.to_string(), Some(v.to_string())),
                _ => (arg.clone(), None),
            };
            let mut value = |what: &str| {
                inline
                    .clone()
                    .or_else(|| args.next())
                    .ok_or_else(|| format!("{name} needs {what}"))
            };
            match name.as_str() {
                "-h" | "--help" => opts.help = true,
                "--sign-in" => opts.sign_in = true,
                "--friends" => opts.friends = true,
                "--voice-check" => opts.voice_check = true,
                "--call" => opts.call = Some(value("a friend's handle")?),
                "--no-audio-devices" => opts.no_audio_devices = true,
                "--relay-only" => opts.relay_only = true,
                "--voice" => opts.voice = true,
                "--vr" => opts.vr = true,
                "--desktop" => opts.desktop = true,
                "--no-bloom" => opts.no_bloom = true,
                "--legal-moves" => opts.legal_moves = true,
                "--showcase" => opts.showcase = true,
                "--card-face" => opts.card_face = Some(PathBuf::from(value("a picture's file")?)),
                "--card-back" => opts.card_back = Some(PathBuf::from(value("a picture's file")?)),
                "--talk" => opts.talk = Some(value("a friend's handle")?),
                "--environment" => opts.environment = Some(value("an environment's name")?),
                "--view" => {
                    let v = value("YAW,PITCH,RADIUS[,X,Z]")?;
                    crate::camera::StartView::parse(&v).map_err(|e| format!("--view: {e}"))?;
                    opts.view = Some(v);
                }
                "--frames" => {
                    let v = value("a number of frames")?;
                    let n: u32 = v
                        .parse()
                        .map_err(|_| format!("--frames: {v:?} is not a number of frames"))?;
                    if n == 0 {
                        return Err("--frames must be at least 1".into());
                    }
                    opts.frames = Some(n);
                }
                "--screenshot" => opts.screenshot = Some(PathBuf::from(value("a file path")?)),
                "--server" => opts.server = Some(value("an address")?),
                "--watch" | "--watch-bots" | "--play-bot" | "--host" | "--join"
                    if opts.watch.is_some() =>
                {
                    return Err(
                        "only one of --watch, --watch-bots, --play-bot, --host and --join, once"
                            .into(),
                    );
                }
                "--watch" => opts.watch = Some(Watch::Room(value("a room code")?)),
                "--watch-bots" => opts.watch = Some(Watch::Bots(value("a game's id")?)),
                "--play-bot" => opts.watch = Some(Watch::PlayBots(value("a game's id")?)),
                "--host" => opts.watch = Some(Watch::Host(value("a game's id")?)),
                "--new-game" => opts.new_game = Some(value("a game's id")?),
                "--join" => opts.watch = Some(Watch::Join(value("a room code")?)),
                _ => return Err(format!("unknown option {arg:?}")),
            }
        }
        if opts.vr && opts.desktop {
            return Err("only one of --vr and --desktop".into());
        }
        if (opts.card_face.is_some() || opts.card_back.is_some()) && !opts.showcase {
            return Err("--card-face and --card-back are for --showcase".into());
        }
        Ok(opts)
    }

    /// Whether to look for a headset waiting, and start in it if there is one: not when told
    /// where to start, nor in a run that a script limits (`--frames`, `--screenshot`), which
    /// should do the same whatever else runs on the machine.
    #[cfg_attr(not(feature = "vr"), allow(dead_code))]
    pub fn looks_for_headset(&self) -> bool {
        !self.vr && !self.desktop && self.last_frame().is_none()
    }

    /// The frame to stop at, if the run is limited: `--frames`, or the default screenshot frame.
    pub fn last_frame(&self) -> Option<u32> {
        match (self.frames, &self.screenshot) {
            (Some(n), _) => Some(n),
            (None, Some(_)) => Some(DEFAULT_SCREENSHOT_FRAME),
            (None, None) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Options, String> {
        Options::parse(args.iter().copied())
    }

    #[test]
    fn no_arguments_runs_until_closed() {
        let opts = parse(&[]).unwrap();
        assert_eq!(opts, Options::default());
        assert_eq!(opts.last_frame(), None);
    }

    #[test]
    fn frames_in_both_spellings() {
        assert_eq!(parse(&["--frames", "120"]).unwrap().frames, Some(120));
        assert_eq!(parse(&["--frames=7"]).unwrap().frames, Some(7));
    }

    #[test]
    fn screenshot_alone_stops_at_the_default_frame() {
        let opts = parse(&["--screenshot", "out.png"]).unwrap();
        assert_eq!(opts.screenshot, Some(PathBuf::from("out.png")));
        assert_eq!(opts.last_frame(), Some(DEFAULT_SCREENSHOT_FRAME));
    }

    #[test]
    fn frames_decide_when_the_screenshot_is_taken() {
        let opts = parse(&["--screenshot=a.png", "--frames", "5"]).unwrap();
        assert_eq!(opts.last_frame(), Some(5));
    }

    #[test]
    fn bad_input_is_an_error() {
        assert!(parse(&["--frames"]).unwrap_err().contains("needs"));
        assert!(
            parse(&["--frames", "lots"])
                .unwrap_err()
                .contains("not a number")
        );
        assert!(
            parse(&["--frames", "0"])
                .unwrap_err()
                .contains("at least 1")
        );
        assert!(parse(&["--screenshot"]).unwrap_err().contains("needs"));
        assert!(
            parse(&["--fullscreen"])
                .unwrap_err()
                .contains("unknown option")
        );
    }

    #[test]
    fn a_server_for_this_run() {
        assert_eq!(
            parse(&["--server", "localhost:9000"])
                .unwrap()
                .server
                .as_deref(),
            Some("localhost:9000")
        );
        assert!(parse(&["--server"]).unwrap_err().contains("needs"));
    }

    #[test]
    fn one_thing_to_watch() {
        assert_eq!(
            parse(&["--watch", "AB12CD"]).unwrap().watch,
            Some(Watch::Room("AB12CD".into()))
        );
        assert_eq!(
            parse(&["--watch-bots=hex-7"]).unwrap().watch,
            Some(Watch::Bots("hex-7".into()))
        );
        assert!(
            parse(&["--watch", "A", "--watch-bots", "b"])
                .unwrap_err()
                .contains("only one")
        );
        assert_eq!(
            parse(&["--play-bot", "hex-7"]).unwrap().watch,
            Some(Watch::PlayBots("hex-7".into()))
        );
        assert!(
            parse(&["--play-bot", "a", "--watch", "B"])
                .unwrap_err()
                .contains("only one")
        );
        assert_eq!(
            parse(&["--host", "hex-7"]).unwrap().watch,
            Some(Watch::Host("hex-7".into()))
        );
        assert_eq!(
            parse(&["--join=AB12CD"]).unwrap().watch,
            Some(Watch::Join("AB12CD".into()))
        );
        assert!(
            parse(&["--join", "AB12CD", "--host", "hex-7"])
                .unwrap_err()
                .contains("only one")
        );
        assert_eq!(
            parse(&["--new-game", "hex-7"]).unwrap().new_game,
            Some("hex-7".into())
        );
        assert!(parse(&["--watch"]).unwrap_err().contains("needs"));
        assert!(parse(&["--sign-in"]).unwrap().sign_in);
        assert!(!parse(&[]).unwrap().sign_in);
        assert!(parse(&["--friends"]).unwrap().friends);
        assert_eq!(parse(&["--talk", "bob"]).unwrap().talk, Some("bob".into()));
        assert!(parse(&["--voice-check"]).unwrap().voice_check);
        assert!(!parse(&[]).unwrap().voice_check);
        assert_eq!(parse(&["--call=bob"]).unwrap().call, Some("bob".into()));
        assert!(parse(&["--no-audio-devices"]).unwrap().no_audio_devices);
        assert!(parse(&["--relay-only"]).unwrap().relay_only);
        assert!(parse(&["--voice"]).unwrap().voice);
    }

    #[test]
    fn where_the_table_stands_and_where_the_camera_starts() {
        let opts = parse(&["--environment", "garden", "--view=30,20,1.5"]).unwrap();
        assert_eq!(opts.environment.as_deref(), Some("garden"));
        assert_eq!(opts.view.as_deref(), Some("30,20,1.5"));
        assert!(parse(&["--view", "sideways"]).is_err());
        assert!(parse(&["--environment"]).is_err());
        assert!(parse(&["--vr"]).unwrap().vr);
        assert!(!parse(&[]).unwrap().vr);
        assert!(parse(&["--desktop"]).unwrap().desktop);
        assert!(parse(&["--no-bloom"]).unwrap().no_bloom);
        assert!(!parse(&[]).unwrap().no_bloom);
        assert!(parse(&["--legal-moves"]).unwrap().legal_moves);
        assert!(parse(&["--vr", "--desktop"]).is_err());
        // A headset is looked for unless told, or in a script's run.
        assert!(parse(&[]).unwrap().looks_for_headset());
        assert!(!parse(&["--vr"]).unwrap().looks_for_headset());
        assert!(!parse(&["--desktop"]).unwrap().looks_for_headset());
        assert!(!parse(&["--frames", "10"]).unwrap().looks_for_headset());
        assert!(
            !parse(&["--screenshot", "a.png"])
                .unwrap()
                .looks_for_headset()
        );
    }

    #[test]
    fn the_showcase_and_pictures_for_its_cards() {
        let opts = Options::parse(["--showcase", "--card-face", "seven.png"]).unwrap();
        assert!(opts.showcase);
        assert_eq!(opts.card_face, Some(PathBuf::from("seven.png")));
        assert_eq!(opts.card_back, None);
        assert!(!Options::parse(["--frames", "3"]).unwrap().showcase);
        assert!(
            Options::parse(["--card-back=b.jpg"]).is_err(),
            "a picture with nothing to put it on"
        );
        assert!(Options::parse(["--showcase", "--card-face"]).is_err());
    }

    #[test]
    fn help() {
        assert!(parse(&["-h"]).unwrap().help);
        assert!(parse(&["--help"]).unwrap().help);
    }
}
