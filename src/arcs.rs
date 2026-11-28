//! A jump drawn as an arc: one for each hop of the move being built (`Turn::hops`), from where
//! the piece left to where it landed, rising over what stands between and coming down on the
//! landing, in the highlight's light (`highlight::Glow::arc`). They last as long as the move
//! being built does; another seat's moves get none.
//!
//! An arc is two ribbons crossed along the same curve, one lying across it and one standing in
//! it, so it reads from over the board and from low across it alike; its light is added to the
//! picture, so where they cross is only a little brighter. The geometry (`apex`, `point`,
//! `ribbons`) has no systems in it.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::board::{Footing, Layout};
use crate::highlight::{self, Glow, GlowMaterial};
use crate::net::WatchStatus;
use crate::scene::BloomOn;

pub struct ArcsPlugin;

impl Plugin for ArcsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Drawn>()
            .init_resource::<BloomOn>()
            .add_systems(Update, (tune, show_arcs).chain());
    }
}

/// How high an arc rises over the tallest piece, as a share of that piece's height, and further
/// for each metre it spans, so a long jump is a long, low curve, not a hump.
const OVER_TALLEST: f32 = 1.5;
const RISE_PER_SPAN: f32 = 0.3;
/// How far over the board an arc starts and ends, so it is not lost in it.
const END_LIFT: f32 = 0.0005;
/// How wide each ribbon is, as a share of the pitch.
const WIDTH: f32 = 0.14;
/// How many pieces each ribbon is drawn in along its length.
const SEGMENTS: usize = 24;

/// How high over its ends an arc from `a` to `b` rises at its middle, over pieces at most
/// `tallest` high.
pub fn apex(a: Vec3, b: Vec3, tallest: f32) -> f32 {
    tallest * OVER_TALLEST + a.distance(b) * RISE_PER_SPAN
}

/// The point a share `t` of the way along the arc from `a` to `b` rising `height` over them:
/// a parabola, in the vertical plane through both.
pub fn point(a: Vec3, b: Vec3, height: f32, t: f32) -> Vec3 {
    a.lerp(b, t) + Vec3::Y * 4.0 * height * t * (1.0 - t)
}

/// Which way the arc goes at `t`.
fn tangent(a: Vec3, b: Vec3, height: f32, t: f32) -> Vec3 {
    ((b - a) + Vec3::Y * 4.0 * height * (1.0 - 2.0 * t)).normalize_or_zero()
}

/// A row of `highlight::mesh`: a position, its normal and its two texture coordinates.
type Row = (Vec3, Vec3, [f32; 2], [f32; 2]);

/// The two ribbons along the arc, `width` wide: the rows of `highlight::mesh` (a position, its
/// normal, `u` from 0 at `a` to 1 at `b` and `v` 0 to 1 across, and `u` again) and the indices
/// of their triangles. The first lies across the curve, level from side to side; the second
/// stands in its plane.
pub fn ribbons(a: Vec3, b: Vec3, height: f32, width: f32) -> (Vec<Row>, Vec<u32>) {
    let side = Vec3::Y.cross(b - a).normalize_or(Vec3::X);
    let mut rows = Vec::with_capacity(4 * (SEGMENTS + 1));
    for standing in [false, true] {
        for i in 0..=SEGMENTS {
            let t = i as f32 / SEGMENTS as f32;
            let at = point(a, b, height, t);
            let up = side.cross(tangent(a, b, height, t)).normalize_or(Vec3::Y);
            let (across, normal) = if standing { (up, side) } else { (side, up) };
            for v in [0.0, 1.0] {
                rows.push((at + across * (v - 0.5) * width, normal, [t, v], [t, 0.0]));
            }
        }
    }
    let one = highlight::strip(SEGMENTS + 1);
    let second = 2 * (SEGMENTS as u32 + 1);
    let indices = one
        .iter()
        .copied()
        .chain(one.iter().map(|i| i + second))
        .collect();
    (rows, indices)
}

/// What is drawn: the board it is on, the hops it shows, and the material, made on first use.
#[derive(Resource, Default)]
struct Drawn {
    root: Option<Entity>,
    hops: Vec<(String, String)>,
    material: Option<Handle<GlowMaterial>>,
}

/// An arc drawn over the board.
#[derive(Component)]
struct JumpArc;

/// Keeps the arcs' material tuned to whether the camera blooms.
fn tune(
    bloom: Res<BloomOn>,
    mut drawn: ResMut<Drawn>,
    materials: Option<ResMut<Assets<GlowMaterial>>>,
) {
    let Some(mut materials) = materials else {
        return;
    };
    match &drawn.material {
        None => {
            drawn.material = Some(materials.add(GlowMaterial {
                glow: Glow::arc(bloom.0),
            }))
        }
        Some(handle) if bloom.is_changed() => {
            if let Some(mut m) = materials.get_mut(handle) {
                m.glow = Glow::arc(bloom.0);
            }
        }
        Some(_) => {}
    }
}

/// Draws an arc for each hop of the move being built, over the board, when they change.
fn show_arcs(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    mut drawn: ResMut<Drawn>,
    roots: Query<(Entity, &Footing)>,
    arcs: Query<Entity, With<JumpArc>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let board = roots.single().ok();
    let hops: Vec<(String, String)> = watch
        .turn
        .as_ref()
        .filter(|_| board.is_some())
        .map(|t| t.hops())
        .unwrap_or_default()
        .into_iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
    let root = board.map(|(e, _)| e);
    if drawn.root == root && drawn.hops == hops {
        return;
    }
    for arc in &arcs {
        commands.entity(arc).despawn();
    }
    drawn.root = root;
    drawn.hops = hops;
    let (Some((root, footing)), Some(table), Some(material)) =
        (board, &watch.table, drawn.material.clone())
    else {
        return;
    };
    if drawn.hops.is_empty() {
        return;
    }
    let layout = Layout::of(&table.info.board);
    let y = Vec3::Y * (footing.surface + END_LIFT);
    for (from, to) in &drawn.hops {
        let (Some(a), Some(b)) = (layout.centres.get(from), layout.centres.get(to)) else {
            continue;
        };
        let (a, b) = (*a + y, *b + y);
        let (rows, indices) = ribbons(a, b, apex(a, b, footing.tallest), footing.pitch * WIDTH);
        commands.spawn((
            Name::new(format!("Arc {from} to {to}")),
            JumpArc,
            Mesh3d(meshes.add(highlight::mesh(rows, indices))),
            MeshMaterial3d(material.clone()),
            Transform::default(),
            NotShadowCaster,
            Pickable::IGNORE,
            ChildOf(root),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PITCH: f32 = 0.028;
    /// A draughtsman's height, about.
    const TALL: f32 = 0.007;

    #[test]
    fn an_arc_starts_and_ends_on_its_spaces_and_clears_the_piece_jumped() {
        let (a, b) = (Vec3::ZERO, Vec3::new(2.0 * PITCH, 0.0, 2.0 * PITCH));
        let h = apex(a, b, TALL);
        assert!(point(a, b, h, 0.0).distance(a) < 1e-6);
        assert!(point(a, b, h, 1.0).distance(b) < 1e-6);
        let middle = point(a, b, h, 0.5);
        assert!(
            middle.y > TALL * 1.4,
            "over the piece between: {}",
            middle.y
        );
        assert!(middle.y < 4.0 * PITCH, "not a tower: {}", middle.y);
        assert!((Vec2::new(middle.x, middle.z) - Vec2::splat(PITCH)).length() < 1e-6);
    }

    #[test]
    fn a_long_jump_still_clears_a_piece_one_space_short_of_its_landing() {
        // A flying king's jump over seven diagonals, the piece jumped next to where it lands.
        let (a, b) = (Vec3::ZERO, Vec3::new(7.0 * PITCH, 0.0, 7.0 * PITCH));
        let h = apex(a, b, TALL);
        let over = point(a, b, h, 6.0 / 7.0);
        assert!(over.y > TALL, "{}", over.y);
    }

    #[test]
    fn the_ribbons_follow_the_curve_one_lying_and_one_standing() {
        let (a, b) = (Vec3::ZERO, Vec3::new(0.0, 0.0, 2.0 * PITCH));
        let h = apex(a, b, TALL);
        let width = PITCH * WIDTH;
        let (rows, indices) = ribbons(a, b, h, width);
        assert_eq!(rows.len(), 4 * (SEGMENTS + 1));
        assert_eq!(indices.len(), 2 * 6 * SEGMENTS);
        assert!(indices.iter().all(|i| (*i as usize) < rows.len()));
        let half = rows.len() / 2;
        for (k, (p, _, [u, v], _)) in rows.iter().enumerate() {
            let on = point(a, b, h, *u);
            assert!(
                (p.distance(on) - width / 2.0).abs() < 1e-6,
                "half its width off"
            );
            assert!(*v == 0.0 || *v == 1.0);
            if k < half {
                // Lying: level from side to side, across the jump.
                assert!((p.y - on.y).abs() < 1e-6 && p.z == on.z);
            } else {
                // Standing: in the plane of the jump.
                assert!(p.x.abs() < 1e-6);
            }
        }
        // `u` runs from where the piece left to where it landed.
        assert_eq!(rows[0].2[0], 0.0);
        assert_eq!(rows[half - 1].2[0], 1.0);
    }
}
