//! The board's coordinates along its edges, shown or hidden with C or a button (at the bottom
//! right, a stopgap until the interface has a place for it).
//!
//! The server names each position (`a1`, `k11`); where every name is letters then a number, as
//! for Hex, the letters name a column and the number a row. Each column's letters go beyond its
//! far and near ends, each row's number beyond its left and right ends. A board named any other
//! way shows none: the client does not guess.
//!
//! The labels are text over the scene, kept over their places on the table as the camera moves,
//! so they read the same from any angle.

use std::collections::BTreeMap;

use bevy::input_focus::InputFocus;
use bevy::picking::Pickable;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button, observe};

use crate::board::{Layout, SURFACE};
use crate::browser::colours;
use crate::fonts::UiFonts;
use crate::net::WatchStatus;
use crate::net::api::BoardPosition;

pub struct CoordinatesPlugin;

impl Plugin for CoordinatesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShowCoordinates>()
            .add_systems(Startup, spawn_button)
            .add_systems(
                Update,
                (
                    toggle_with_key,
                    colour_button,
                    sync_labels.run_if(resource_changed::<WatchStatus>),
                    place_labels,
                )
                    .chain(),
            );
    }
}

/// Whether the coordinates are shown.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ShowCoordinates(pub bool);

/// A coordinate beside a position, pointing out of the board.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeLabel {
    pub text: String,
    pub position: String,
    /// The way out of the board, in layout axes (x right, y down the board).
    pub out: Vec2,
}

/// A position's name as a column (letters) and a row (a number), if it is one.
fn split(id: &str) -> Option<(&str, &str)> {
    let at = id.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = id.split_at(at);
    let ok = !letters.is_empty()
        && letters.chars().all(|c| c.is_ascii_alphabetic())
        && digits.chars().all(|c| c.is_ascii_digit());
    ok.then_some((letters, digits))
}

/// The labels of a board's edges: each column's letters at its two ends along the board (far and
/// near), each row's number at its two ends across it (left and right). None, unless every
/// position is named as a column and a row.
pub fn edge_labels(positions: &[BoardPosition]) -> Vec<EdgeLabel> {
    let mut columns: BTreeMap<&str, Vec<&BoardPosition>> = BTreeMap::new();
    let mut rows: BTreeMap<&str, Vec<&BoardPosition>> = BTreeMap::new();
    for p in positions {
        let Some((column, row)) = split(&p.id) else {
            return Vec::new();
        };
        columns.entry(column).or_default().push(p);
        rows.entry(row).or_default().push(p);
    }
    let mut out = Vec::new();
    let mut ends =
        |text: &str, cells: &[&BoardPosition], key: fn(&BoardPosition) -> f32, axis: Vec2| {
            let first = cells.iter().min_by(|a, b| key(a).total_cmp(&key(b)));
            let last = cells.iter().max_by(|a, b| key(a).total_cmp(&key(b)));
            if let (Some(first), Some(last)) = (first, last) {
                out.push(EdgeLabel {
                    text: text.to_string(),
                    position: first.id.clone(),
                    out: -axis,
                });
                out.push(EdgeLabel {
                    text: text.to_string(),
                    position: last.id.clone(),
                    out: axis,
                });
            }
        };
    for (column, cells) in &columns {
        ends(column, cells, |p| p.y, Vec2::Y);
    }
    for (row, cells) in &rows {
        ends(row, cells, |p| p.x, Vec2::X);
    }
    out
}

/// How far beyond its position's middle a label stands, in pitches: on the board's margin.
const LABEL_OUT: f32 = 1.05;

/// A label on screen, and the point on the table it stays over.
#[derive(Component)]
struct Label {
    at: Vec3,
}

/// The labels' root, rebuilt for each board.
#[derive(Component)]
struct Labels {
    room_code: String,
}

fn sync_labels(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    fonts: Res<UiFonts>,
    roots: Query<(Entity, &Labels)>,
) {
    let room = watch.table.as_ref().map(|t| t.room_code.as_str());
    if roots
        .iter()
        .any(|(_, l)| Some(l.room_code.as_str()) == room)
    {
        return;
    }
    for (root, _) in &roots {
        commands.entity(root).despawn();
    }
    let Some(table) = &watch.table else { return };
    let layout = Layout::new(&table.info.board.positions);
    let root = commands
        .spawn((
            Labels {
                room_code: table.room_code.clone(),
            },
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .id();
    for label in edge_labels(&table.info.board.positions) {
        let Some(centre) = layout.centres.get(&label.position) else {
            continue;
        };
        let at = *centre
            + Vec3::new(label.out.x, 0.0, label.out.y) * layout.pitch * LABEL_OUT
            + Vec3::Y * SURFACE;
        commands.spawn((
            Label { at },
            Text::new(label.text),
            fonts.semibold(14.0),
            TextColor(Color::srgba(0.96, 0.94, 0.88, 0.92)),
            TextShadow::default(),
            TextLayout::justify(Justify::Center),
            Node {
                position_type: PositionType::Absolute,
                width: px(LABEL_BOX.x),
                height: px(LABEL_BOX.y),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Visibility::Hidden,
            Pickable::IGNORE,
            ChildOf(root),
        ));
    }
}

/// The box a label's text is centred in, in logical pixels.
const LABEL_BOX: Vec2 = Vec2::new(32.0, 18.0);

/// Keeps each label over its place on the table, or hidden when the coordinates are off or the
/// place is out of sight.
fn place_labels(
    show: Res<ShowCoordinates>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut labels: Query<(&Label, &mut Node, &mut Visibility)>,
) {
    let camera = cameras.iter().find(|(c, _)| c.is_active);
    for (label, mut node, mut visibility) in &mut labels {
        let seen = camera
            .filter(|_| show.0)
            .and_then(|(camera, at)| camera.world_to_viewport(at, label.at).ok());
        match seen {
            Some(p) => {
                node.left = px(p.x - LABEL_BOX.x / 2.0);
                node.top = px(p.y - LABEL_BOX.y / 2.0);
                visibility.set_if_neq(Visibility::Inherited);
            }
            None => {
                visibility.set_if_neq(Visibility::Hidden);
            }
        }
    }
}

fn toggle_with_key(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Option<Res<InputFocus>>,
    mut show: ResMut<ShowCoordinates>,
) {
    let typing = focus.is_some_and(|f| f.get().is_some());
    if keys.just_pressed(KeyCode::KeyC) && !typing {
        show.0 = !show.0;
    }
}

/// The button that shows or hides the coordinates.
#[derive(Component)]
struct CoordinatesButton;

fn spawn_button(mut commands: Commands, fonts: Res<UiFonts>) {
    commands.spawn((
        Button,
        CoordinatesButton,
        Hovered::default(),
        Node {
            position_type: PositionType::Absolute,
            right: px(12),
            bottom: px(66),
            padding: UiRect::axes(px(10), px(4)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(colours::BUTTON),
        observe(|_: On<Activate>, mut show: ResMut<ShowCoordinates>| show.0 = !show.0),
        children![(
            Text::new("Coordinates"),
            fonts.semibold(13.0),
            TextColor(colours::TEXT),
            Pickable::IGNORE,
        )],
    ));
}

fn colour_button(
    show: Res<ShowCoordinates>,
    mut buttons: Query<(&Hovered, &mut BackgroundColor), With<CoordinatesButton>>,
) {
    for (hovered, mut background) in &mut buttons {
        let colour = match (show.0, hovered.0) {
            (true, false) => colours::CHOSEN,
            (true, true) => colours::CHOSEN_HOVER,
            (false, false) => colours::BUTTON,
            (false, true) => colours::BUTTON_HOVER,
        };
        if background.0 != colour {
            background.0 = colour;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(id: &str, x: f32, y: f32) -> BoardPosition {
        BoardPosition {
            id: id.into(),
            x,
            y,
        }
    }

    /// A 3×3 hex board as Bingle lays it out: columns a to c, rows 1 to 3 down the board, each
    /// row half a cell to the right of the one above.
    fn hex3() -> Vec<BoardPosition> {
        let mut out = Vec::new();
        for r in 0..3 {
            for q in 0..3 {
                let id = format!("{}{}", (b'a' + q) as char, r + 1);
                out.push(pos(&id, q as f32 + r as f32 / 2.0, r as f32 * 0.866));
            }
        }
        out
    }

    #[test]
    fn columns_are_named_at_both_ends_and_rows_at_both_sides() {
        let labels = edge_labels(&hex3());
        assert_eq!(
            labels.len(),
            12,
            "three columns and three rows, two ends each"
        );
        let at = |text: &str, out: Vec2| {
            labels
                .iter()
                .find(|l| l.text == text && l.out == out)
                .map(|l| l.position.as_str())
        };
        assert_eq!(at("a", -Vec2::Y), Some("a1"), "a column's far end is row 1");
        assert_eq!(at("a", Vec2::Y), Some("a3"));
        assert_eq!(
            at("2", -Vec2::X),
            Some("a2"),
            "a row's left end is column a"
        );
        assert_eq!(at("2", Vec2::X), Some("c2"));
    }

    #[test]
    fn a_board_not_named_by_column_and_row_has_no_labels() {
        assert!(edge_labels(&[pos("a1", 0.0, 0.0), pos("centre", 1.0, 0.0)]).is_empty());
        assert!(edge_labels(&[pos("1a", 0.0, 0.0)]).is_empty());
        assert!(edge_labels(&[]).is_empty());
        assert_eq!(split("aa10"), Some(("aa", "10")));
        assert_eq!(split("a1b"), None);
    }

    #[test]
    fn c_shows_and_hides_them() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<ShowCoordinates>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, toggle_with_key);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyC);
        app.update();
        assert!(app.world().resource::<ShowCoordinates>().0);
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(KeyCode::KeyC);
        keys.clear();
        keys.press(KeyCode::KeyC);
        app.update();
        assert!(!app.world().resource::<ShowCoordinates>().0);
    }
}
