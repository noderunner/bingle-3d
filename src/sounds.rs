//! The sounds playing a seat makes, for a game whose appearance gives them (`[sounds]`): a piece
//! picked up, a piece set down where it goes (or one placed), and the move sent. A game without
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

/// Something the player did that may make a sound.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cue {
    /// A piece picked up.
    Lift,
    /// A piece set down where it goes, or one placed.
    Put,
    /// The move sent.
    Play,
}

impl Cue {
    /// Its file, as the game's look gives it.
    pub fn file(self, sounds: &SoundLook) -> Option<&str> {
        match self {
            Cue::Lift => sounds.lift.as_deref(),
            Cue::Put => sounds.put.as_deref(),
            Cue::Play => sounds.play.as_deref(),
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

fn listen(watch: Res<WatchStatus>, mut before: Local<Moment>, mut cues_out: MessageWriter<Cue>) {
    let now = Moment::of(watch.turn.as_ref());
    for cue in cues(&before, &now) {
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
    for cue in [Cue::Lift, Cue::Put, Cue::Play] {
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
        for (cue, longest) in [(Cue::Lift, 0.2), (Cue::Put, 0.35), (Cue::Play, 0.7)] {
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
}
