//! Time in a timed game: what the server's timestamps say, and time left in words.
//!
//! The server reports when the seat on impulse runs out of time (`impulse_deadline`) together
//! with its own time (`server_time`), so the time left is their difference, whatever this
//! machine's clock says; the client then counts it down itself until the next report.

use std::time::{Duration, Instant};

use crate::net::api::Clock;

/// A timed game's time, as the server last reported it.
#[derive(Debug, Clone, PartialEq)]
pub struct Timing {
    /// Every seat's time, in a game played with a clock.
    pub clock: Option<Clock>,
    /// What the seat on impulse had left when the server said so.
    pub impulse_left: Option<f64>,
    /// When the report arrived.
    pub received: Instant,
}

impl Timing {
    /// From a report; `None` when the game is not timed.
    pub fn new(clock: Option<Clock>, deadline: &str, server_time: &str) -> Option<Self> {
        let impulse_left = match (unix_seconds(deadline), unix_seconds(server_time)) {
            (Some(d), Some(now)) => Some(d - now),
            _ => None,
        };
        (clock.is_some() || impulse_left.is_some()).then(|| Timing {
            clock,
            impulse_left,
            received: Instant::now(),
        })
    }

    /// The time `seat` has left at `now`. `impulse_seat` is the seat whose time runs, while the
    /// game is being played. `None` for a seat whose time is not counted (any seat but the one
    /// on impulse, in a game timed by the move).
    pub fn left(&self, seat: u32, impulse_seat: Option<u32>, now: Instant) -> Option<f64> {
        let elapsed = now.saturating_duration_since(self.received).as_secs_f64();
        let running = |base: f64| Some((base - elapsed).max(0.0));
        match &self.clock {
            Some(clock) => {
                let own = clock.seats.iter().find(|s| s.seat == seat)?.seconds_left;
                if impulse_seat.is_some() && clock.running == Some(seat) {
                    running(self.impulse_left.unwrap_or(own))
                } else {
                    Some(own.max(0.0))
                }
            }
            None if impulse_seat == Some(seat) => running(self.impulse_left?),
            None => None,
        }
    }
}

/// Time left, as a clock shows it: "4:05", "1:02:09", or "2 d 3 h" for days.
pub fn words(seconds: f64) -> String {
    let s = Duration::from_secs_f64(seconds.max(0.0).ceil()).as_secs();
    let (d, h, m, s) = (s / 86_400, s / 3600 % 24, s / 60 % 60, s % 60);
    match (d, h) {
        (0, 0) => format!("{m}:{s:02}"),
        (0, h) => format!("{h}:{m:02}:{s:02}"),
        (d, h) => format!("{d} d {h} h"),
    }
}

/// An RFC 3339 time (`2026-11-19T23:18:50Z`, with fractions of a second or an offset) in
/// seconds since 1970; `None` for anything else, an empty string included.
pub fn unix_seconds(text: &str) -> Option<f64> {
    let b = text.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') {
        return None;
    }
    let num = |from: usize, len: usize| -> Option<i64> {
        let part = text.get(from..from + len)?;
        part.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    let (year, month, day) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let (hour, minute, second) = (num(11, 2)?, num(14, 2)?, num(17, 2)?);
    if b[13] != b':' || b[16] != b':' || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let mut rest = &text[19..];
    let mut fraction = 0.0;
    if let Some(after) = rest.strip_prefix('.') {
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        fraction = format!("0.{}", &after[..digits]).parse().ok()?;
        rest = &after[digits..];
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            if rest.len() != 6 || rest.as_bytes()[3] != b':' {
                return None;
            }
            sign * (num(text.len() - 5, 2)? * 3600 + num(text.len() - 2, 2)? * 60)
        }
    };
    let days = days_from_civil(year, month, day);
    let secs = days * 86_400 + hour * 3600 + minute * 60 + second - offset;
    Some(secs as f64 + fraction)
}

/// Days from 1970-01-01 to a date of the proleptic Gregorian calendar (Howard Hinnant's
/// algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::api::SeatClock;

    #[test]
    fn timestamps_read_as_the_server_writes_them() {
        assert_eq!(unix_seconds("1970-01-01T00:00:00Z"), Some(0.0));
        assert_eq!(unix_seconds("2000-03-01T00:00:00Z"), Some(951_868_800.0));
        assert_eq!(unix_seconds("2026-11-19T23:18:50Z"), Some(1_795_130_330.0));
        assert_eq!(
            unix_seconds("2026-11-19T23:18:50.25Z"),
            Some(1_795_130_330.25)
        );
        assert_eq!(
            unix_seconds("2026-11-20T00:18:50+01:00"),
            Some(1_795_130_330.0)
        );
        for bad in [
            "",
            "yesterday",
            "2026-11-19",
            "2026-13-19T23:18:50Z",
            "2026-11-19T23:18:50",
        ] {
            assert_eq!(unix_seconds(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn time_left_reads_like_a_clock() {
        assert_eq!(words(0.0), "0:00");
        assert_eq!(words(4.2), "0:05");
        assert_eq!(words(245.0), "4:05");
        assert_eq!(words(3729.0), "1:02:09");
        assert_eq!(words(2.0 * 86_400.0 + 3.0 * 3600.0), "2 d 3 h");
        assert_eq!(words(-3.0), "0:00");
    }

    fn clock(running: u32) -> Clock {
        Clock {
            seats: vec![
                SeatClock {
                    seat: 1,
                    initial_seconds: 300,
                    seconds_left: 302.8,
                },
                SeatClock {
                    seat: 2,
                    initial_seconds: 300,
                    seconds_left: 300.0,
                },
            ],
            running: Some(running),
            increment_seconds: 3,
        }
    }

    #[test]
    fn the_running_clock_counts_down_from_the_deadline_and_the_others_stand() {
        let t = Timing::new(
            Some(clock(2)),
            "2026-11-19T23:18:50Z",
            "2026-11-19T23:13:50Z",
        )
        .unwrap();
        let later = t.received + Duration::from_secs(10);
        assert_eq!(t.left(2, Some(2), later), Some(290.0));
        assert_eq!(t.left(1, Some(2), later), Some(302.8));
        // Once the game is over nothing runs.
        assert_eq!(t.left(2, None, later), Some(300.0));
        assert_eq!(t.left(3, Some(2), later), None);
    }

    #[test]
    fn a_game_timed_by_the_move_counts_only_the_seat_on_impulse() {
        let t = Timing::new(None, "2026-11-19T23:14:50Z", "2026-11-19T23:13:50Z").unwrap();
        let later = t.received + Duration::from_secs(75);
        assert_eq!(t.left(1, Some(1), later), Some(0.0));
        assert_eq!(t.left(2, Some(1), later), None);
        assert_eq!(Timing::new(None, "", ""), None, "an untimed game");
    }
}
