//! The sounds playing a seat makes, for a game whose appearance gives them (`[sounds]`): a piece
//! picked up, a piece set down where it goes (or one placed), the move sent, and the game won
//! or lost. A game without
//! them is silent; nothing here names a game.
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
}

impl Cue {
    const ALL: [Cue; 5] = [Cue::Lift, Cue::Put, Cue::Play, Cue::Win, Cue::Lose];

    /// Its file, as the game's look gives it.
    pub fn file(self, sounds: &SoundLook) -> Option<&str> {
        match self {
            Cue::Lift => sounds.lift.as_deref(),
            Cue::Put => sounds.put.as_deref(),
            Cue::Play => sounds.play.as_deref(),
            Cue::Win => sounds.win.as_deref(),
            Cue::Lose => sounds.lose.as_deref(),
        }
    }
}

/// What of the turn makes sounds, at one moment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Moment {
    /// Where the piece in hand was picked up from, while one is held and not yet put anywhere.
    lifted: Option<String>,
    /// Where the move puts something new (a piece placed, an arrow shot).
    placed: Vec<String>,
    /// Whether the move has been sent.
    sent: bool,
}

impl Moment {
    pub fn of(turn: Option<&Turn>) -> Self {
        let Some(turn) = turn else {
            return Moment::default();
        };
        Moment {
            lifted: turn
                .carried()
                .into_iter()
                .find(|c| c.to.is_none())
                .map(|c| c.from.to_string()),
            placed: turn.placed().map(str::to_string).collect(),
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
    mut before: Local<(Moment, Standing)>,
    mut cues_out: MessageWriter<Cue>,
) {
    let now = (
        Moment::of(watch.turn.as_ref()),
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
    let Some(sounds) = sounds_of(&watch, &appearances) else {
        return;
    };
    for cue in Cue::ALL {
        if let Some(file) = cue.file(sounds)
            && !loaded.0.contains_key(file)
        {
            loaded
                .0
                .insert(file.to_string(), server.load(assets::path(file)));
        }
    }
}

fn play(
    mut commands: Commands,
    mut cues_in: MessageReader<Cue>,
    watch: Res<WatchStatus>,
    appearances: Res<Appearances>,
    loaded: Res<Loaded>,
) {
    let sounds = sounds_of(&watch, &appearances);
    for cue in cues_in.read() {
        let Some(handle) = sounds
            .and_then(|s| cue.file(s))
            .and_then(|file| loaded.0.get(file))
        else {
            continue;
        };
        commands.spawn((
            Name::new(format!("Sound of {cue:?}")),
            AudioPlayer::new(handle.clone()),
            PlaybackSettings::DESPAWN,
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
    fn each_cue_is_its_own_file_and_a_look_without_one_is_silent() {
        let sounds = SoundLook {
            lift: Some("a.ogg".into()),
            put: None,
            play: Some("c.ogg".into()),
            ..SoundLook::default()
        };
        assert_eq!(Cue::Lift.file(&sounds), Some("a.ogg"));
        assert_eq!(Cue::Put.file(&sounds), None);
        assert_eq!(Cue::Play.file(&sounds), Some("c.ogg"));
    }

    #[test]
    fn the_sounds_decode_as_the_client_plays_them_short_and_unclipped() {
        use bevy::audio::{Decodable, Source};

        let look = Appearances::built_in();
        let sounds = &look.for_game("amazons").expect("a look").sounds;
        for (cue, longest) in [
            (Cue::Lift, 0.2),
            (Cue::Put, 0.35),
            (Cue::Play, 0.7),
            (Cue::Win, 2.0),
            (Cue::Lose, 2.0),
        ] {
            let file = cue.file(sounds).expect("a sound for each");
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
