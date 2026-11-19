//! Generic defaults for how things look, for any game (Bingle's ADR 0124): the server says what
//! things are, and this client decides how they look. A per-game appearance may improve on them
//! later; no game needs one.

use bevy::prelude::*;

/// The colour a seat is drawn in. A seat's colour word is an identifier, not a colour, but most
/// games name their seats after the colour players know them by, so a word this client knows is
/// drawn as that colour. Any other word gets a colour from a palette by seat, so that every seat
/// still looks different.
pub fn seat_colour(word: &str, seat: u32) -> Color {
    named(word).unwrap_or_else(|| {
        let i = (seat.max(1) - 1) as usize % PALETTE.len();
        PALETTE[i]
    })
}

/// Distinct, fairly saturated colours that read well on a light board and on each other.
const PALETTE: [Color; 8] = [
    Color::srgb(0.80, 0.18, 0.16),
    Color::srgb(0.15, 0.33, 0.78),
    Color::srgb(0.95, 0.75, 0.15),
    Color::srgb(0.20, 0.60, 0.30),
    Color::srgb(0.55, 0.30, 0.70),
    Color::srgb(0.95, 0.50, 0.15),
    Color::srgb(0.15, 0.65, 0.70),
    Color::srgb(0.85, 0.40, 0.60),
];

fn named(word: &str) -> Option<Color> {
    Some(match word.to_ascii_lowercase().as_str() {
        "red" => PALETTE[0],
        "blue" => PALETTE[1],
        "yellow" | "gold" => PALETTE[2],
        "green" => PALETTE[3],
        "purple" | "violet" => PALETTE[4],
        "orange" => PALETTE[5],
        "cyan" | "teal" => PALETTE[6],
        "pink" => PALETTE[7],
        "white" => Color::srgb(0.92, 0.91, 0.87),
        "black" => Color::srgb(0.08, 0.08, 0.09),
        "grey" | "gray" | "silver" => Color::srgb(0.55, 0.56, 0.58),
        "brown" => Color::srgb(0.45, 0.28, 0.15),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_words_are_their_colour_and_others_differ_by_seat() {
        assert_eq!(seat_colour("Red", 2), seat_colour("red", 1));
        assert_ne!(seat_colour("red", 1), seat_colour("blue", 2));
        assert_eq!(seat_colour("cthonic", 1), PALETTE[0]);
        assert_ne!(seat_colour("cthonic", 1), seat_colour("abyssal", 2));
        assert_eq!(seat_colour("x", 9), PALETTE[0], "the palette goes round");
        assert_eq!(seat_colour("x", 0), PALETTE[0]);
    }
}
