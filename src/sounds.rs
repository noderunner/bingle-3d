//! The sounds playing a seat makes: a piece picked up, a piece set down where it goes (or one
//! placed), the move sent, and the game won or lost. Every game makes the same by default
//! ([`Cue::default_file`]); a game's appearance may give any of them another (`[sounds]`).
//! Nothing here names a game.
//!
//! Most cues are read from the turn as it changes ([`cues`], with no Bevy in it); a piece
//! carried to where it goes sounds when it touches the board (`hand.rs` says so). With
//! `--no-audio-devices` the plugin plays nothing, and Bevy's own audio is not added, so no
//! speaker is opened.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::appearance::{Appearances, SoundLook};
use crate::assets;
use crate::net::WatchStatus;
use crate::play::{Stage, Turn};
use crate::table::Table;

/// Something the player did that may make a sound.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cue {
    /// A piece picked up.
    Lift,
    /// A piece set down where it goes, or one placed.
    Put,
    /// The move sent.
    Play,
    /// The game over, this client's seat having won.
    Win,
    /// The game over, this client's seat having lost.
    Lose,
    /// A piece captured, set down where its captor keeps what it took (a tray beside the
    /// board): several may follow one another, so it is softer still.
    Capture,
}

impl Cue {
    const ALL: [Cue; 6] = [
        Cue::Lift,
        Cue::Put,
        Cue::Play,
        Cue::Win,
        Cue::Lose,
        Cue::Capture,
    ];

    /// The sound every game makes for it, unless its look gives another: synthesised by
    /// `assets-src/sounds/sounds.py`.
    pub fn default_file(self) -> &'static str {
        match self {
            Cue::Lift => "sounds/tick.ogg",
            Cue::Put => "sounds/thump.ogg",
            Cue::Play => "sounds/key.ogg",
            Cue::Win => "sounds/cheer.ogg",
            Cue::Lose => "sounds/groan.ogg",
            Cue::Capture => "sounds/tock.ogg",
        }
    }

    /// Its file: the one the game's look gives, or the default.
    pub fn file(self, look: Option<&SoundLook>) -> &str {
        look.and_then(|s| self.given(s))
            .unwrap_or(self.default_file())
    }

    /// Its file, if the game's look gives one.
    fn given(self, sounds: &SoundLook) -> Option<&str> {
        match self {
            Cue::Lift => sounds.lift.as_deref(),
            Cue::Put => sounds.put.as_deref(),
            Cue::Play => sounds.play.as_deref(),
            Cue::Win => sounds.win.as_deref(),
            Cue::Lose => sounds.lose.as_deref(),
            Cue::Capture => sounds.capture.as_deref(),
        }
    }
}

/// What of the turn makes sounds, at one moment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Moment {
    /// What is in hand and not yet put anywhere: where a piece was picked up from, or the kind
    /// of piece taken (from a bowl) to place.
    lifted: Option<String>,
    /// Where the move puts something new (a piece placed, an arrow shot).
    placed: Vec<String>,
    /// Whether the move has been sent.
    sent: bool,
}

impl Moment {
    /// The turn's moment. With `bowls`, a piece placed is carried from its bowl and sounds as it
    /// lands (`hand.rs`), so placing it is silent here.
    pub fn of(turn: Option<&Turn>, bowls: bool) -> Self {
        let Some(turn) = turn else {
            return Moment::default();
        };
        let placements: Vec<&str> = turn.placements().map(|(cell, _)| cell).collect();
        Moment {
            lifted: turn
                .carried()
                .into_iter()
                .find(|c| c.to.is_none())
                .map(|c| c.from.to_string())
                .or_else(|| turn.in_hand().map(|piece| format!("piece {piece}")))
                // Something taken up to put down (a fence from its bowl).
                .or_else(|| turn.taken().map(|_| "taken".to_string()))
                // A stack picked up, and each time the wheel changes how much of it.
                .or_else(|| {
                    let m = turn.stack_moves().pop()?;
                    (turn.stage == Stage::Choosing && m.drops.is_empty())
                        .then(|| format!("stack {} {}", m.from, m.count))
                }),
            placed: turn
                .placed()
                .filter(|cell| !bowls || !placements.contains(cell))
                .map(str::to_string)
                .collect(),
            sent: turn.stage == Stage::Sending,
        }
    }
}

/// How a game stands, for the sound of its end: which room, and how it ended for this client's
/// seat, once it has.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Standing {
    room: String,
    finished: bool,
    /// The seat's sound: won or lost. `None` for a draw, a game it has no seat in, or one going
    /// on.
    result: Option<Cue>,
}

impl Standing {
    pub fn of(table: Option<&Table>) -> Self {
        let Some(t) = table else {
            return Standing::default();
        };
        let s = &t.state;
        Standing {
            room: t.room_code.clone(),
            finished: t.finished(),
            result: result(
                t.yours().map(|y| y.seat),
                t.finished(),
                s.winner_seat,
                s.all_lost,
            ),
        }
    }
}

/// How a game ended for the seat `mine`, as a sound: won, lost (to another seat, or with every
/// seat), or nothing (a draw, no seat, not over).
pub fn result(
    mine: Option<u32>,
    finished: bool,
    winner: Option<u32>,
    all_lost: bool,
) -> Option<Cue> {
    let mine = mine.filter(|_| finished)?;
    match winner {
        Some(w) if w == mine => Some(Cue::Win),
        Some(_) => Some(Cue::Lose),
        None if all_lost => Some(Cue::Lose),
        None => None,
    }
}

/// The sound of a game's end: only as the game being watched ends, not when one already over is
/// opened.
pub fn ended(before: &Standing, now: &Standing) -> Option<Cue> {
    let ending = now.finished && !before.finished && now.room == before.room;
    now.result.filter(|_| ending)
}

/// The sounds a change of the turn makes: a piece newly in hand, something newly placed, the move
/// newly sent. A piece moved sounds when it lands (`hand.rs`), not here.
pub fn cues(before: &Moment, now: &Moment) -> Vec<Cue> {
    let mut out = Vec::new();
    if now.lifted.is_some() && now.lifted != before.lifted {
        out.push(Cue::Lift);
    }
    if now.placed.iter().any(|p| !before.placed.contains(p)) {
        out.push(Cue::Put);
    }
    if now.sent && !before.sent {
        out.push(Cue::Play);
    }
    out
}

pub struct SoundsPlugin {
    /// Whether sounds are played (not with `--no-audio-devices`).
    pub on: bool,
}

impl Plugin for SoundsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Cue>()
            .add_systems(Update, listen.run_if(resource_changed::<WatchStatus>));
        if self.on {
            app.init_resource::<Loaded>()
                .add_systems(Update, (load, play).chain().after(listen));
        }
    }
}

/// The sounds of the game being watched, loaded before they are first wanted.
#[derive(Resource, Default)]
struct Loaded(HashMap<String, Handle<AudioSource>>);

fn listen(
    watch: Res<WatchStatus>,
    appearances: Res<Appearances>,
    mut before: Local<(Moment, Standing)>,
    mut cues_out: MessageWriter<Cue>,
) {
    // A piece placed from a bowl or a seat's supply is carried there, and sounds as it lands.
    let bowls = watch.table.as_ref().is_some_and(|t| {
        appearances
            .for_game(&t.info.id)
            .is_some_and(|look| look.bowls.is_some())
            || !crate::supply::pools(&t.info).is_empty()
    });
    let now = (
        Moment::of(watch.turn.as_ref(), bowls),
        Standing::of(watch.table.as_ref()),
    );
    for cue in cues(&before.0, &now.0) {
        cues_out.write(cue);
    }
    if let Some(cue) = ended(&before.1, &now.1) {
        cues_out.write(cue);
    }
    *before = now;
}

/// The game's sounds the look gives.
fn sounds_of<'a>(watch: &WatchStatus, appearances: &'a Appearances) -> Option<&'a SoundLook> {
    let table = watch.table.as_ref()?;
    Some(&appearances.for_game(&table.info.id)?.sounds)
}

fn load(
    watch: Res<WatchStatus>,
    appearances: Res<Appearances>,
    server: Res<AssetServer>,
    mut loaded: ResMut<Loaded>,
) {
    if watch.table.is_none() {
        return;
    }
    let sounds = sounds_of(&watch, &appearances);
    for cue in Cue::ALL {
        let file = cue.file(sounds);
        if !loaded.0.contains_key(file) {
            loaded
                .0
                .insert(file.to_string(), server.load(assets::path(file)));
        }
    }
}

/// How fast the `n`th sound of `cue` is played: a capture a little higher or lower each time,
/// so several in a row are not one sound repeated; the others as they are.
pub fn speed(cue: Cue, n: u32) -> f32 {
    const CAPTURES: [f32; 5] = [1.0, 1.05, 0.96, 1.03, 0.98];
    match cue {
        Cue::Capture => CAPTURES[n as usize % CAPTURES.len()],
        _ => 1.0,
    }
}

fn play(
    mut commands: Commands,
    mut cues_in: MessageReader<Cue>,
    watch: Res<WatchStatus>,
    appearances: Res<Appearances>,
    loaded: Res<Loaded>,
    mut played: Local<u32>,
) {
    let sounds = sounds_of(&watch, &appearances);
    for cue in cues_in.read() {
        let Some(handle) = loaded.0.get(cue.file(sounds)) else {
            continue;
        };
        *played = played.wrapping_add(1);
        commands.spawn((
            Name::new(format!("Sound of {cue:?}")),
            AudioPlayer::new(handle.clone()),
            PlaybackSettings::DESPAWN.with_speed(speed(*cue, *played)),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moment(lifted: Option<&str>, placed: &[&str], sent: bool) -> Moment {
        Moment {
            lifted: lifted.map(Into::into),
            placed: placed.iter().map(|p| p.to_string()).collect(),
            sent,
        }
    }

    #[test]
    fn a_stack_picked_up_ticks_and_ticks_again_as_the_wheel_changes_how_much() {
        use crate::net::api::{Gesture, LegalSelection, Selections};
        let option = |count: &str| LegalSelection {
            notation: "m".into(),
            action: "move".into(),
            to: Some("b3".into()),
            from: Some("b2".into()),
            piece: None,
            aim: None,
            gestures: [("pos", "b2"), ("count", count), ("pos", "b3")]
                .iter()
                .chain(if count == "2" {
                    &[("pos", "b3")][..]
                } else {
                    &[]
                })
                .map(|(k, v)| Gesture {
                    kind: k.to_string(),
                    value: v.to_string(),
                })
                .collect(),
        };
        let mut turn = Turn::new(1);
        turn.answered(Selections {
            seat: 1,
            ply: 4,
            made: vec![],
            options: vec![option("1"), option("2")],
            complete: false,
            may_end: false,
        });
        let before = Moment::of(Some(&turn), true);
        turn.gesture(&Gesture::pos("b2"));
        let lifted = Moment::of(Some(&turn), true);
        assert_eq!(cues(&before, &lifted), [Cue::Lift]);
        turn.wheel(false, &Default::default());
        let fewer = Moment::of(Some(&turn), true);
        assert_eq!(cues(&lifted, &fewer), [Cue::Lift]);
        assert!(cues(&fewer, &fewer).is_empty());
    }

    #[test]
    fn a_piece_picked_up_placed_and_a_move_sent_each_sound_once() {
        let nothing = Moment::default();
        let held = moment(Some("d1"), &[], false);
        assert_eq!(cues(&nothing, &held), [Cue::Lift]);
        assert!(cues(&held, &held).is_empty(), "held still: no sound again");
        assert_eq!(
            cues(&held, &moment(Some("g1"), &[], false)),
            [Cue::Lift],
            "another piece picked up instead"
        );
        // Put down where it goes: it sounds when it lands, not here.
        let moved = moment(None, &[], false);
        assert!(cues(&held, &moved).is_empty());
        // An arrow shot, then another instead.
        let shot = moment(None, &["f5"], false);
        assert_eq!(cues(&moved, &shot), [Cue::Put]);
        assert_eq!(cues(&shot, &moment(None, &["d1"], false)), [Cue::Put]);
        // Sent: the move's sound, and nothing more as it waits.
        let sent = moment(None, &["f5"], true);
        assert_eq!(cues(&shot, &sent), [Cue::Play]);
        assert!(cues(&sent, &sent).is_empty());
        // The turn over, and taking back: no sounds.
        assert!(cues(&sent, &nothing).is_empty());
        assert!(cues(&shot, &moved).is_empty());
    }

    #[test]
    fn every_game_makes_the_default_sounds_unless_its_look_gives_others() {
        let sounds = SoundLook {
            lift: Some("a.ogg".into()),
            play: Some("c.ogg".into()),
            ..SoundLook::default()
        };
        assert_eq!(Cue::Lift.file(Some(&sounds)), "a.ogg");
        assert_eq!(Cue::Put.file(Some(&sounds)), Cue::Put.default_file());
        assert_eq!(Cue::Play.file(Some(&sounds)), "c.ogg");
        // A game with no look of its own, and one whose look says nothing of sounds.
        for look in [None, Some(&SoundLook::default())] {
            for cue in Cue::ALL {
                assert_eq!(cue.file(look), cue.default_file());
            }
        }
        // Each cue its own sound, and every default one the client has.
        let files: std::collections::HashSet<_> =
            Cue::ALL.iter().map(|c| c.default_file()).collect();
        assert_eq!(files.len(), Cue::ALL.len());
        assert!(files.iter().all(|f| assets::exists(f)));
    }

    #[test]
    fn captures_in_a_row_each_sound_a_little_different_and_nothing_else_does() {
        let five: Vec<f32> = (0..5).map(|n| speed(Cue::Capture, n)).collect();
        for (i, a) in five.iter().enumerate() {
            assert!((0.9..=1.1).contains(a), "only a little: {a}");
            assert!(five[i + 1..].iter().all(|b| b != a), "{five:?}");
        }
        for cue in [Cue::Lift, Cue::Put, Cue::Play, Cue::Win, Cue::Lose] {
            assert_eq!(speed(cue, 3), 1.0);
        }
    }

    #[test]
    fn the_sounds_decode_as_the_client_plays_them_short_and_unclipped() {
        use bevy::audio::{Decodable, Source};

        for (cue, longest) in [
            (Cue::Lift, 0.2),
            (Cue::Put, 0.35),
            (Cue::Play, 0.7),
            (Cue::Win, 2.0),
            (Cue::Lose, 2.0),
            (Cue::Capture, 0.2),
        ] {
            let file = cue.default_file();
            let source = AudioSource {
                bytes: assets::FILES
                    .iter()
                    .find(|(f, _)| *f == file)
                    .map(|(_, bytes)| bytes.to_vec())
                    .expect("compiled in")
                    .into(),
            };
            let decoder = source.decoder();
            let rate = decoder.sample_rate().get() as f32;
            let samples: Vec<f32> = decoder.collect();
            let secs = samples.len() as f32 / rate;
            assert!(secs > 0.05 && secs < longest, "{file}: {secs} s");
            let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(peak > 0.1 && peak < 0.95, "{file}: peak {peak}");
        }
    }

    fn standing(room: &str, finished: bool, result: Option<Cue>) -> Standing {
        Standing {
            room: room.into(),
            finished,
            result,
        }
    }

    #[test]
    fn a_game_won_or_lost_sounds_as_it_ends_and_not_when_opened_over() {
        let playing = standing("AB12", false, None);
        let won = standing("AB12", true, Some(Cue::Win));
        let lost = standing("AB12", true, Some(Cue::Lose));
        assert_eq!(ended(&playing, &won), Some(Cue::Win));
        assert_eq!(ended(&playing, &lost), Some(Cue::Lose));
        assert_eq!(ended(&won, &won), None, "once");
        assert_eq!(
            ended(&playing, &standing("AB12", true, None)),
            None,
            "a draw"
        );
        // A game opened when it is over already, or another table's end: no sound.
        assert_eq!(ended(&Standing::default(), &won), None);
        assert_eq!(ended(&standing("ZZ99", false, None), &won), None);
    }

    #[test]
    fn how_a_game_ended_is_read_for_this_clients_seat() {
        assert_eq!(result(Some(1), false, None, false), None, "not over");
        assert_eq!(result(Some(1), true, Some(1), false), Some(Cue::Win));
        assert_eq!(result(Some(1), true, Some(2), false), Some(Cue::Lose));
        assert_eq!(
            result(Some(1), true, None, true),
            Some(Cue::Lose),
            "everybody lost"
        );
        assert_eq!(result(Some(1), true, None, false), None, "a draw");
        assert_eq!(
            result(None, true, Some(1), false),
            None,
            "watching others play"
        );
    }
}
