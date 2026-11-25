//! The board's coordinates along its edges, shown or hidden with C or a button (`toggles.rs`).
//!
//! The server names each position (`a1`, `k11`); where every name is letters then a number, as
//! for Hex, the letters name a column and the number a row. Each row's number goes beyond its
//! left and right ends, and each column's letters beyond its far and near ends where those are
//! on the board's first and last rows (a hexagonal board's columns also end on its slanted
//! sides, where the rows' numbers are). A board named any other
//! way shows none: the client does not guess. A game on several boards (Alice chess) has each
//! board's along its own edges.
//!
//! They are engraved in the board's margin: glyphs of the client's font, made by the asset
//! pipeline (`assets-src/blender/engraving.py`) as an atlas of the cut's colour and its walls'
//! slopes, laid flat on the wood and reading from the player's side, as on a real board. They
//! stand half way across the margin between the outermost spaces (or a stone on the outermost
//! lines) and the board's rounded rim (`board::Surface::label_at`), on any size of board.

use std::collections::BTreeMap;

use std::f32::consts::FRAC_PI_2;

use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;

use crate::appearance::Appearances;
use crate::board::{BOARD_HEIGHT, Layout, Surface};
use crate::net::WatchStatus;
use crate::net::api::BoardPosition;
use crate::textures;

pub struct CoordinatesPlugin;

impl Plugin for CoordinatesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShowCoordinates>().add_systems(
            Update,
            (
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

/// The labels of a board's edges: each row's number at its two ends across the board (left and
/// right), each column's letters at its ends along it (far and near) that are on the board's first
/// and last rows. None, unless every position is named as a column and a row.
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
    let (top, bottom) = positions.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
        (lo.min(p.y), hi.max(p.y))
    });
    let on_first_or_last_row =
        |p: &BoardPosition| (p.y - top).abs() < 0.01 || (p.y - bottom).abs() < 0.01;
    let mut out = Vec::new();
    let mut ends = |text: &str,
                    cells: &[&BoardPosition],
                    key: fn(&BoardPosition) -> f32,
                    axis: Vec2,
                    columns: bool| {
        let first = cells.iter().min_by(|a, b| key(a).total_cmp(&key(b)));
        let last = cells.iter().max_by(|a, b| key(a).total_cmp(&key(b)));
        if let (Some(first), Some(last)) = (first, last) {
            for (end, way) in [(first, -axis), (last, axis)] {
                if !columns || on_first_or_last_row(end) {
                    out.push(EdgeLabel {
                        text: text.to_string(),
                        position: end.id.clone(),
                        out: way,
                    });
                }
            }
        }
    };
    for (column, cells) in &columns {
        ends(column, cells, |p| p.y, Vec2::Y, true);
    }
    for (row, cells) in &rows {
        ends(row, cells, |p| p.x, Vec2::X, false);
    }
    out
}

/// The glyphs in the atlas, in order, and its grid (as `engraving.py` makes it).
const GLYPHS: &str = "0123456789abcdefghijklmnopqrstuvwxyz";
const COLUMNS: usize = 8;
const ROWS: usize = 5;
/// A glyph's cell, in pitches (its letters about a quarter of a pitch high), and how far apart a
/// label's glyphs stand, in cells: a little under the advance of the font's figures, since each
/// glyph is centred in its cell and a narrow one (1) would otherwise stand apart.
const GLYPH_SIZE: f32 = 0.44;
const ADVANCE: f32 = 0.36;

/// Where a glyph is in the atlas, if it has one.
pub fn glyph(ch: char) -> Option<usize> {
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
pub fn glyph_mesh(index: usize, size: f32) -> Mesh {
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

/// The cut a glyph is engraved with: dark where it is deep, its walls catching the light, laid
/// over the wood it is cut into.
pub fn engraving(server: &AssetServer) -> StandardMaterial {
    StandardMaterial {
        base_color_texture: Some(textures::load(
            server,
            "textures/engraving/glyphs_albedo.png",
        )),
        normal_map_texture: Some(textures::load(
            server,
            "textures/engraving/glyphs_normal.png",
        )),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.8,
        reflectance: 0.3,
        ..default()
    }
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
    appearances: Res<Appearances>,
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
    // Along each board's own edges, for a game on several (Alice chess).
    let labels: Vec<EdgeLabel> = crate::board::parts(&table.info.board)
        .iter()
        .flat_map(|part| edge_labels(part))
        .collect();
    if labels.is_empty() {
        return;
    }
    let surface = Surface::of(&table.info.board, appearances.for_game(&table.info.id));
    // A game with no board has no margin to engrave them in.
    if matches!(surface, Surface::Table(_)) {
        return;
    }
    let layout = Layout::of(&table.info.board);
    let material = materials.add(engraving(&server));
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
        let out = surface.label_at(label.out.y != 0.0, layout.pitch);
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
    fn on_a_hexagonal_board_a_column_ending_on_a_slanted_side_is_not_named_there() {
        let hexhex = [
            pos("b1", 1.0, 0.0),
            pos("c1", 2.0, 0.0),
            pos("a2", 0.5, 0.866),
            pos("b2", 1.5, 0.866),
            pos("c2", 2.5, 0.866),
            pos("b3", 1.0, 1.732),
            pos("c3", 2.0, 1.732),
        ];
        let labels = edge_labels(&hexhex);
        assert!(
            labels.iter().all(|l| l.text != "a"),
            "a ends on the sides only"
        );
        assert_eq!(labels.iter().filter(|l| l.text == "b").count(), 2);
        assert_eq!(
            labels.iter().filter(|l| l.text == "2").count(),
            2,
            "rows still"
        );
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
}
