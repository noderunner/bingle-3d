//! Move tiles: a game played with move cards, each showing where a piece may go, has each card
//! of the view's drawn as a tile of wood beside the board, the grid cut into it and the squares
//! stamped on it in ink, for a game whose look has `[move_tiles]`.
//!
//! The server says what the cards are: the view lists those each seat holds and the one at the
//! side (`MoveTilesLook::view`), each with its id, its name, its moves (from where the piece
//! stands, as its holder sees them: to the right, and forward) and the seat its stamp is. The
//! client draws them from that alone, so no card is named here: the square the piece stands on
//! stamped in one ink, each square it may go to in another, the card's name engraved under the
//! grid and a seal in its stamp's seat's colour beside it. A seat's tiles lie in front of it,
//! reading from its side; the side's lies at the right hand of the seat on impulse, which takes
//! it next. When the cards change hands the tiles slide to their new places.
//!
//! A click on a tile makes the choice the look names, then the card's id (`play::Choice`); the
//! tiles a click would choose glow, and the one chosen is lifted until the move is played.
//!
//! `dealt`, `reach`, `stamps`, `grid` and `places` have no Bevy in them.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;
use serde_json::Value;

use crate::appearance::{Appearance, Appearances, MoveTilesLook};
use crate::assets;
use crate::board::BoardOutline;
use crate::coordinates;
use crate::hand;
use crate::highlight::{Lit, Outline};
use crate::look::{self, MaterialSpec};
use crate::net::WatchStatus;
use crate::net::api::Gesture;
use crate::play::{Choice, Outcome, Stage, Turn};
use crate::table::Table;

pub struct MoveTilesPlugin;

impl Plugin for MoveTilesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BoardOutline>()
            .add_systems(Update, (sync_tiles, slide_tiles).chain());
    }
}

/// The tile's model, and its size (`assets-src/blender/move_tiles.py`).
pub const MODEL: &str = "models/move_tile.glb";
pub const WIDTH: f32 = 0.064;
pub const DEPTH: f32 = 0.070;
/// The stamp the squares are printed with.
const STAMP: &str = "textures/stamp/square_albedo.png";
/// Between the board's edge and the tiles (a gap the eye sees), and between two tiles.
const GAP: f32 = 0.015;
const BETWEEN: f32 = 0.012;
/// The grid: its side, and how far its middle is from the tile's, away from its holder.
const GRID: f32 = 0.040;
const GRID_AT: f32 = -0.009;
/// A stamped square against its cell, and the grid's lines' width against a cell.
const SQUARE: f32 = 0.86;
const LINE: f32 = 0.07;
/// The name's line, towards the holder; a glyph's cell; the seal's side.
const NAME_AT: f32 = 0.0235;
const GLYPH: f32 = 0.0105;
const ADVANCE: f32 = 0.36;
const SEAL: f32 = 0.0075;
/// How far over the tile's top what is printed on it lies, so it is not lost in it.
const PRINT: f32 = 0.0002;

// The grid and the name's line lie on the tile, apart.
const _: () = assert!(GRID / 2.0 - GRID_AT < DEPTH / 2.0 && GRID < WIDTH);
const _: () = assert!(GRID_AT + GRID / 2.0 < NAME_AT - GLYPH / 2.0);
const _: () = assert!(NAME_AT + GLYPH / 2.0 < DEPTH / 2.0);

/// One card, as the view gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveCard {
    pub id: String,
    pub name: String,
    /// Each move: to the right and forward, as its holder sees it.
    pub moves: Vec<[i32; 2]>,
    /// The colour word of the seat its stamp is, if it has one.
    pub stamp: Option<String>,
}

/// The cards in play: those each seat holds, by the seat's place among the game's seats, and
/// the one at the side.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dealt {
    pub hands: Vec<Vec<MoveCard>>,
    pub side: Option<MoveCard>,
}

impl Dealt {
    pub fn all(&self) -> impl Iterator<Item = &MoveCard> {
        self.hands.iter().flatten().chain(self.side.as_ref())
    }
}

fn card(v: &Value) -> Option<MoveCard> {
    let moves = v["moves"]
        .as_array()?
        .iter()
        .filter_map(|m| Some([m[0].as_i64()? as i32, m[1].as_i64()? as i32]))
        .collect();
    Some(MoveCard {
        id: v["id"].as_str()?.to_string(),
        name: v["name"].as_str().unwrap_or_default().to_string(),
        moves,
        stamp: v["stamp"].as_str().map(str::to_string),
    })
}

/// The cards the view lists under `key`, for a game of `seats` seats (numbered from 1): none
/// when it lists none.
pub fn dealt(view: &Value, key: &str, seats: usize) -> Option<Dealt> {
    let cards = view.get(key).filter(|c| c.is_object())?;
    let hands = (1..=seats)
        .map(|seat| {
            cards["hands"][seat.to_string()]
                .as_array()
                .map(|h| h.iter().filter_map(card).collect())
                .unwrap_or_default()
        })
        .collect();
    let dealt = Dealt {
        hands,
        side: card(&cards["side"]),
    };
    let any = dealt.all().next().is_some();
    any.then_some(dealt)
}

/// How far the farthest move of any card goes, in squares (at least one): every tile's grid
/// is as wide, so they read alike.
pub fn reach<'a>(cards: impl Iterator<Item = &'a MoveCard>) -> i32 {
    cards
        .flat_map(|c| c.moves.iter())
        .map(|[r, f]| r.abs().max(f.abs()))
        .max()
        .unwrap_or(1)
        .max(1)
}

/// A cell of a grid that is `reach` squares from its middle each way.
fn cell(reach: i32) -> f32 {
    GRID / (2 * reach + 1) as f32
}

/// What is stamped on a square.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    /// Where the piece stands.
    Piece,
    /// Where it may go.
    Move,
}

/// The squares stamped on a card's tile, at their middles on the tile (x to its holder's right,
/// z towards them): the piece's in the grid's middle, and each it may go to. A move beyond the
/// grid is left out.
pub fn stamps(card: &MoveCard, reach: i32) -> Vec<(Vec2, Ink)> {
    let side = cell(reach);
    let middle = Vec2::new(0.0, GRID_AT);
    std::iter::once((middle, Ink::Piece))
        .chain(
            card.moves
                .iter()
                .filter(|[r, f]| r.abs() <= reach && f.abs() <= reach && (*r, *f) != (0, 0))
                .map(|[r, f]| (middle + Vec2::new(*r as f32, -*f as f32) * side, Ink::Move)),
        )
        .collect()
}

/// The grid's lines on a tile, each from one end to the other (x, z).
pub fn grid(reach: i32) -> Vec<(Vec2, Vec2)> {
    let side = cell(reach);
    let n = 2 * reach + 1;
    let half = GRID / 2.0;
    (0..=n)
        .flat_map(|i| {
            let at = -half + i as f32 * side;
            [
                (Vec2::new(at, GRID_AT - half), Vec2::new(at, GRID_AT + half)),
                (
                    Vec2::new(-half, GRID_AT + at),
                    Vec2::new(half, GRID_AT + at),
                ),
            ]
        })
        .collect()
}

/// A seat's way of facing the board: towards it from its side (`back`, from the board to the
/// seat) and its right hand, for the seat `i` of `seats`, as trays and bowls place them.
fn frame(i: usize, seats: usize) -> (f32, Vec2, Vec2) {
    let turn = TAU * i as f32 / seats.max(1) as f32;
    (
        turn,
        Vec2::new(turn.sin(), turn.cos()),
        Vec2::new(turn.cos(), -turn.sin()),
    )
}

/// Where each tile lies (x, z) and how it is turned about the up axis, reading from its holder:
/// each seat's in a row in front of it, beyond the board's `outline`, and the side's at the
/// right hand of seat `side_to`, which takes it next. In the order of `Dealt::all`.
pub fn places(dealt: &Dealt, outline: &[Vec2], side_to: usize) -> Vec<(Vec2, f32)> {
    let seats = dealt.hands.len();
    let reach = |dir: Vec2| outline.iter().fold(0.0f32, |far, p| far.max(p.dot(dir)));
    let mut out = Vec::new();
    for (i, hand) in dealt.hands.iter().enumerate() {
        let (turn, back, right) = frame(i, seats);
        let front = back * (reach(back) + GAP + DEPTH / 2.0);
        let middle = (hand.len() as f32 - 1.0) / 2.0;
        for k in 0..hand.len() {
            let along = (k as f32 - middle) * (WIDTH + BETWEEN);
            out.push((front + right * along, turn));
        }
    }
    if dealt.side.is_some() {
        let (turn, _, right) = frame(side_to.min(seats.saturating_sub(1)), seats);
        out.push((right * (reach(right) + GAP + WIDTH / 2.0), turn));
    }
    out
}

/// How far the tiles reach from the board's middle: in front of the player (the first seat's
/// row), and beside the board (the side's tile), for the camera to frame them with it.
pub fn reaches(outline: &[Vec2]) -> (f32, f32) {
    let far = |dir: Vec2| outline.iter().fold(0.0f32, |far, p| far.max(p.dot(dir)));
    (far(Vec2::Y) + GAP + DEPTH, far(Vec2::X) + GAP + WIDTH)
}

/// A tile of the watched game.
#[derive(Component, Debug, Clone, PartialEq)]
struct Tile {
    id: String,
    /// Where it lies when nothing lifts it.
    rest: Transform,
    /// Lifted: the card chosen for the move being built.
    lifted: bool,
}

/// The tiles of one game.
#[derive(Component, Debug)]
struct TilesRoot {
    room_code: String,
}

/// The game's look, its look for its tiles, and its cards, if it has them.
fn watched<'a>(
    watch: &WatchStatus,
    all: &'a Appearances,
) -> Option<(&'a Appearance, &'a MoveTilesLook, Dealt)> {
    let table = watch.table.as_ref()?;
    let look = all.for_game(&table.info.id)?;
    let tiles = look.move_tiles.as_ref()?;
    let view = &table.state.state.ext["view"];
    Some((
        look,
        tiles,
        dealt(view, &tiles.view, table.info.seats.len())?,
    ))
}

/// The card chosen in the move being built, by its choice's value.
fn chosen<'a>(turn: &'a Turn, prefix: &str) -> Option<&'a str> {
    turn.made
        .iter()
        .flat_map(|s| s.gestures.iter())
        .chain(&turn.gestures)
        .find(|g| g.kind == "action" && g.value.starts_with(prefix))
        .map(|g| &g.value[prefix.len()..])
}

/// What a click on a card's tile chooses, if a click would be taken now.
fn choice_for(turn: Option<&Turn>, prefix: &str, id: &str) -> Option<Gesture> {
    let turn = turn.filter(|t| matches!(t.stage, Stage::Choosing | Stage::Ready))?;
    let g = Gesture {
        kind: "action".into(),
        value: format!("{prefix}{id}"),
    };
    (turn.clone().gesture(&g) != Outcome::Ignored).then_some(g)
}

#[allow(clippy::too_many_arguments)]
fn sync_tiles(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    appearances: Res<Appearances>,
    edge: Res<BoardOutline>,
    server: Res<AssetServer>,
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
    roots: Query<(Entity, &TilesRoot)>,
    mut tiles: Query<(Entity, &mut Tile, Option<&Choice>, Has<Lit>)>,
) {
    if !(watch.is_changed() || edge.is_changed() || appearances.is_changed()) {
        return;
    }
    let found = watched(&watch, &appearances);
    let room = watch.table.as_ref().map(|t| t.room_code.as_str());
    let mut root = None;
    for (entity, r) in &roots {
        if found.is_some() && Some(r.room_code.as_str()) == room && !edge.0.is_empty() {
            root = Some(entity);
        } else {
            commands.entity(entity).despawn();
        }
    }
    let (Some((look, tiles_look, dealt)), Some(table)) = (found, watch.table.as_ref()) else {
        return;
    };
    // The board is not built yet: the tiles lie beside it, so they wait for it.
    if edge.0.is_empty() {
        return;
    }
    let fresh = root.is_none();
    let root = root.unwrap_or_else(|| {
        commands
            .spawn((
                Name::new("Move tiles"),
                TilesRoot {
                    room_code: table.room_code.clone(),
                },
                Transform::default(),
                Visibility::default(),
            ))
            .id()
    });
    // The side's tile goes to the seat on impulse, which takes it next.
    let side_to = table
        .state
        .impulse_seat
        .and_then(|s| table.info.seats.iter().position(|d| d.seat == s))
        .unwrap_or(0);
    let lie = assets::model_span(MODEL).map_or(0.00225, |(low, _)| -low);
    let turn = watch.turn.as_ref();
    let lifted = turn.and_then(|t| chosen(t, &tiles_look.choice));
    let mut kit = None;
    let mut seen = Vec::new();
    for (card, (at, yaw)) in dealt.all().zip(places(&dealt, &edge.0, side_to)) {
        seen.push(card.id.clone());
        let rest = Transform::from_xyz(at.x, lie, at.y).with_rotation(Quat::from_rotation_y(yaw));
        let up = lifted == Some(card.id.as_str());
        let choice = choice_for(turn, &tiles_look.choice, &card.id);
        // Lit while it may be chosen and nothing is yet.
        let light = choice.is_some() && lifted.is_none();
        if let Some((entity, mut tile, had, lit)) =
            tiles.iter_mut().find(|(_, t, _, _)| t.id == card.id)
        {
            if tile.rest != rest || tile.lifted != up {
                tile.rest = rest;
                tile.lifted = up;
            }
            if had.map(|c| &c.0) != choice.as_ref() {
                match &choice {
                    Some(g) => commands.entity(entity).insert(Choice(g.clone())),
                    None => commands.entity(entity).remove::<Choice>(),
                };
            }
            if light != lit {
                if light {
                    commands.entity(entity).insert(Lit);
                } else {
                    commands.entity(entity).remove::<Lit>();
                }
            }
            continue;
        }
        let kit = kit.get_or_insert_with(|| {
            Kit::new(
                look,
                tiles_look,
                &dealt,
                table,
                &appearances,
                &server,
                &mut meshes,
                &mut materials,
            )
        });
        // A tile lies where it rests when the game is opened; one dealt later comes from the
        // middle of the table, under the board.
        let start = if fresh {
            rest
        } else {
            Transform::from_xyz(0.0, -0.05, 0.0)
        };
        let entity = spawn_tile(&mut commands, root, kit, card, start, lie, &mut meshes);
        commands.entity(entity).insert(Tile {
            id: card.id.clone(),
            rest,
            lifted: up,
        });
        if let Some(g) = choice {
            commands.entity(entity).insert(Choice(g));
        }
        if light {
            commands.entity(entity).insert(Lit);
        }
    }
    for (entity, tile, _, _) in &tiles {
        if !seen.contains(&tile.id) {
            commands.entity(entity).despawn();
        }
    }
}

/// What every tile of a game is made of.
struct Kit {
    model: Handle<Mesh>,
    wood: Handle<StandardMaterial>,
    lines: Handle<StandardMaterial>,
    grid: Handle<Mesh>,
    ink: Handle<StandardMaterial>,
    piece_ink: Handle<StandardMaterial>,
    square: Handle<Mesh>,
    engraving: Handle<StandardMaterial>,
    glyphs: HashMap<usize, Handle<Mesh>>,
    seal: Handle<Mesh>,
    /// The seal's ink in each colour word's colour.
    seals: HashMap<String, Handle<StandardMaterial>>,
    reach: i32,
}

/// An ink: the stamp's cover, in its colour, cut out where the stamp did not reach.
fn ink(spec: MaterialSpec) -> MaterialSpec {
    MaterialSpec {
        albedo: spec.albedo.clone().or_else(|| Some(STAMP.into())),
        cutout: spec.cutout.or(Some(0.5)),
        roughness: spec.roughness.or(Some(0.75)),
        ..spec
    }
}

/// A square lying flat, `side` across, mapped once over (a stamp's, a seal's).
fn flat_square(side: f32) -> Mesh {
    Rectangle::new(side, side)
        .mesh()
        .build()
        .rotated_by(Quat::from_rotation_x(-FRAC_PI_2))
}

/// The grid's lines as strips lying flat, `width` wide, mapped in metres (a wood's).
fn grid_mesh(lines: &[(Vec2, Vec2)], width: f32) -> Mesh {
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    for (a, b) in lines {
        let along = (*b - *a).normalize_or_zero();
        let across = along.perp() * width / 2.0;
        let (a, b) = (*a - along * width / 2.0, *b + along * width / 2.0);
        let base = positions.len() as u32;
        for p in [a - across, b - across, b + across, a + across] {
            positions.push([p.x, 0.0, p.y]);
            uvs.push([p.x, p.y]);
        }
        indices.extend([base, base + 2, base + 1, base, base + 3, base + 2]);
    }
    let n = positions.len();
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n])
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(bevy::mesh::Indices::U32(indices));
    if let Err(e) = mesh.generate_tangents() {
        warn!("a move tile's grid: {e}");
    }
    mesh
}

/// The tiles' pale wood, without a look of its own: maple, oiled and a little warmed.
fn pale_wood() -> MaterialSpec {
    MaterialSpec {
        wood: Some("maple".into()),
        tint: Some([0.93, 0.82, 0.66]),
        clearcoat: Some(0.3),
        clearcoat_roughness: Some(0.3),
        ..default()
    }
}

impl Kit {
    #[allow(clippy::too_many_arguments)]
    fn new(
        look: &Appearance,
        tiles: &MoveTilesLook,
        dealt: &Dealt,
        table: &Table,
        appearances: &Appearances,
        server: &AssetServer,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
    ) -> Self {
        let named = |name: &Option<String>| look.material(name.as_ref()).cloned();
        let wood = named(&tiles.material).unwrap_or_else(pale_wood);
        // The grid is cut into the wood, darkened as the board's lines are.
        let [r, g, b] = wood.tint.unwrap_or([1.0; 3]);
        let lines = MaterialSpec {
            tint: Some([r * 0.30, g * 0.23, b * 0.18]),
            ..wood.clone()
        };
        let ink_of = |name: &Option<String>, tint: [f32; 3]| {
            ink(named(name).unwrap_or(MaterialSpec {
                tint: Some(tint),
                ..default()
            }))
        };
        let mut build = |spec: &MaterialSpec| materials.add(spec.build(server));
        let reach = reach(dealt.all());
        let mut seals = HashMap::new();
        for word in dealt.all().filter_map(|c| c.stamp.clone()) {
            if seals.contains_key(&word) {
                continue;
            }
            let seat = table.info.seats.iter().find(|s| s.color == word);
            let paint = match seat {
                Some(s) => appearances.seat_paint(&table.info.id, &word, s.seat),
                None => look::seat_paint(&word, 0),
            }
            .to_srgba();
            let spec = ink(MaterialSpec {
                tint: Some([paint.red, paint.green, paint.blue]),
                ..default()
            });
            seals.insert(word, build(&spec));
        }
        Kit {
            model: server.load(
                GltfAssetLabel::Primitive {
                    mesh: 0,
                    primitive: 0,
                }
                .from_asset(assets::path(MODEL)),
            ),
            wood: build(&wood),
            lines: build(&lines),
            ink: build(&ink_of(&tiles.ink, [0.66, 0.12, 0.07])),
            piece_ink: build(&ink_of(&tiles.piece_ink, [0.06, 0.05, 0.05])),
            engraving: materials.add(coordinates::engraving(server)),
            grid: meshes.add(grid_mesh(&grid(reach), cell(reach) * LINE)),
            square: meshes.add(flat_square(cell(reach) * SQUARE)),
            seal: meshes.add(flat_square(SEAL)),
            glyphs: HashMap::new(),
            seals,
            reach,
        }
    }
}

/// A card's name as glyphs of the atlas, each with its offset from the name's middle in glyph
/// cells: a space is a glyph's advance with nothing in it, and what the atlas lacks is left out.
pub fn lettering(name: &str) -> Vec<(usize, f32)> {
    let mut at = 0.0;
    let mut out = Vec::new();
    for ch in name.chars() {
        if let Some(g) = coordinates::glyph(ch) {
            out.push((g, at));
            at += ADVANCE;
        } else if ch == ' ' {
            at += ADVANCE;
        }
    }
    let width = out.last().map_or(0.0, |(_, x)| *x);
    out.iter_mut().for_each(|(_, x)| *x -= width / 2.0);
    out
}

/// How turned a stamp is, a quarter at a time, so no two stamps of a tile look alike: the same
/// every time the tile is drawn.
fn quarter_turns(id: &str, index: usize) -> f32 {
    let mut h: u32 = 2166136261;
    for b in id.bytes().chain([index as u8]) {
        h = (h ^ b as u32).wrapping_mul(16777619);
    }
    (h % 4) as f32 * FRAC_PI_2
}

fn spawn_tile(
    commands: &mut Commands,
    root: Entity,
    kit: &mut Kit,
    card: &MoveCard,
    start: Transform,
    top: f32,
    meshes: &mut Assets<Mesh>,
) -> Entity {
    let (hx, hz) = (WIDTH / 2.0, DEPTH / 2.0);
    let corners = vec![
        Vec2::new(hx, hz),
        Vec2::new(hx, -hz),
        Vec2::new(-hx, -hz),
        Vec2::new(-hx, hz),
    ];
    let tile = commands
        .spawn((
            Name::new(format!("Move tile {}", card.id)),
            Mesh3d(kit.model.clone()),
            MeshMaterial3d(kit.wood.clone()),
            start,
            Visibility::default(),
            Pickable::default(),
            Outline::space(corners, top),
            ChildOf(root),
        ))
        .id();
    let print = |n: usize| top + PRINT * (1 + n) as f32;
    commands.spawn((
        Mesh3d(kit.grid.clone()),
        MeshMaterial3d(kit.lines.clone()),
        Transform::from_xyz(0.0, print(0), 0.0),
        ChildOf(tile),
    ));
    for (i, (at, ink)) in stamps(card, kit.reach).into_iter().enumerate() {
        let material = match ink {
            Ink::Piece => kit.piece_ink.clone(),
            Ink::Move => kit.ink.clone(),
        };
        commands.spawn((
            Mesh3d(kit.square.clone()),
            MeshMaterial3d(material),
            Transform::from_xyz(at.x, print(1), at.y)
                .with_rotation(Quat::from_rotation_y(quarter_turns(&card.id, i))),
            ChildOf(tile),
        ));
    }
    // The name, under the grid, and the seal at its right.
    let letters = lettering(&card.name);
    // Its width in glyph cells; smaller than it would be when a long name would reach the seal.
    let span = letters.last().map_or(1.0, |(_, x)| 2.0 * x + 1.0);
    let room = WIDTH - 2.0 * SEAL - 0.012;
    let size = GLYPH.min(room / span);
    let name_at = Vec2::new(-SEAL * 0.8, NAME_AT);
    for (index, offset) in letters {
        let mesh = kit
            .glyphs
            .entry(index)
            .or_insert_with(|| meshes.add(coordinates::glyph_mesh(index, size)))
            .clone();
        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(kit.engraving.clone()),
            Transform::from_xyz(name_at.x + offset * size, print(0), name_at.y),
            ChildOf(tile),
        ));
    }
    if let Some(seal) = card.stamp.as_ref().and_then(|w| kit.seals.get(w)) {
        commands.spawn((
            Mesh3d(kit.seal.clone()),
            MeshMaterial3d(seal.clone()),
            Transform::from_xyz(hx - SEAL - 0.004, print(1), NAME_AT)
                .with_rotation(Quat::from_rotation_y(quarter_turns(&card.id, 99))),
            ChildOf(tile),
        ));
    }
    tile
}

/// How high a tile travelling rises over where it rests, when it has `left` metres still to go
/// across the table: enough to clear the board while it is far, down onto its place as it comes.
pub fn rise(left: f32) -> f32 {
    (left * 0.8).min(RISE)
}

/// The most a travelling tile rises: over the board's rim and its pieces.
const RISE: f32 = 0.09;

/// Moves each tile towards where it rests, over the board rather than through it, lifted and
/// bobbing while it is the card chosen. It rises more quickly than it goes across, so it is
/// clear of the board before it reaches it.
fn slide_tiles(time: Res<Time>, mut tiles: Query<(&Tile, &mut Transform)>) {
    let share = hand::approach(time.delta_secs());
    let quick = hand::approach(time.delta_secs() * 3.0);
    for (tile, mut at) in &mut tiles {
        let to = tile.rest;
        let left = (at.translation.xz() - to.translation.xz()).length();
        let mut height = to.translation.y + rise(left);
        if tile.lifted {
            height += hand::LIFT + hand::bob(time.elapsed_secs());
        }
        let across = at.translation.xz().lerp(to.translation.xz(), share);
        at.translation = Vec3::new(
            across.x,
            at.translation.y + (height - at.translation.y) * quick,
            across.y,
        );
        at.rotation = at.rotation.slerp(to.rotation, share);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> Value {
        serde_json::json!({
            "cards": {
                "hands": {
                    "1": [
                        {"id": "a", "name": "Alpha", "moves": [[0, 2], [-1, 0]], "stamp": "red"},
                        {"id": "b", "name": "Beta", "moves": [[1, 1]], "stamp": "blue"}
                    ],
                    "2": [
                        {"id": "c", "name": "Gamma", "moves": [[-2, 1], [9, 0]], "stamp": "red"},
                        {"id": "d", "name": "Delta", "moves": [[0, -1]], "stamp": "blue"}
                    ]
                },
                "side": {"id": "e", "name": "Epsilon", "moves": [[1, 0]], "stamp": "blue"}
            }
        })
    }

    #[test]
    fn the_cards_are_read_from_the_view_by_the_looks_name_for_them() {
        let dealt = dealt(&view(), "cards", 2).unwrap();
        assert_eq!(dealt.hands.len(), 2);
        assert_eq!(dealt.hands[0][0].moves, [[0, 2], [-1, 0]]);
        assert_eq!(dealt.hands[1][1].stamp.as_deref(), Some("blue"));
        assert_eq!(dealt.side.as_ref().unwrap().name, "Epsilon");
        let ids: Vec<&str> = dealt.all().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "c", "d", "e"]);
        assert!(super::dealt(&view(), "other", 2).is_none());
        assert!(super::dealt(&serde_json::json!({"cards": {}}), "cards", 2).is_none());
    }

    #[test]
    fn each_move_is_stamped_where_its_holder_sees_it() {
        let dealt = dealt(&view(), "cards", 2).unwrap();
        // The farthest move a grid shows is two squares (the one nine away is no square).
        let r = reach(std::iter::once(&dealt.hands[0][0]));
        assert_eq!(r, 2);
        let side = cell(r);
        let squares = stamps(&dealt.hands[0][0], r);
        let middle = Vec2::new(0.0, GRID_AT);
        assert_eq!(squares[0], (middle, Ink::Piece));
        // Two forward: away from the holder, -z.
        assert_eq!(
            squares[1],
            (middle + Vec2::new(0.0, -2.0 * side), Ink::Move)
        );
        // One to the left.
        assert_eq!(squares[2], (middle + Vec2::new(-side, 0.0), Ink::Move));
        let wide = stamps(&dealt.hands[1][0], 2);
        assert_eq!(
            wide.len(),
            2,
            "the move nine to the right is beyond the grid"
        );
        // Every stamp lies within the grid, and the grid within the tile.
        for (at, _) in squares.iter().chain(&wide) {
            assert!(at.x.abs() < GRID / 2.0 && (at.y - GRID_AT).abs() < GRID / 2.0);
        }
        assert_eq!(grid(2).len(), 12, "six lines each way round five squares");
    }

    #[test]
    fn each_seat_has_its_tiles_in_front_of_it_and_the_side_goes_to_the_seat_on_impulse() {
        let dealt = dealt(&view(), "cards", 2).unwrap();
        let half = 0.15;
        let outline = [
            Vec2::new(-half, -half),
            Vec2::new(half, -half),
            Vec2::new(half, half),
            Vec2::new(-half, half),
        ];
        let at = places(&dealt, &outline, 1);
        assert_eq!(at.len(), 5);
        // The first seat's, on its side (+z), side by side, reading from it.
        let front = half + GAP + DEPTH / 2.0;
        assert!((at[0].0.y - front).abs() < 1e-6 && (at[1].0.y - front).abs() < 1e-6);
        assert!(at[0].0.x < 0.0 && at[1].0.x > 0.0, "the first at its left");
        assert!(at[1].0.x - at[0].0.x >= WIDTH);
        assert_eq!(at[0].1, 0.0);
        // The second seat's across the board, turned to it: its first at its own left.
        assert!((at[2].0.y + front).abs() < 1e-6);
        assert!(at[2].0.x > 0.0);
        assert!((at[2].1 - TAU / 2.0).abs() < 1e-6);
        // The side's at the right hand of the second seat (-x), clear of the board, turned to it.
        assert!(at[4].0.x <= -(half + GAP + WIDTH / 2.0) + 1e-6);
        assert!((at[4].1 - TAU / 2.0).abs() < 1e-6);
        // At the first seat's right when it is on impulse.
        assert!(places(&dealt, &outline, 0)[4].0.x > half);
        // The camera frames them all.
        let (front, beside) = reaches(&outline);
        assert!(at.iter().all(|(p, _)| p.y + DEPTH / 2.0 <= front + 1e-6));
        assert!(at.iter().all(|(p, _)| p.x.abs() <= beside + 1e-6));
    }

    /// A seat holding cards a and b: a moves a piece from a1 to a2 or b2, b from a1 to b1.
    fn turn() -> Turn {
        let option = |card: &str, to: &str| {
            serde_json::json!({"notation": format!("{card} a1-{to}"), "action": "move",
                "from": "a1", "to": to, "piece": card,
                "gestures": [{"kind": "action", "value": format!("card-{card}")},
                             {"kind": "pos", "value": "a1"}, {"kind": "pos", "value": to}]})
        };
        let answer = serde_json::json!({"seat": 1, "ply": 0, "made": [], "complete": false,
            "options": [option("a", "a2"), option("a", "b2"), option("b", "b1")]});
        let mut turn = Turn::new(0);
        turn.answered(serde_json::from_value(answer).unwrap());
        turn
    }

    #[test]
    fn a_click_on_a_held_tile_chooses_its_card_and_lifts_it() {
        let mut t = turn();
        assert_eq!(chosen(&t, "card-"), None);
        let a = choice_for(Some(&t), "card-", "a").expect("a may be chosen");
        assert_eq!((a.kind.as_str(), a.value.as_str()), ("action", "card-a"));
        assert!(choice_for(Some(&t), "card-", "b").is_some());
        assert!(
            choice_for(Some(&t), "card-", "e").is_none(),
            "nobody's to choose"
        );
        assert!(choice_for(None, "card-", "a").is_none(), "watching");
        assert_ne!(t.gesture(&a), Outcome::Ignored);
        assert_eq!(chosen(&t, "card-"), Some("a"));
        // The other card may still be chosen instead, until the piece is moved.
        assert!(choice_for(Some(&t), "card-", "b").is_some());
    }

    /// A game of two seats with the view's cards in it, the second seat on impulse.
    fn table(view: Value, impulse: u32) -> Table {
        let info = serde_json::json!({
            "id": "made-up", "name": "A game of move cards",
            "seats": [{"seat": 1, "color": "red", "label": "Red"},
                      {"seat": 2, "color": "blue", "label": "Blue"}],
            "board": {"kind": "grid", "positions": [{"id": "a1", "x": 0, "y": 0}]}
        });
        let state = serde_json::json!({
            "state": {"cells": {}, "ext": {"view": view}},
            "status": "active", "move_seq": 1, "impulse_seat": impulse
        });
        let game = serde_json::json!({"game_def_id": "made-up", "status": "active", "seats": []});
        Table::new(crate::net::session::Opened {
            room_code: "AB12CD".into(),
            info: serde_json::from_value(info).unwrap(),
            game: serde_json::from_value(game).unwrap(),
            state: serde_json::from_value(state).unwrap(),
            bots: vec![],
        })
    }

    fn app() -> App {
        let look = Appearance::parse(
            "games = [\"made-up\"]\n[move_tiles]\nview = \"cards\"\nchoice = \"card-\"\n",
        )
        .unwrap();
        let half = 0.15;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), MoveTilesPlugin))
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Image>()
            .insert_resource(Appearances(vec![look]))
            .insert_resource(BoardOutline(vec![
                Vec2::new(-half, -half),
                Vec2::new(half, -half),
                Vec2::new(half, half),
                Vec2::new(-half, half),
            ]))
            .init_resource::<WatchStatus>();
        app
    }

    fn tiles(app: &mut App) -> Vec<(Entity, String, Transform)> {
        let world = app.world_mut();
        let mut found: Vec<_> = world
            .query::<(Entity, &Tile)>()
            .iter(world)
            .map(|(e, t)| (e, t.id.clone(), t.rest))
            .collect();
        found.sort_by(|a, b| a.1.cmp(&b.1));
        found
    }

    #[test]
    fn the_tiles_follow_the_cards_as_they_change_hands_and_go_with_the_game() {
        let mut app = app();
        app.world_mut().resource_mut::<WatchStatus>().table = Some(table(view(), 2));
        app.update();
        let before = tiles(&mut app);
        let ids: Vec<&str> = before.iter().map(|(_, id, _)| id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "c", "d", "e"]);
        // The side's, e, at the right hand of the second seat, on impulse (-x).
        assert!(before[4].2.translation.x < -0.15);

        // The first seat played a: it goes to the side, and e comes to its hand.
        let mut swapped = view();
        let cards = &mut swapped["cards"];
        let a = cards["hands"]["1"][0].clone();
        cards["hands"]["1"][0] = cards["side"].clone();
        cards["side"] = a;
        app.world_mut().resource_mut::<WatchStatus>().table = Some(table(swapped, 2));
        app.update();
        let after = tiles(&mut app);
        assert_eq!(after.len(), 5);
        assert_eq!(after[0].0, before[0].0, "the same tile, moved");
        assert_eq!(after[0].2, before[4].2, "a lies where e lay");
        assert_eq!(after[4].2, before[0].2, "and e where a lay");

        // Another game: they go.
        app.world_mut().resource_mut::<WatchStatus>().table = None;
        app.update();
        app.update();
        assert!(tiles(&mut app).is_empty());
    }

    #[test]
    fn a_tile_rises_while_it_travels_and_comes_down_onto_its_place() {
        assert_eq!(rise(0.0), 0.0);
        assert!(rise(0.01) < rise(0.05));
        assert_eq!(rise(1.0), RISE);
    }

    #[test]
    fn a_name_is_lettered_about_its_middle_with_its_spaces() {
        let one = lettering("Ox");
        assert_eq!(one.len(), 2);
        assert!((one[0].1 + one[1].1).abs() < 1e-6, "centred");
        let two = lettering("Sea Snake");
        assert_eq!(two.len(), 8, "the space is no glyph");
        // The space keeps a glyph's advance between the words.
        assert!((two[3].1 - two[2].1 - 2.0 * ADVANCE).abs() < 1e-6);
        assert!(lettering("").is_empty());
    }

    #[test]
    fn a_tiles_squares_turn_with_it() {
        // Turned half round for the far seat, a forward move points at the board from there.
        let (turn, _, _) = frame(1, 2);
        let local = Vec3::new(0.0, 0.0, -1.0);
        let world = Quat::from_rotation_y(turn) * local;
        assert!(world.z > 0.99, "{world}");
    }
}
