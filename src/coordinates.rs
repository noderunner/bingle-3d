//! The board's coordinates along its edges, shown or hidden with C or a button (at the bottom
//! right, a stopgap until the interface has a place for it).
//!
//! The server names each position (`a1`, `k11`); where every name is letters then a number, as
//! for Hex, the letters name a column and the number a row. Each column's letters go beyond its
//! far and near ends, each row's number beyond its left and right ends. A board named any other
//! way shows none: the client does not guess.
//!
//! They are engraved in the board's margin: glyphs of the client's font, made by the asset
//! pipeline (`assets-src/blender/engraving.py`) as an atlas of the cut's colour and its walls'
//! slopes, laid flat on the wood and reading from the player's side, as on a real board.

use std::collections::BTreeMap;

use std::f32::consts::FRAC_PI_2;

use bevy::input_focus::InputFocus;
use bevy::mesh::VertexAttributeValues;
use bevy::picking::Pickable;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button, observe};

use crate::board::{BOARD_HEIGHT, Layout};
use crate::browser::colours;
use crate::fonts::UiFonts;
use crate::net::WatchStatus;
use crate::net::api::BoardPosition;
use crate::textures;

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
                    show_labels,
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

/// The glyphs in the atlas, in order, and its grid (as `engraving.py` makes it).
const GLYPHS: &str = "0123456789abcdefghijklmnopqrstuvwxyz";
const COLUMNS: usize = 8;
const ROWS: usize = 5;
/// A glyph's cell, in pitches, and how far apart a label's glyphs stand, in cells: the advance of
/// one of the font's figures at the size the glyphs are set.
const GLYPH_SIZE: f32 = 0.62;
const ADVANCE: f32 = 0.44;
/// How far beyond its position's middle a label stands, in pitches: on the board's margin,
/// clear of the cell's points (a hexagon's are towards the far and near sides).
const OUT_ALONG: f32 = 0.92;
const OUT_ACROSS: f32 = 0.85;

/// Where a glyph is in the atlas, if it has one.
fn glyph(ch: char) -> Option<usize> {
    GLYPHS.find(ch.to_ascii_lowercase())
}

/// A label's glyphs, and each one's offset from the label's middle, in cells, left to right.
fn glyphs(text: &str) -> Vec<(usize, f32)> {
    let found: Vec<usize> = text.chars().filter_map(glyph).collect();
    let middle = (found.len() as f32 - 1.0) / 2.0;
    found
        .into_iter()
        .enumerate()
        .map(|(i, g)| (g, (i as f32 - middle) * ADVANCE))
        .collect()
}

/// A glyph's square, flat on the table and reading from the player's side (its top away from
/// them), its texture coordinates its cell of the atlas.
fn glyph_mesh(index: usize, size: f32) -> Mesh {
    let (col, row) = ((index % COLUMNS) as f32, (index / COLUMNS) as f32);
    let mut mesh = Rectangle::new(size, size)
        .mesh()
        .build()
        .rotated_by(Quat::from_rotation_x(-FRAC_PI_2));
    if let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0) {
        for uv in uvs.iter_mut() {
            *uv = [(col + uv[0]) / COLUMNS as f32, (row + uv[1]) / ROWS as f32];
        }
    }
    if let Err(e) = mesh.generate_tangents() {
        warn!("an engraved glyph: {e}");
    }
    mesh
}

/// The engraved coordinates of the board being shown.
#[derive(Component)]
struct Labels {
    room_code: String,
}

fn sync_labels(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    show: Res<ShowCoordinates>,
    server: Res<AssetServer>,
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
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
    let labels = edge_labels(&table.info.board.positions);
    if labels.is_empty() {
        return;
    }
    let layout = Layout::new(&table.info.board.positions);
    // The cut: dark where it is deep, its walls catching the light.
    let material = materials.add(StandardMaterial {
        base_color_texture: Some(textures::load(
            &server,
            "textures/engraving/glyphs_albedo.png",
        )),
        normal_map_texture: Some(textures::load(
            &server,
            "textures/engraving/glyphs_normal.png",
        )),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.8,
        reflectance: 0.3,
        ..default()
    });
    let size = layout.pitch * GLYPH_SIZE;
    let mut shapes: Vec<Option<Handle<Mesh>>> = vec![None; GLYPHS.len()];
    let root = commands
        .spawn((
            Name::new("Coordinates"),
            Labels {
                room_code: table.room_code.clone(),
            },
            Transform::default(),
            visibility(show.0),
        ))
        .id();
    for label in labels {
        let Some(centre) = layout.centres.get(&label.position) else {
            continue;
        };
        let out = if label.out.y != 0.0 {
            OUT_ALONG
        } else {
            OUT_ACROSS
        };
        let middle = *centre + Vec3::new(label.out.x, 0.0, label.out.y) * layout.pitch * out;
        for (index, offset) in glyphs(&label.text) {
            let mesh = shapes[index]
                .get_or_insert_with(|| meshes.add(glyph_mesh(index, size)))
                .clone();
            commands.spawn((
                Name::new(format!("Coordinate {}", label.text)),
                Mesh3d(mesh),
                MeshMaterial3d(material.clone()),
                // Just over the wood, which it darkens and shapes.
                Transform::from_translation(
                    middle + Vec3::new(offset * size, BOARD_HEIGHT + 0.0001, 0.0),
                ),
                ChildOf(root),
            ));
        }
    }
}

fn visibility(show: bool) -> Visibility {
    if show {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    }
}

/// Shows or hides the engraving as the coordinates are turned on or off.
fn show_labels(show: Res<ShowCoordinates>, mut roots: Query<&mut Visibility, With<Labels>>) {
    if !show.is_changed() {
        return;
    }
    for mut v in &mut roots {
        v.set_if_neq(visibility(show.0));
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
    fn a_label_is_its_glyphs_side_by_side_round_its_middle() {
        assert_eq!(glyphs("a"), vec![(10, 0.0)]);
        let two = glyphs("11");
        assert_eq!(two.len(), 2);
        assert_eq!(two[0].0, 1);
        assert!((two[0].1 + two[1].1).abs() < 1e-6, "centred");
        assert!(two[0].1 < two[1].1, "left to right");
        assert_eq!(glyph('K'), glyph('k'));
        assert!(
            glyphs("?").is_empty(),
            "a character with no glyph is left out"
        );
    }

    #[test]
    fn a_glyph_reads_from_the_players_side_on_its_cell_of_the_atlas() {
        let mesh = glyph_mesh(GLYPHS.find('k').unwrap(), 1.0);
        let Some(VertexAttributeValues::Float32x3(p)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions");
        };
        let Some(VertexAttributeValues::Float32x2(uv)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            panic!("texture coordinates");
        };
        assert!(p.iter().all(|v| v[1].abs() < 1e-6), "flat on the table");
        // 'k' is the 21st glyph: row 2, column 4 of 8 by 5.
        for (v, t) in p.iter().zip(uv) {
            assert!(t[0] >= 4.0 / 8.0 - 1e-6 && t[0] <= 5.0 / 8.0 + 1e-6);
            assert!(t[1] >= 2.0 / 5.0 - 1e-6 && t[1] <= 3.0 / 5.0 + 1e-6);
            // The glyph's top (the texture's) is away from the player (-Z).
            if v[2] < 0.0 {
                assert!((t[1] - 2.0 / 5.0).abs() < 1e-6);
            }
        }
        assert!(mesh.attribute(Mesh::ATTRIBUTE_TANGENT).is_some());
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
