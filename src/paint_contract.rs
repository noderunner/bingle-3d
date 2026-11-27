//! The paint contract (Bingle's ADR 0154, decision 4), held by a test for every look this client
//! ships. Within one game, whatever look it is drawn in:
//!
//! 1. the paints of all the colors in play can be told apart, and
//! 2. the order of lightness is kept: where the game plays `white` its paint is the lightest of
//!    the game's colors, and where it plays `black`, the darkest.
//!
//! A color is a name and paint is this client's own, so this holds a look to what it must keep,
//! not to what a name says. The colors of each game are in `GAMES`, a short table copied from
//! the definitions Bingle ships (`internal/games/**/*.yaml`, `seats` and `extra_colors`): the
//! client has no other source for them offline, and a test below fails for a game id an appearance
//! file names that the table does not have.
//!
//! Four paints of a color are held to it, each on its own among the colors of the game:
//!
//! - `paint`, what `appearance::seat_paint` gives (the panels' dot, the cells along a goal),
//!   which is also held near what its pieces look like (`MAX_FROM_PIECES`), so that a dot beside
//!   a seat's name stands for its pieces;
//! - `tint`, the `tint` of the material the seat's pieces are made of, where it has one;
//! - `pieces`, what the pieces look like: the material's texture (a wood's, or a stone's) at its
//!   average, in linear light, times its tint. Where a look gives a seat no material the pieces
//!   are the standard look's paint, as `board::Tools::piece_material` draws them, which is not
//!   always the look's own `paint` (every seat a shipped look paints names its pieces, so today
//!   they agree);
//! - `goal`, the edge of the board a seat connects to, where a look gives one.
//!
//! The pieces and the goal are worked out for each wood the player may choose for the board
//! (`look::BOARD_WOODS`), since a material of the board's own wood, or one that stands out
//! against it, changes with it.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, Image, ImageSampler, ImageType};
use bevy::prelude::*;

use crate::appearance::{self, Appearance, Appearances};
use crate::assets;
use crate::look::{self, BOARD_WOODS, MaterialSpec};

/// The least difference two paints of one game may have, in OKLab (the distance between two
/// colors there, in the units of its L, a and b, where L runs from 0 to 1). About 0.02 is the
/// least difference a person sees at all, so this is four times that: a pair clearly apart, on a
/// glossy piece, in shade and across a table. Today's closest pairs are Blooms' red jasper and
/// yellow tiger-eye stones by their textures' average (0.092), then Draughts' tints (0.199), and
/// every other is over 0.2. Not a tighter threshold, because the average of a veined stone is
/// only a stand-in for what a person sees of it.
const MIN_DELTA: f32 = 0.08;

/// How much lighter than every other color of its game `white` must be, and darker `black`
/// less: in CIE L* (0 to 100). The rule is an order, so any margin would do; this one is a few
/// steps a person tells apart, so a pair that merely edges ahead does not count.
const MIN_LIGHTER: f32 = 4.0;

/// The most a seat's `paint` (its dot beside its name, issue #91) may differ from what its pieces
/// look like, on any board wood, in OKLab. The dot is flat and a stone is veined and polished, so
/// it is not held to the average closely: this is about what tells a pale dot from dark pieces,
/// or one hue from another. Today the farthest is Draughts' black (0.082, both near black).
const MAX_FROM_PIECES: f32 = 0.1;

/// Where a look's `paint` for a seat is farther from what its pieces look like than
/// `MAX_FROM_PIECES`, as lines saying where.
fn misleading_dots(look: &Appearance) -> Vec<String> {
    let mut out = Vec::new();
    for (color, seat) in &look.seats {
        let (Some(paint), Some(material)) = (seat.paint, made_of(look, color, false)) else {
            continue;
        };
        for wood in BOARD_WOODS {
            let pieces = looks_like(material, wood);
            let d = delta(paint, pieces);
            if d > MAX_FROM_PIECES {
                out.push(format!(
                    "{}, {color}: paint {paint:?} is {d:.3} from its pieces on {wood} \
                     ({pieces:.2?}), over {MAX_FROM_PIECES}",
                    look.games.join(" "),
                ));
            }
        }
    }
    out
}

/// What a game plays: each color, with the seat it belongs to (an unnamed color is painted by
/// seat, so it matters; a color no seat plays has the next number). The first column is the game
/// ids it holds for.
type Colors = &'static [(u32, &'static str)];

const GAMES: &[(&[&str], Colors)] = &[
    (&["amazons", "amazons-6"], &[(1, "white"), (2, "black")]),
    (
        &["blooms", "blooms-4", "blooms-6", "blooms-8", "blooms-10"],
        &[(1, "red"), (1, "yellow"), (2, "blue"), (2, "black")],
    ),
    (&["bowling-solitaire", "solitaire"], &[(1, "red")]),
    (&["ataxx"], &[(1, "red"), (2, "blue")]),
    (&["breakthrough"], &[(1, "white"), (2, "black")]),
    (&["brandubh"], &[(1, "black"), (2, "white")]),
    (
        &[
            "chess",
            "chess-960",
            "chess-crazyhouse",
            "chess-antichess",
            "chess-atomic",
            "chess-alice",
        ],
        &[(1, "white"), (2, "black")],
    ),
    (
        &["courtfall-1.0.0", "courtfall-1.0.0~arena"],
        &[(1, "white"), (2, "black")],
    ),
    (&["draughts"], &[(1, "black"), (2, "red")]),
    (
        &["fendo", "fendo-9", "fendo-11"],
        &[(1, "white"), (2, "orange")],
    ),
    // Green is the pawn's, which no seat plays (`extra_colors`).
    (&["fist"], &[(1, "white"), (2, "black"), (3, "green")]),
    (
        &["hex-7", "hex-9", "hex-11", "hex-13"],
        &[(1, "red"), (2, "blue")],
    ),
    (&["othello", "reversi"], &[(1, "black"), (2, "white")]),
    (
        &["go-9x9", "go-13x13", "go-19x19", "gomoku", "pente"],
        &[(1, "black"), (2, "white")],
    ),
    (
        &["meridians", "meridians-5", "meridians-7"],
        &[(1, "white"), (2, "black")],
    ),
    (
        &[
            "havannah-4",
            "havannah-6",
            "havannah-8",
            "havannah-10",
            "yavalath",
        ],
        &[(1, "white"), (2, "black")],
    ),
    (
        &["mycelium", "mycelium-1.0.0"],
        &[(1, "black"), (2, "green")],
    ),
    (
        &[
            "tak-3x3", "tak-4x4", "tak-5x5", "tak-6x6", "tak-7x7", "tak-8x8",
        ],
        &[(1, "white"), (2, "black")],
    ),
];

/// A paint, in sRGB.
type Paint = [f32; 3];

fn colors_of(game: &str) -> Option<Colors> {
    GAMES
        .iter()
        .find(|(ids, _)| ids.contains(&game))
        .map(|(_, colors)| *colors)
}

fn to_linear(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// OKLab (Björn Ottosson's), whose distances follow what a person sees.
fn oklab(paint: Paint) -> [f32; 3] {
    let [r, g, b] = paint.map(to_linear);
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

fn delta(a: Paint, b: Paint) -> f32 {
    let (a, b) = (oklab(a), oklab(b));
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// CIE L*, 0 (black) to 100 (white).
fn lightness(paint: Paint) -> f32 {
    let [r, g, b] = paint.map(to_linear);
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let f = if y > 216.0 / 24389.0 {
        y.cbrt()
    } else {
        (24389.0 / 27.0 * y + 16.0) / 116.0
    };
    116.0 * f - 16.0
}

/// What is wrong with these paints as one game's: a pair too close, `white` not the lightest,
/// `black` not the darkest. Each is a line saying which, with the numbers.
fn breaches(paints: &[(String, Paint)]) -> Vec<String> {
    let mut out = Vec::new();
    for (i, (a, pa)) in paints.iter().enumerate() {
        for (b, pb) in &paints[i + 1..] {
            let d = delta(*pa, *pb);
            if d < MIN_DELTA {
                out.push(format!(
                    "{a} {pa:?} and {b} {pb:?} differ by {d:.3}, under {MIN_DELTA}"
                ));
            }
        }
    }
    for (word, lightest) in [("white", true), ("black", false)] {
        let Some((_, mine)) = paints.iter().find(|(c, _)| c == word) else {
            continue;
        };
        let mine = lightness(*mine);
        for (other, paint) in paints.iter().filter(|(c, _)| c != word) {
            let theirs = lightness(*paint);
            let ahead = if lightest {
                mine - theirs
            } else {
                theirs - mine
            };
            if ahead < MIN_LIGHTER {
                out.push(format!(
                    "{word} (L* {mine:.1}) is not {} than {other} (L* {theirs:.1})",
                    if lightest { "lighter" } else { "darker" },
                ));
            }
        }
    }
    out
}

/// The average of a color texture of ours, in linear light, as the GPU's mipmaps would have it
/// (every few texels: it is a grain, and the test has to be quick).
fn mean_albedo(file: &str) -> [f32; 3] {
    static CACHE: OnceLock<Mutex<HashMap<String, [f32; 3]>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(found) = cache.lock().unwrap().get(file) {
        return *found;
    }
    let bytes = assets::FILES
        .iter()
        .find(|(f, _)| *f == file)
        .unwrap_or_else(|| panic!("no texture {file}"))
        .1;
    let image = Image::from_buffer(
        bytes,
        ImageType::Extension("jpg"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::default(),
        RenderAssetUsages::default(),
    )
    .unwrap_or_else(|e| panic!("{file}: {e}"));
    let data = image.data.as_ref().expect("pixels");
    assert_eq!(
        data.len() as u32,
        image.width() * image.height() * 4,
        "{file}"
    );
    let (mut sum, mut n) = ([0.0f64; 3], 0.0f64);
    for texel in data.as_chunks::<4>().0.iter().step_by(7) {
        for (s, c) in sum.iter_mut().zip(texel) {
            *s += f64::from(to_linear(f32::from(*c) / 255.0));
        }
        n += 1.0;
    }
    let mean = sum.map(|s| (s / n) as f32);
    cache.lock().unwrap().insert(file.to_string(), mean);
    mean
}

/// What a material looks like, on a board of this wood: its texture (the wood's, or a file's)
/// at its average, times what `MaterialSpec::build` multiplies it by. One with no texture is
/// its tint alone.
fn looks_like(spec: &MaterialSpec, board_wood: &str) -> Paint {
    let spec = spec.on_board(board_wood);
    let tint = spec.tint.unwrap_or([1.0; 3]);
    let (base, texture) = match (spec.wood.as_deref().and_then(look::wood), &spec.albedo) {
        (Some(w), _) => (
            [0, 1, 2].map(|i| tint[i] * w.finish[i]),
            Some(format!("textures/wood/{}_albedo.jpg", w.name)),
        ),
        (None, albedo) => (tint, albedo.clone()),
    };
    let texture = texture.map_or([1.0; 3], |file| mean_albedo(&file));
    [0, 1, 2].map(|i| to_srgb(to_linear(base[i]) * texture[i]))
}

/// The material a seat's pieces, or its goal's edge, are made of in a look, if it gives one.
fn made_of<'a>(look: &'a Appearance, color: &str, goal: bool) -> Option<&'a MaterialSpec> {
    let seat = look.seats.get(color)?;
    look.material(if goal {
        seat.goal.as_ref()
    } else {
        seat.pieces.as_ref()
    })
}

fn clamped(c: Color) -> Paint {
    let c = c.to_srgba();
    [c.red, c.green, c.blue]
}

/// Every set of paints a game's colors have in a look (or in the standard look, for `None`),
/// named by what they are and, for what depends on it, the board's wood.
fn paints_of(look: Option<&Appearance>, colors: Colors) -> Vec<(String, Vec<(String, Paint)>)> {
    let named = |f: &dyn Fn(&str, u32) -> Paint| -> Vec<(String, Paint)> {
        colors
            .iter()
            .map(|(seat, color)| (color.to_string(), f(color, *seat)))
            .collect()
    };
    let paint = |color: &str, seat| clamped(appearance::seat_paint(look, color, seat));
    let mut sets = vec![("paint".to_string(), named(&paint))];
    let Some(look) = look else {
        return sets;
    };
    sets.push((
        "tint".into(),
        named(&|color, seat| {
            made_of(look, color, false)
                .and_then(|m| m.tint)
                .unwrap_or_else(|| paint(color, seat))
        }),
    ));
    for wood in BOARD_WOODS {
        sets.push((
            format!("pieces, on a board of {wood}"),
            named(&|color, seat| match made_of(look, color, false) {
                Some(material) => looks_like(material, wood),
                // What `board::Tools::piece_material` draws.
                None => clamped(look::seat_paint(color, seat)),
            }),
        ));
        let goals: Vec<(String, Paint)> = colors
            .iter()
            .filter_map(|(_, color)| {
                Some((
                    color.to_string(),
                    looks_like(made_of(look, color, true)?, wood),
                ))
            })
            .collect();
        if goals.len() > 1 {
            sets.push((format!("goal, on a board of {wood}"), goals));
        }
    }
    sets
}

/// Every breach of the contract in one game's look, as lines saying where.
fn breaches_of(game: &str, look: Option<&Appearance>) -> Vec<String> {
    let colors = colors_of(game).unwrap_or_else(|| panic!("add {game} to GAMES"));
    let mut out = Vec::new();
    for (name, paints) in paints_of(look, colors) {
        out.extend(
            breaches(&paints)
                .into_iter()
                .map(|b| format!("{game}, {name}: {b}")),
        );
    }
    out
}

/// A look that fails nothing, or what it fails, for every game its file covers.
fn check(look: &Appearance) -> Vec<String> {
    look.games
        .iter()
        .flat_map(|g| breaches_of(g, Some(look)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pairs that fail today and are left as they are for the maintainer to decide.
    const ALLOWED: &[&str] = &[];

    fn shipped() -> Vec<Appearance> {
        Appearances::built_in().0
    }

    fn unallowed(breaches: Vec<String>) -> Vec<String> {
        breaches
            .into_iter()
            .filter(|b| !ALLOWED.iter().any(|a| b.contains(a)))
            .collect()
    }

    #[test]
    fn every_game_a_look_covers_has_its_colors_here() {
        for look in shipped() {
            for game in &look.games {
                assert!(colors_of(game).is_some(), "add {game} to GAMES");
                // A look paints only colors that a game it covers has (a file for several
                // families of games names the colors of them all).
                for word in look.seats.keys() {
                    assert!(
                        look.games
                            .iter()
                            .any(|g| colors_of(g)
                                .is_some_and(|cs| cs.iter().any(|(_, c)| c == word))),
                        "no game of the look has color {word}, which it paints"
                    );
                }
            }
        }
    }

    #[test]
    fn every_shipped_look_keeps_the_contract() {
        let bad = unallowed(shipped().iter().flat_map(check).collect());
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }

    #[test]
    fn every_shipped_looks_dot_is_painted_as_its_pieces_look() {
        let bad: Vec<String> = shipped().iter().flat_map(misleading_dots).collect();
        assert!(bad.is_empty(), "{}", bad.join("\n"));
        // A dot much paler than the pieces it stands for.
        let mut pale = Appearances::built_in().for_game("chess").unwrap().clone();
        pale.seats.get_mut("black").unwrap().paint = Some([0.55, 0.5, 0.45]);
        assert!(!misleading_dots(&pale).is_empty());
    }

    #[test]
    fn a_game_with_no_look_keeps_it_in_the_standard_look() {
        let mut none = 0;
        for (ids, _) in GAMES {
            for game in *ids {
                if Appearances::built_in().for_game(game).is_none() {
                    none += 1;
                    let bad = unallowed(breaches_of(game, None));
                    assert!(bad.is_empty(), "{}", bad.join("\n"));
                }
            }
        }
        assert!(none > 0);
    }

    /// The standard look's eight palette paints and its color words (the palette's six among
    /// them, `gold`, `violet`, `teal`, `gray` and `silver` being other names for paints it
    /// has), all together, as if one game played every one.
    #[test]
    fn the_standard_palette_and_its_words_keep_the_contract() {
        let words = [
            "red", "blue", "yellow", "green", "purple", "orange", "cyan", "pink", "white", "black",
            "grey", "brown",
        ];
        let mut paints: Vec<(String, Paint)> = words
            .iter()
            .map(|w| (w.to_string(), clamped(look::seat_paint(w, 1))))
            .collect();
        // Unnamed colors, by seat: the palette goes round after eight.
        paints.extend((1..=8).map(|seat| {
            (
                format!("unnamed seat {seat}"),
                clamped(look::seat_paint("no-such-color", seat)),
            )
        }));
        // An unnamed color's seat paints it as a word's paint does (seat 1 red), which is two
        // names for one paint and is no breach: the seats of one game never share a name.
        let mut distinct = paints.clone();
        distinct.retain(|(n, p)| {
            !n.starts_with("unnamed")
                || !words.iter().any(|w| clamped(look::seat_paint(w, 1)) == *p)
        });
        let bad = unallowed(breaches(&distinct));
        assert!(bad.is_empty(), "{}", bad.join("\n"));
        // And the eight palette paints among themselves, whatever the seats.
        let palette: Vec<(String, Paint)> = (1..=8)
            .map(|s| (format!("seat {s}"), clamped(look::seat_paint("x", s))))
            .collect();
        let bad = unallowed(breaches(&palette));
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }

    #[test]
    fn it_fails_when_two_paints_of_one_game_are_made_alike() {
        let chess = Appearances::built_in().for_game("chess").unwrap().clone();
        assert!(check(&chess).is_empty());
        // The panels' dot of black the same as white's.
        let mut alike = chess.clone();
        alike.seats.get_mut("black").unwrap().paint = alike.seats["white"].paint;
        assert!(!check(&alike).is_empty());
        // Black's pieces as pale as white's.
        let mut pale = chess.clone();
        pale.materials.get_mut("ebonised").unwrap().tint = pale.materials["boxwood"].tint;
        let bad = check(&pale);
        assert!(bad.iter().any(|b| b.contains("tint")), "{bad:?}");
        // The same wood too, so that the pieces look alike as well.
        pale.materials.get_mut("ebonised").unwrap().wood = pale.materials["boxwood"].wood.clone();
        let bad = check(&pale);
        assert!(bad.iter().any(|b| b.contains("pieces")), "{bad:?}");
        // White darker than black.
        let mut swapped = chess.clone();
        let (w, b) = (swapped.seats["white"].paint, swapped.seats["black"].paint);
        swapped.seats.get_mut("white").unwrap().paint = b;
        swapped.seats.get_mut("black").unwrap().paint = w;
        let bad = check(&swapped);
        assert!(bad.iter().any(|b| b.contains("not lighter")), "{bad:?}");
        // Two colors of the standard look made alike.
        assert!(
            !breaches(&[
                ("red".into(), clamped(look::seat_paint("red", 1))),
                ("orange".into(), clamped(look::seat_paint("red", 1))),
            ])
            .is_empty()
        );
    }
}
