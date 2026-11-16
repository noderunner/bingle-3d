//! Command-line options.

use std::path::PathBuf;

pub const USAGE: &str = "\
Usage: bingle-3d [options]

Options:
  --server URL       the Bingle server for this run, in place of the configured one
                     (localhost:8080, https://games.example.com)
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
    pub help: bool,
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
                _ => return Err(format!("unknown option {arg:?}")),
            }
        }
        Ok(opts)
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
    fn help() {
        assert!(parse(&["-h"]).unwrap().help);
        assert!(parse(&["--help"]).unwrap().help);
    }
}
