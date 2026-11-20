//! The server's games, grouped by family for the list: one entry for a game, with its sizes,
//! layouts and variants as choices inside it, rather than one for each.
//!
//! The server gives a definition's `family` (the id of the game it belongs to) but no name for
//! the family, so one is made from the members' names: what they share before their
//! parenthesised part ("Go" for "Go (9×9)" and "Go (13×13)"), each member then going by that
//! part ("9×9"). Members that share no such name (different games of one family) keep their
//! own names, and the family takes the name of the member whose id it is. Nothing here names a
//! game.

use std::cmp::Ordering;

use crate::net::api::GameDef;

/// A game and its members, in the order the list shows them.
#[derive(Debug, Clone, PartialEq)]
pub struct Family<'a> {
    /// The family's id: the members' `family`, or the one member's id without one.
    pub id: String,
    pub title: String,
    /// In natural order of their names (7×7 before 11×11).
    pub members: Vec<Member<'a>>,
    /// The member chosen unless the player chooses another: the one whose id is the family's,
    /// or else the first.
    pub default: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Member<'a> {
    pub def: &'a GameDef,
    /// What its choice says, within the family.
    pub label: String,
}

impl<'a> Family<'a> {
    /// The member with this id, or the default one.
    pub fn member(&self, id: Option<&str>) -> &Member<'a> {
        id.and_then(|id| self.members.iter().find(|m| m.def.id == id))
            .unwrap_or(&self.members[self.default])
    }

    /// The marks on the family's card, as the web page puts them: *Prototype* while the rules are
    /// still changing, else *Beta* while its version is below 1.0 (both as its default member
    /// says), and *Tutorial* when any member has one.
    pub fn badges(&self) -> Vec<Badge> {
        let lead = self.members[self.default].def;
        let mut out = Vec::new();
        if lead.prototype {
            out.push(Badge::Prototype);
        } else if is_beta(&lead.version) {
            out.push(Badge::Beta);
        }
        if self.members.iter().any(|m| m.def.tutorials > 0) {
            out.push(Badge::Tutorial);
        }
        out
    }

    /// How many can sit at the family's tables, every member counted: "2 players", "3–5
    /// players". Empty when no member says.
    pub fn players(&self) -> String {
        let seats = self
            .members
            .iter()
            .map(|m| m.def.seats_required)
            .filter(|n| *n > 0);
        let (Some(lo), Some(hi)) = (seats.clone().min(), seats.max()) else {
            return String::new();
        };
        match (lo, hi) {
            (1, 1) => "1 player".into(),
            _ if lo == hi => format!("{lo} players"),
            _ => format!("{lo}\u{2013}{hi} players"),
        }
    }
}

/// A mark on a game's card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Badge {
    Prototype,
    Beta,
    Tutorial,
}

impl Badge {
    pub fn label(self) -> &'static str {
        match self {
            Badge::Prototype => "PROTOTYPE",
            Badge::Beta => "BETA",
            Badge::Tutorial => "Tutorial",
        }
    }
}

/// Whether a game's version is a beta: below 1.0.0, or not given.
pub fn is_beta(version: &str) -> bool {
    version
        .split('.')
        .next()
        .and_then(|major| major.trim().parse::<u64>().ok())
        .is_none_or(|major| major < 1)
}

/// The definitions grouped into families, in natural order of their titles.
pub fn families(defs: &[GameDef]) -> Vec<Family<'_>> {
    let mut groups: Vec<(String, Vec<&GameDef>)> = Vec::new();
    for def in defs {
        let id = def.family.clone().unwrap_or_else(|| def.id.clone());
        match groups.iter_mut().find(|(g, _)| *g == id) {
            Some((_, members)) => members.push(def),
            None => groups.push((id, vec![def])),
        }
    }
    let mut out: Vec<Family> = groups
        .into_iter()
        .map(|(id, mut defs)| {
            defs.sort_by(|a, b| natural(&a.name, &b.name));
            let default = defs.iter().position(|d| d.id == id).unwrap_or(0);
            let shared = base_name(&defs[0].name);
            let alike = defs.len() > 1 && defs.iter().all(|d| base_name(&d.name) == shared);
            let title = if defs.len() == 1 {
                defs[0].name.clone()
            } else if alike {
                shared.to_string()
            } else {
                defs[default].name.clone()
            };
            let members = defs
                .into_iter()
                .map(|def| Member {
                    label: match variant(&def.name) {
                        Some(v) if alike => v.to_string(),
                        _ => def.name.clone(),
                    },
                    def,
                })
                .collect();
            Family {
                id,
                title,
                members,
                default,
            }
        })
        .collect();
    out.sort_by(|a, b| natural(&a.title, &b.title));
    out
}

/// A name without its closing parenthesised part: "Go" for "Go (9×9)".
fn base_name(name: &str) -> &str {
    match variant(name) {
        Some(_) => name[..name.rfind(" (").unwrap()].trim_end(),
        None => name,
    }
}

/// The closing parenthesised part of a name, without its parentheses: "9×9" for "Go (9×9)".
fn variant(name: &str) -> Option<&str> {
    let inner = name.strip_suffix(')')?;
    let at = inner.rfind(" (")?;
    let v = &inner[at + 2..];
    (!v.is_empty() && at > 0).then_some(v)
}

/// Compares names as a person reads them: case aside, and runs of digits by their value, so
/// that "7×7" comes before "11×11".
pub fn natural(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let number = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut n: u64 = 0;
                    while let Some(d) = it.peek().and_then(|c| c.to_digit(10)) {
                        n = n.saturating_mul(10).saturating_add(u64::from(d));
                        it.next();
                    }
                    n
                };
                let order = number(&mut a).cmp(&number(&mut b));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.to_lowercase().cmp(y.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                a.next();
                b.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(id: &str, name: &str, family: Option<&str>) -> GameDef {
        GameDef {
            id: id.into(),
            name: name.into(),
            seats_required: 2,
            family: family.map(Into::into),
            description: String::new(),
            prototype: false,
            version: "1.0.0".into(),
            tutorials: 0,
        }
    }

    #[test]
    fn a_card_says_prototype_or_beta_and_whether_it_teaches() {
        let mut a = def("a", "A", None);
        assert!(families(std::slice::from_ref(&a))[0].badges().is_empty());
        a.version = "0.4.1".into();
        assert_eq!(
            families(std::slice::from_ref(&a))[0].badges(),
            [Badge::Beta]
        );
        a.prototype = true;
        a.tutorials = 2;
        assert_eq!(
            families(std::slice::from_ref(&a))[0].badges(),
            [Badge::Prototype, Badge::Tutorial],
            "a prototype is not also called a beta"
        );
        assert!(
            is_beta(""),
            "a game that gives no version is a beta, as on the page"
        );
        assert!(!is_beta("1.10.0"));
        assert!(!is_beta("2.0.0"));
    }

    #[test]
    fn a_card_says_how_many_play_its_members() {
        let mut small = def("x-3", "X (3 players)", Some("x"));
        small.seats_required = 3;
        let mut big = def("x", "X (5 players)", Some("x"));
        big.seats_required = 5;
        assert_eq!(
            families(&[small.clone(), big])[0].players(),
            "3\u{2013}5 players"
        );
        assert_eq!(
            families(std::slice::from_ref(&small))[0].players(),
            "3 players"
        );
        small.seats_required = 1;
        assert_eq!(
            families(std::slice::from_ref(&small))[0].players(),
            "1 player"
        );
        small.seats_required = 0;
        assert_eq!(families(&[small])[0].players(), "");
    }

    fn summary(f: &Family) -> (String, Vec<String>, String) {
        (
            f.title.clone(),
            f.members.iter().map(|m| m.label.clone()).collect(),
            f.member(None).def.id.clone(),
        )
    }

    #[test]
    fn sizes_of_one_game_are_one_entry_in_natural_order() {
        let defs = [
            def("sq-11", "Squares (11×11)", Some("sq-9")),
            def("sq-7", "Squares (7×7)", Some("sq-9")),
            def("sq-9", "Squares (9×9)", Some("sq-9")),
        ];
        let f = families(&defs);
        assert_eq!(f.len(), 1);
        assert_eq!(
            summary(&f[0]),
            (
                "Squares".into(),
                vec!["7×7".into(), "9×9".into(), "11×11".into()],
                "sq-9".into()
            ),
            "the family's own member is chosen to begin with"
        );
        assert_eq!(f[0].member(Some("sq-11")).label, "11×11");
        assert_eq!(
            f[0].member(Some("gone")).def.id,
            "sq-9",
            "a choice no longer offered falls back"
        );
    }

    #[test]
    fn a_member_without_a_variant_keeps_its_name_and_comes_first() {
        let defs = [
            def("rings-10", "Rings (base 10)", Some("rings")),
            def("rings", "Rings", Some("rings")),
            def("rings-4", "Rings (base 4)", Some("rings")),
        ];
        let f = families(&defs);
        assert_eq!(
            summary(&f[0]),
            (
                "Rings".into(),
                vec!["Rings".into(), "base 4".into(), "base 10".into()],
                "rings".into()
            )
        );
    }

    #[test]
    fn different_games_of_one_family_keep_their_names_under_the_familys() {
        let defs = [
            def("cells", "Free Cells", Some("cards")),
            def("cards", "Cards", Some("cards")),
            def("cards-3", "Cards (draw three)", Some("cards")),
        ];
        let f = families(&defs);
        assert_eq!(
            summary(&f[0]),
            (
                "Cards".into(),
                vec![
                    "Cards".into(),
                    "Cards (draw three)".into(),
                    "Free Cells".into()
                ],
                "cards".into()
            )
        );
    }

    #[test]
    fn a_game_alone_keeps_its_whole_name_and_families_are_sorted() {
        let defs = [
            def("dots", "Dots (15×15)", None),
            def("b", "beta", None),
            def("a", "Alpha (Original)", Some("a")),
        ];
        let titles: Vec<String> = families(&defs).iter().map(|f| f.title.clone()).collect();
        assert_eq!(titles, ["Alpha (Original)", "beta", "Dots (15×15)"]);
        assert_eq!(families(&defs)[2].members[0].label, "Dots (15×15)");
    }

    #[test]
    fn names_compare_as_read() {
        assert_eq!(natural("A (7×7)", "A (11×11)"), Ordering::Less);
        assert_eq!(natural("a", "B"), Ordering::Less);
        assert_eq!(natural("A", "A (base 4)"), Ordering::Less);
        assert_eq!(natural("x10", "x10"), Ordering::Equal);
        assert_eq!(variant("Go (9×9)"), Some("9×9"));
        assert_eq!(variant("(odd)"), None);
        assert_eq!(base_name("Go (9×9)"), "Go");
    }
}
