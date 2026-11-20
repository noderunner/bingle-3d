//! The board and pieces of the game being watched, built from the game definition and the
//! position alone (Bingle's ADR 0124): the board's kind and its positions' layout coordinates say
//! where every cell is, and each stack's pieces are drawn in their seat's colour.
//!
//! Nothing here names a game. A hex board's cells are hexagons and a grid's squares, and any other
//! board's positions are round spots; the same code draws every board of each kind.
//!
//! A seat's goal is drawn from the game's `goals` and the board's `regions`: a strip in the seat's
//! colour along the outer sides of each region the seat must connect (a hex board's edges), and a
//! neutral one for a goal every seat shares. Only `connect` goals are drawn; any other type is
//! skipped, as the API asks.
//!
//! While this client's seat is on impulse, the positions a click would be taken on are lit, and
//! the move being built is shown as see-through pieces in the seat's colour: where its selections
//! put something, and under the pointer, where a click would.

use std::collections::{BTreeMap, HashMap};
use std::f32::consts::{FRAC_PI_2, TAU};
use std::time::Duration;

use bevy::picking::Pickable;
use bevy::prelude::*;

use crate::look::seat_colour;
use crate::net::WatchStatus;
use crate::net::api::{Board, BoardPosition, GameInfo, PieceOnBoard};
use crate::play::{Hovering, Spot, Stage};
use crate::table::Table;

pub struct BoardPlugin;

impl Plugin for BoardPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<BoardBuilt>().add_systems(
            Update,
            (
                sync_board.run_if(resource_changed::<WatchStatus>),
                show_turn
                    .run_if(resource_changed::<WatchStatus>.or_eager(resource_changed::<Hovering>)),
                drop_pieces,
            )
                .chain(),
        );
    }
}

/// Metres between neighbouring cells, unless the board would then be too big for the table.
pub const PITCH: f32 = 0.028;
/// The widest a board may be, so a large one still fits in front of the player.
pub const MAX_SIZE: f32 = 0.5;

/// The base the cells stand on, and the cells on it.
const BASE_HEIGHT: f32 = 0.016;
const CELL_HEIGHT: f32 = 0.003;
/// The top of the cells, where pieces stand.
pub const SURFACE: f32 = BASE_HEIGHT + CELL_HEIGHT;

/// A board has been put on the table, centred on the origin: the camera may frame it.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct BoardBuilt {
    /// Its width (X) and depth (Z), in metres.
    pub size: Vec2,
}

/// Where every position of a board is on the table.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// Metres per layout unit: the distance between neighbours.
    pub pitch: f32,
    /// Each position's centre on the table plane (y = 0), the board centred on the origin.
    pub centres: HashMap<String, Vec3>,
    /// The width (X) and depth (Z) its cells cover.
    pub size: Vec2,
}

impl Layout {
    /// Lays a board's positions out on the table: layout x to the right (+X), layout y (which
    /// grows downwards) towards the player (+Z), so the top of the board is the far side.
    pub fn new(positions: &[BoardPosition]) -> Self {
        let (min, max) = positions.iter().fold(
            (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
            |(lo, hi), p| {
                let v = Vec2::new(p.x, p.y);
                (lo.min(v), hi.max(v))
            },
        );
        if positions.is_empty() {
            return Layout {
                pitch: PITCH,
                centres: HashMap::new(),
                size: Vec2::ZERO,
            };
        }
        // One unit more than the span: the cells at the edges are a whole cell wide.
        let span = max - min + 1.0;
        let pitch = PITCH.min(MAX_SIZE / span.max_element());
        let middle = (min + max) / 2.0;
        let centres = positions
            .iter()
            .map(|p| {
                let v = (Vec2::new(p.x, p.y) - middle) * pitch;
                (p.id.clone(), Vec3::new(v.x, 0.0, v.y))
            })
            .collect();
        Layout {
            pitch,
            centres,
            size: span * pitch,
        }
    }
}

/// The shape of a board's cells, from its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellShape {
    Hexagon,
    Square,
    Spot,
}

impl CellShape {
    pub fn of(board: &Board) -> Self {
        match board.kind.as_str() {
            "hex" => CellShape::Hexagon,
            "grid" => CellShape::Square,
            _ => CellShape::Spot,
        }
    }

    /// A cell's mesh, standing on y = 0, `scale` of the full size that would touch its neighbours.
    fn mesh(self, pitch: f32, scale: f32, height: f32) -> Mesh {
        let size = pitch * scale;
        match self {
            // Neighbours are one pitch apart across flat sides, so the hexagon's inradius is half
            // a pitch. Its points are towards the far and near sides, as the layout's rows are.
            CellShape::Hexagon => {
                Extrusion::new(RegularPolygon::new(size / 3f32.sqrt(), 6), height)
                    .mesh()
                    .build()
                    .rotated_by(Quat::from_rotation_x(-FRAC_PI_2))
            }
            CellShape::Square => Cuboid::new(size, height, size).mesh().build(),
            CellShape::Spot => Cylinder::new(size * 0.3, height).mesh().build(),
        }
        .translated_by(Vec3::Y * height / 2.0)
    }
}

/// What the scene shows, so the board is built again only when the game changes.
#[derive(Component)]
struct BoardRoot {
    room_code: String,
}

/// A piece on the board, by what it is and where.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
struct PieceAt {
    cell: String,
    layer: usize,
    piece: PieceOnBoard,
}

/// A cell of the board, which is lit while a click on it would be taken.
#[derive(Component)]
struct Cell(String);

/// A see-through piece: part of the move being built, or where a click would put one.
#[derive(Component)]
struct Ghost;

/// The mark on the cell of the last move.
#[derive(Component)]
struct LastMoveMark;

/// A piece on its way down to where it was played.
#[derive(Component)]
struct Dropping {
    timer: Timer,
    to: f32,
}

const DROP_HEIGHT: f32 = 0.06;
const DROP_TIME: Duration = Duration::from_millis(280);

/// The meshes and materials of the board being shown.
#[derive(Component)]
struct Kit {
    stone: Handle<Mesh>,
    stone_height: f32,
    materials: HashMap<String, Handle<StandardMaterial>>,
    cell: Handle<StandardMaterial>,
    /// A cell a click would be taken on.
    target: Handle<StandardMaterial>,
    /// A piece of the move being built, in this client's seat's colour.
    ghost: Handle<StandardMaterial>,
}

fn sync_board(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    // Together, to keep the argument count within clippy's limit.
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
    roots: Query<(Entity, &BoardRoot, &Kit)>,
    pieces: Query<(Entity, &PieceAt)>,
    mut marks: Query<(&mut Transform, &mut Visibility), With<LastMoveMark>>,
    mut built: MessageWriter<BoardBuilt>,
) {
    let Some(table) = &watch.table else {
        for (root, ..) in &roots {
            commands.entity(root).despawn();
        }
        return;
    };
    let current = roots
        .iter()
        .find(|(_, shown, _)| shown.room_code == table.room_code);
    let Some((root, _, kit)) = current else {
        for (old, ..) in &roots {
            commands.entity(old).despawn();
        }
        let layout = Layout::new(&table.info.board.positions);
        let kit = build_board(&mut commands, &mut meshes, &mut materials, table, &layout);
        built.write(BoardBuilt { size: layout.size });
        // A board just built shows the position as it is, without dropping every piece.
        let root = kit.0;
        place_pieces(&mut commands, root, &kit.1, &layout, table, &[], false);
        commands.entity(root).insert(kit.1);
        return;
    };

    let layout = Layout::new(&table.info.board.positions);
    let shown: Vec<(Entity, &PieceAt)> = pieces.iter().collect();
    place_pieces(&mut commands, root, kit, &layout, table, &shown, true);
    for (mut t, mut vis) in &mut marks {
        match table.last_move.as_ref().and_then(|c| layout.centres.get(c)) {
            Some(centre) => {
                t.translation = *centre + Vec3::Y * (SURFACE + 0.0005);
                *vis = Visibility::Inherited;
            }
            None => *vis = Visibility::Hidden,
        }
    }
}

/// The pieces the position has and the scene does not are added (dropping in when `animate`),
/// and those the scene has and the position does not are taken away.
fn place_pieces(
    commands: &mut Commands,
    root: Entity,
    kit: &Kit,
    layout: &Layout,
    table: &Table,
    shown: &[(Entity, &PieceAt)],
    animate: bool,
) {
    let wanted = wanted_pieces(table);
    for (entity, at) in shown {
        if !wanted.contains(at) {
            commands.entity(*entity).despawn();
        }
    }
    for at in wanted {
        if shown.iter().any(|(_, s)| **s == at) {
            continue;
        }
        let Some(centre) = layout.centres.get(&at.cell) else {
            // A piece on a position the board does not have: not a board cell (a reserve, say),
            // which this stage does not draw yet.
            continue;
        };
        let rest = SURFACE + kit.stone_height * (at.layer as f32 + 0.5);
        let material = kit
            .materials
            .get(&at.piece.color)
            .cloned()
            .unwrap_or_default();
        let start = if animate { rest + DROP_HEIGHT } else { rest };
        let mut piece = commands.spawn((
            Name::new(format!("{} at {}", at.piece.piece, at.cell)),
            Mesh3d(kit.stone.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(Vec3::new(centre.x, start, centre.z)),
            // A click on a piece is a click on where it stands.
            Spot(at.cell.clone()),
            Pickable::default(),
            at,
            ChildOf(root),
        ));
        if animate {
            piece.insert(Dropping {
                timer: Timer::new(DROP_TIME, TimerMode::Once),
                to: rest,
            });
        }
    }
}

/// Every piece of the position, bottom of each stack first.
fn wanted_pieces(table: &Table) -> Vec<PieceAt> {
    table
        .state
        .state
        .cells
        .iter()
        .flat_map(|(cell, stack)| {
            stack.iter().enumerate().map(|(layer, piece)| PieceAt {
                cell: cell.clone(),
                layer,
                piece: piece.clone(),
            })
        })
        .collect()
}

fn build_board(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    table: &Table,
    layout: &Layout,
) -> (Entity, Kit) {
    let shape = CellShape::of(&table.info.board);
    let pitch = layout.pitch;
    let base_mesh = meshes.add(shape.mesh(pitch, 1.0, BASE_HEIGHT));
    let cell_mesh = meshes.add(shape.mesh(pitch, 0.9, CELL_HEIGHT));
    let base = materials.add(StandardMaterial {
        base_color: Color::srgb(0.42, 0.30, 0.20),
        perceptual_roughness: 0.8,
        ..default()
    });
    let cell = materials.add(StandardMaterial {
        base_color: Color::srgb(0.86, 0.80, 0.68),
        perceptual_roughness: 0.7,
        ..default()
    });
    let target = materials.add(StandardMaterial {
        base_color: Color::srgb(0.95, 0.92, 0.80),
        emissive: LinearRgba::rgb(0.10, 0.12, 0.05),
        perceptual_roughness: 0.6,
        ..default()
    });

    let root = commands
        .spawn((
            Name::new(format!("Board of {}", table.info.name)),
            BoardRoot {
                room_code: table.room_code.clone(),
            },
            Transform::default(),
            Visibility::default(),
        ))
        .id();
    // Round spots stand on one slab; hexagons and squares make their own base, which keeps the
    // board's outline (a hex board's rhombus, say).
    if shape == CellShape::Spot {
        let (lo, hi) = layout.centres.values().fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(lo, hi), c| (lo.min(*c), hi.max(*c)),
        );
        if lo.x.is_finite() {
            let size = hi - lo + Vec3::splat(pitch);
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(size.x, BASE_HEIGHT, size.z))),
                MeshMaterial3d(base.clone()),
                Transform::from_translation((lo + hi) / 2.0 + Vec3::Y * BASE_HEIGHT / 2.0),
                ChildOf(root),
            ));
        }
    }
    for (id, centre) in &layout.centres {
        if shape != CellShape::Spot {
            commands.spawn((
                Mesh3d(base_mesh.clone()),
                MeshMaterial3d(base.clone()),
                Transform::from_translation(*centre),
                ChildOf(root),
            ));
        }
        commands.spawn((
            Name::new(format!("Cell {id}")),
            Mesh3d(cell_mesh.clone()),
            MeshMaterial3d(cell.clone()),
            Transform::from_translation(*centre + Vec3::Y * BASE_HEIGHT),
            Cell(id.clone()),
            Spot(id.clone()),
            Pickable::default(),
            ChildOf(root),
        ));
    }

    draw_goals(commands, meshes, materials, root, table, layout, shape);

    let mark = materials.add(StandardMaterial {
        base_color: Color::srgb(0.98, 0.84, 0.30),
        emissive: LinearRgba::rgb(0.6, 0.45, 0.1),
        ..default()
    });
    commands.spawn((
        Name::new("Last move"),
        LastMoveMark,
        Mesh3d(meshes.add(Torus::new(pitch * 0.40, pitch * 0.47))),
        MeshMaterial3d(mark),
        Transform::default(),
        Visibility::Hidden,
        ChildOf(root),
    ));

    let stone_height = pitch * 0.28;
    let yours = table.yours();
    let ghost = materials.add(StandardMaterial {
        base_color: seat_colour(
            yours.map_or("", |s| s.color.as_str()),
            yours.map_or(1, |s| s.seat),
        )
        .with_alpha(0.5),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.35,
        ..default()
    });
    let kit = Kit {
        stone: meshes.add(Cylinder::new(pitch * 0.40, stone_height)),
        stone_height,
        materials: seat_materials(&table.info, materials),
        cell,
        target,
        ghost,
    };
    (root, kit)
}

/// A strip along the outer side of a cell, marking a region a goal names.
#[derive(Debug, Clone, PartialEq)]
pub struct GoalStrip {
    pub region: String,
    /// The seat whose goal the region is, or `None` when it is more than one seat's.
    pub seat: Option<u32>,
    /// The middle of the strip, on the table plane.
    pub centre: Vec3,
    /// The way the side faces, out of the board (X and Z).
    pub normal: Vec2,
    pub length: f32,
}

/// How wide a goal strip is, as a share of the pitch.
const STRIP_WIDTH: f32 = 0.16;

/// Where the strips of the game's `connect` goals go. Each region's cells' sides that face off the
/// board are marked; a side of a corner cell that is in two regions (Hex's corners) goes to the
/// region it faces most, so each edge of the board shows one colour.
pub fn goal_strips(info: &GameInfo, layout: &Layout, shape: CellShape) -> Vec<GoalStrip> {
    // Which seat each region is the goal of.
    let mut owners: BTreeMap<&str, Option<u32>> = BTreeMap::new();
    for goal in info.goals.iter().filter(|g| g.kind == "connect") {
        for region in goal.named_regions() {
            let owner = owners.entry(region).or_insert(goal.seat);
            if *owner != goal.seat {
                *owner = None;
            }
        }
    }
    let flat = |v: Vec3| Vec2::new(v.x, v.z);
    let cells: Vec<Vec2> = layout.centres.values().map(|c| flat(*c)).collect();
    if cells.is_empty() {
        return Vec::new();
    }
    let middle = cells.iter().sum::<Vec2>() / cells.len() as f32;

    // Each goal region's cells, and the way it faces out of the board.
    struct Region<'a> {
        name: &'a str,
        seat: Option<u32>,
        cells: Vec<Vec2>,
        outward: Vec2,
    }
    let regions: Vec<Region> = owners
        .iter()
        .filter_map(|(name, seat)| {
            let cells: Vec<Vec2> = info
                .board
                .regions
                .get(*name)?
                .iter()
                .filter_map(|id| layout.centres.get(id).map(|c| flat(*c)))
                .collect();
            if cells.is_empty() {
                return None;
            }
            let outward = outward(&cells, middle)?;
            Some(Region {
                name,
                seat: *seat,
                cells,
                outward,
            })
        })
        .collect();

    let pitch = layout.pitch;
    let (sides, apothem, side_length): (Vec<Vec2>, f32, f32) = match shape {
        CellShape::Hexagon => (
            (0..6)
                .map(|i| Vec2::from_angle(i as f32 * TAU / 6.0))
                .collect(),
            pitch / 2.0,
            pitch / 3f32.sqrt(),
        ),
        CellShape::Square => (
            (0..4)
                .map(|i| Vec2::from_angle(i as f32 * TAU / 4.0))
                .collect(),
            pitch / 2.0,
            pitch,
        ),
        // Spots have no sides: one short strip beside each, facing out of the board.
        CellShape::Spot => (Vec::new(), pitch * 0.3, pitch * 0.5),
    };
    let off_board = |cell: Vec2, side: Vec2| {
        let beyond = cell + side * pitch;
        !cells.iter().any(|c| c.distance(beyond) < pitch * 0.25)
    };

    let mut strips = Vec::new();
    let mut taken: Vec<(Vec2, Vec2)> = Vec::new();
    for region in &regions {
        for &cell in &region.cells {
            let facing: Vec<Vec2> = if shape == CellShape::Spot {
                vec![region.outward]
            } else {
                sides
                    .iter()
                    .copied()
                    .filter(|&side| off_board(cell, side))
                    .collect()
            };
            for side in facing {
                // The region of this cell that the side faces most. A side that faces two
                // equally (a hex board's obtuse corners) goes to the first by name, so the
                // same board is always marked the same way.
                let mut best: Option<(&str, f32)> = None;
                for r in regions
                    .iter()
                    .filter(|r| r.cells.iter().any(|c| c.distance(cell) < 1e-5))
                {
                    let dot = side.dot(r.outward);
                    if best.is_none_or(|(_, d)| dot > d + 1e-3) {
                        best = Some((r.name, dot));
                    }
                }
                let best = best.map(|(name, _)| name);
                if best != Some(region.name) || side.dot(region.outward) < 0.3 {
                    continue;
                }
                if taken.contains(&(cell, side)) {
                    continue;
                }
                taken.push((cell, side));
                let at = cell + side * (apothem + pitch * STRIP_WIDTH / 2.0);
                strips.push(GoalStrip {
                    region: region.name.to_string(),
                    seat: region.seat,
                    centre: Vec3::new(at.x, 0.0, at.y),
                    normal: side,
                    length: side_length,
                });
            }
        }
    }
    strips
}

/// The way a region faces out of the board: square to the line its cells lie along (a board's
/// edge), away from the board's middle. A region that is not a line faces the way its middle
/// lies from the board's.
fn outward(cells: &[Vec2], middle: Vec2) -> Option<Vec2> {
    let n = cells.len() as f32;
    let centre = cells.iter().sum::<Vec2>() / n;
    let away = centre - middle;
    // The covariance of the cells' positions: its main axis is the line they lie along.
    let (mut xx, mut xy, mut yy) = (0.0, 0.0, 0.0);
    for c in cells {
        let d = *c - centre;
        xx += d.x * d.x;
        xy += d.x * d.y;
        yy += d.y * d.y;
    }
    let mean = (xx + yy) / 2.0;
    let spread = (((xx - yy) / 2.0).powi(2) + xy * xy).sqrt();
    let (major, minor) = (mean + spread, mean - spread);
    if cells.len() < 2 || minor > major * 0.1 {
        return away.try_normalize();
    }
    let along = Vec2::from_angle(0.5 * f32::atan2(2.0 * xy, xx - yy));
    let out = along.perp();
    Some(if out.dot(away) < 0.0 { -out } else { out })
}

fn draw_goals(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    root: Entity,
    table: &Table,
    layout: &Layout,
    shape: CellShape,
) {
    let strips = goal_strips(&table.info, layout, shape);
    if strips.is_empty() {
        return;
    }
    let height = SURFACE * 0.9;
    // A little longer than the side, so that strips meet round a hex board's zigzag edge.
    let mesh = meshes.add(Cuboid::new(
        strips[0].length * 1.12,
        height,
        layout.pitch * STRIP_WIDTH,
    ));
    let mut by_seat: HashMap<Option<u32>, Handle<StandardMaterial>> = HashMap::new();
    for strip in strips {
        let material = by_seat
            .entry(strip.seat)
            .or_insert_with(|| {
                let colour = match strip
                    .seat
                    .and_then(|s| table.info.seats.iter().find(|d| d.seat == s))
                {
                    Some(seat) => seat_colour(&seat.color, seat.seat),
                    // A goal every seat shares.
                    None => Color::srgb(0.25, 0.22, 0.20),
                };
                materials.add(StandardMaterial {
                    base_color: colour,
                    perceptual_roughness: 0.5,
                    ..default()
                })
            })
            .clone();
        // The strip's length runs along the side, square to the way it faces.
        let along = strip.normal.perp();
        commands.spawn((
            Name::new(format!("Goal: {}", strip.region)),
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(strip.centre + Vec3::Y * height / 2.0)
                .with_rotation(Quat::from_rotation_y(f32::atan2(-along.y, along.x))),
            ChildOf(root),
        ));
    }
}

/// One material per seat colour word. A piece in a colour no seat plays is drawn in the default
/// material, which no seat uses.
fn seat_materials(
    info: &GameInfo,
    materials: &mut Assets<StandardMaterial>,
) -> HashMap<String, Handle<StandardMaterial>> {
    info.seats
        .iter()
        .map(|s| {
            let handle = materials.add(StandardMaterial {
                base_color: seat_colour(&s.color, s.seat),
                perceptual_roughness: 0.35,
                ..default()
            });
            (s.color.clone(), handle)
        })
        .collect()
}

/// Lights the cells a click would be taken on, and shows the move being built.
fn show_turn(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    hovering: Res<Hovering>,
    roots: Query<(Entity, &Kit), With<BoardRoot>>,
    mut cells: Query<(&Cell, &mut MeshMaterial3d<StandardMaterial>)>,
    ghosts: Query<Entity, With<Ghost>>,
) {
    for ghost in &ghosts {
        commands.entity(ghost).despawn();
    }
    let (Some(table), Ok((root, kit))) = (&watch.table, roots.single()) else {
        return;
    };
    let turn = watch.turn.as_ref();
    let targets = turn.map(|t| t.targets()).unwrap_or_default();
    for (cell, mut material) in &mut cells {
        let wanted = if targets.contains(&cell.0) {
            &kit.target
        } else {
            &kit.cell
        };
        if material.0 != *wanted {
            material.0 = wanted.clone();
        }
    }
    let Some(turn) = turn else { return };
    let mut shown: Vec<&str> = match turn.stage {
        Stage::Asking | Stage::Ready | Stage::Sending => turn.placed().collect(),
        Stage::Choosing | Stage::Failed => Vec::new(),
    };
    if let Some(cell) = hovering.0.as_deref().filter(|c| targets.contains(*c)) {
        shown.push(cell);
    }
    let layout = Layout::new(&table.info.board.positions);
    for cell in shown {
        let Some(centre) = layout.centres.get(cell) else {
            continue;
        };
        let below = table.state.state.cells.get(cell).map_or(0, Vec::len);
        let y = SURFACE + kit.stone_height * (below as f32 + 0.5);
        commands.spawn((
            Name::new(format!("Ghost at {cell}")),
            Ghost,
            Mesh3d(kit.stone.clone()),
            MeshMaterial3d(kit.ghost.clone()),
            Transform::from_translation(Vec3::new(centre.x, y, centre.z)),
            ChildOf(root),
        ));
    }
}

fn drop_pieces(
    mut commands: Commands,
    time: Res<Time>,
    mut pieces: Query<(Entity, &mut Dropping, &mut Transform)>,
) {
    for (entity, mut drop, mut t) in &mut pieces {
        drop.timer.tick(time.delta());
        let f = drop.timer.fraction();
        // Falls faster and faster, as a dropped stone does.
        t.translation.y = drop.to + DROP_HEIGHT * (1.0 - f * f);
        if drop.timer.is_finished() {
            t.translation.y = drop.to;
            commands.entity(entity).remove::<Dropping>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::api::Goal;

    fn pos(id: &str, x: f32, y: f32) -> BoardPosition {
        BoardPosition {
            id: id.into(),
            x,
            y,
        }
    }

    /// A 3×3 hex board's positions as Bingle lays them out: each row half a cell to the right of
    /// the one above, rows √3/2 apart.
    fn hex3() -> Vec<BoardPosition> {
        let mut out = Vec::new();
        for r in 0..3 {
            for q in 0..3 {
                let id = format!("{}{}", (b'a' + q) as char, r + 1);
                out.push(pos(&id, q as f32 + r as f32 / 2.0, r as f32 * 0.866_025_4));
            }
        }
        out
    }

    #[test]
    fn neighbours_are_one_pitch_apart_and_the_board_is_centred() {
        let layout = Layout::new(&hex3());
        assert_eq!(layout.pitch, PITCH);
        let c = |id: &str| layout.centres[id];
        assert!((c("a1").distance(c("b1")) - PITCH).abs() < 1e-5);
        assert!(
            (c("a1").distance(c("a2")) - PITCH).abs() < 1e-5,
            "hex neighbours too"
        );
        assert!(c("b2").length() < 1e-5, "the middle cell is at the origin");
        assert!(c("a1").z < c("a3").z, "the top row is on the far side");
        assert!(c("a1").x < c("c1").x, "columns run to the right");
        assert!(layout.centres.values().all(|v| v.y == 0.0));
        // Three columns plus the last row's shift of one cell; three rows √3/2 apart.
        let expected = Vec2::new(4.0, 1.0 + 2.0 * 0.866_025_4) * PITCH;
        assert!(layout.size.distance(expected) < 1e-5, "{}", layout.size);
    }

    #[test]
    fn a_big_board_is_shrunk_to_fit_the_table() {
        let positions: Vec<_> = (0..40)
            .flat_map(|y| (0..40).map(move |x| pos(&format!("{x},{y}"), x as f32, y as f32)))
            .collect();
        let layout = Layout::new(&positions);
        assert!(layout.pitch < PITCH);
        let xs = layout.centres.values().map(|v| v.x);
        let width = xs.clone().fold(f32::MIN, f32::max) - xs.fold(f32::MAX, f32::min);
        assert!(width + layout.pitch <= MAX_SIZE + 1e-4, "{width}");
    }

    mod systems {
        use super::*;
        use crate::net::api::{Event, Game, GameState, Position, SeatDef};
        use crate::net::session::Opened;

        fn app() -> App {
            let mut app = App::new();
            app.add_plugins((MinimalPlugins, AssetPlugin::default(), BoardPlugin))
                .init_asset::<Mesh>()
                .init_asset::<StandardMaterial>()
                .init_resource::<WatchStatus>()
                .init_resource::<Hovering>();
            app
        }

        fn state(seq: u64, cells: &[&str]) -> GameState {
            GameState {
                state: Position {
                    cells: cells
                        .iter()
                        .map(|c| {
                            let stone = PieceOnBoard {
                                piece: "stone".into(),
                                color: "red".into(),
                            };
                            (c.to_string(), vec![stone])
                        })
                        .collect(),
                },
                status: "active".into(),
                move_seq: seq,
                ..Default::default()
            }
        }

        fn table() -> Table {
            Table::new(Opened {
                room_code: "AB12CD".into(),
                info: GameInfo {
                    id: "g".into(),
                    name: "A game".into(),
                    seats: vec![SeatDef {
                        seat: 1,
                        color: "red".into(),
                        label: "Red".into(),
                    }],
                    board: Board {
                        kind: "hex".into(),
                        positions: hex3(),
                        regions: Default::default(),
                    },
                    actions: vec![],
                    goals: vec![],
                },
                game: Game {
                    game_def_id: "g".into(),
                    status: "active".into(),
                    invite_url: String::new(),
                    clock: None,
                    impulse_deadline: String::new(),
                    server_time: String::new(),
                    move_time_seconds: None,
                    lobby: false,
                    host_seat: None,
                    seats: vec![],
                },
                state: state(1, &["a1"]),
                bots: vec![],
            })
        }

        fn pieces(app: &mut App) -> Vec<(String, bool)> {
            let mut q = app.world_mut().query::<(&PieceAt, Option<&Dropping>)>();
            let mut out: Vec<_> = q
                .iter(app.world())
                .map(|(p, d)| (p.cell.clone(), d.is_some()))
                .collect();
            out.sort();
            out
        }

        #[test]
        fn a_table_is_built_framed_and_follows_the_moves() {
            let mut app = app();
            app.world_mut().resource_mut::<WatchStatus>().table = Some(table());
            app.update();
            let built: Vec<BoardBuilt> = app
                .world_mut()
                .resource_mut::<Messages<BoardBuilt>>()
                .drain()
                .collect();
            assert_eq!(built.len(), 1, "the camera is told about the new board");
            assert_eq!(
                pieces(&mut app),
                [("a1".to_string(), false)],
                "the position as it was, not dropping"
            );

            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            let t = watch.table.as_mut().unwrap();
            t.apply(&Event::Move {
                seq: 2,
                state: state(2, &["a1", "b2"]),
            });
            app.update();
            assert_eq!(
                pieces(&mut app),
                [("a1".to_string(), false), ("b2".to_string(), true)],
                "only the new stone drops in"
            );
            assert!(
                app.world_mut()
                    .resource_mut::<Messages<BoardBuilt>>()
                    .drain()
                    .next()
                    .is_none(),
                "the same board is not built again"
            );

            app.world_mut().resource_mut::<WatchStatus>().table = None;
            app.update();
            assert!(pieces(&mut app).is_empty(), "the board goes with the game");
            let mut roots = app.world_mut().query::<&BoardRoot>();
            assert_eq!(roots.iter(app.world()).count(), 0);
        }
    }

    fn hex3_with_goals() -> GameInfo {
        let regions = [
            ("top", ["a1", "b1", "c1"]),
            ("bottom", ["a3", "b3", "c3"]),
            ("left", ["a1", "a2", "a3"]),
            ("right", ["c1", "c2", "c3"]),
        ];
        let goal = |seat, a: &str, b: &str| Goal {
            kind: "connect".into(),
            seat: Some(seat),
            regions: vec![vec![a.into(), b.into()]],
            of: vec![],
        };
        GameInfo {
            id: "g".into(),
            name: "A game".into(),
            seats: vec![],
            board: Board {
                kind: "hex".into(),
                positions: hex3(),
                regions: regions
                    .iter()
                    .map(|(n, cells)| (n.to_string(), cells.map(String::from).to_vec()))
                    .collect(),
            },
            actions: vec![],
            goals: vec![goal(1, "top", "bottom"), goal(2, "left", "right")],
        }
    }

    #[test]
    fn each_edge_of_a_hex_board_is_marked_once_in_its_seats_colour() {
        let info = hex3_with_goals();
        let layout = Layout::new(&info.board.positions);
        let strips = goal_strips(&info, &layout, CellShape::Hexagon);
        let of = |region: &str| -> Vec<&GoalStrip> {
            strips.iter().filter(|s| s.region == region).collect()
        };
        // Two sides of each cell face off the board, and every one is marked once. The side
        // between two edges at each obtuse corner goes to the first edge by name: bottom rather
        // than left, right rather than top.
        assert_eq!(strips.len(), 22);
        assert_eq!(of("top").len(), 5);
        assert_eq!(of("bottom").len(), 6);
        assert_eq!(of("left").len(), 5);
        assert_eq!(of("right").len(), 6);
        for (i, a) in strips.iter().enumerate() {
            for b in &strips[i + 1..] {
                assert!(a.centre.distance(b.centre) > 1e-5, "a side twice: {a:?}");
            }
        }
        assert!(
            of("top")
                .iter()
                .all(|s| s.normal.y < 0.0 && s.seat == Some(1))
        );
        assert!(
            of("bottom")
                .iter()
                .all(|s| s.normal.y > 0.0 && s.seat == Some(1))
        );
        assert!(
            of("left")
                .iter()
                .all(|s| s.normal.x < 0.0 && s.seat == Some(2))
        );
        assert!(
            of("right")
                .iter()
                .all(|s| s.normal.x > 0.0 && s.seat == Some(2))
        );
        // Outside the cells, just beyond their sides.
        let a1 = layout.centres["a1"];
        let top_of_a1 = strips
            .iter()
            .find(|s| s.region == "top" && s.centre.distance(a1) < PITCH)
            .unwrap();
        let out = top_of_a1.centre.distance(a1);
        assert!(out > PITCH / 2.0 && out < PITCH * 0.7, "{out}");
    }

    #[test]
    fn a_square_board_has_one_strip_per_cell_along_each_edge() {
        let mut info = hex3_with_goals();
        info.board.kind = "grid".into();
        info.board.positions = (0..3)
            .flat_map(|y| {
                (0..3).map(move |x| {
                    let id = format!("{}{}", (b'a' + x as u8) as char, y + 1);
                    pos(&id, x as f32, y as f32)
                })
            })
            .collect();
        let layout = Layout::new(&info.board.positions);
        let strips = goal_strips(&info, &layout, CellShape::of(&info.board));
        for region in ["top", "bottom", "left", "right"] {
            assert_eq!(
                strips.iter().filter(|s| s.region == region).count(),
                3,
                "{region}"
            );
        }
        let top = strips.iter().find(|s| s.region == "top").unwrap();
        assert!((top.normal - Vec2::new(0.0, -1.0)).length() < 1e-4);
    }

    #[test]
    fn goals_this_client_does_not_know_or_shared_by_every_seat() {
        let mut info = hex3_with_goals();
        let layout = Layout::new(&info.board.positions);
        info.goals[0].kind = "surround".into();
        info.goals[1].seat = None;
        let strips = goal_strips(&info, &layout, CellShape::Hexagon);
        assert!(
            strips
                .iter()
                .all(|s| s.region == "left" || s.region == "right")
        );
        assert!(strips.iter().all(|s| s.seat.is_none()));
        info.goals.clear();
        assert!(goal_strips(&info, &layout, CellShape::Hexagon).is_empty());
    }

    #[test]
    fn an_empty_board_lays_out_nothing() {
        assert!(Layout::new(&[]).centres.is_empty());
    }

    #[test]
    fn the_cell_shape_comes_from_the_board_kind() {
        let board = |kind: &str| Board {
            kind: kind.into(),
            positions: vec![],
            regions: Default::default(),
        };
        assert_eq!(CellShape::of(&board("hex")), CellShape::Hexagon);
        assert_eq!(CellShape::of(&board("grid")), CellShape::Square);
        assert_eq!(CellShape::of(&board("explicit")), CellShape::Spot);
        assert_eq!(CellShape::of(&board("something-new")), CellShape::Spot);
    }

    #[test]
    fn a_hexagon_cell_touches_its_neighbours_across_flat_sides() {
        let mesh = CellShape::Hexagon.mesh(1.0, 1.0, 0.1);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(points)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("the mesh has positions");
        };
        let (lo, hi) = points.iter().map(|p| Vec3::from_array(*p)).fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(lo, hi), p| (lo.min(p), hi.max(p)),
        );
        let size = hi - lo;
        assert!(
            (size.x - 1.0).abs() < 1e-4,
            "flat sides one pitch apart: {size}"
        );
        assert!(
            (size.z - 2.0 / 3f32.sqrt()).abs() < 1e-4,
            "points far and near: {size}"
        );
        assert!(lo.y.abs() < 1e-5 && (hi.y - 0.1).abs() < 1e-5);
    }
}
