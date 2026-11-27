//! How a position a click would be taken on is shown lit. The board says which positions are lit
//! (`Lit`) and what each one's outline is (`Outline`); this module alone decides what that looks
//! like, so another effect can take the place of this one without the board knowing.
//!
//! The effect today (`Effect::Glow`) traces the outline with a soft line of light lying on the
//! position's top (the ring), and thin streaks of light rising a little from that edge (the
//! ribbon), both slowly moving: a shader of our own (`highlight/cell_glow.wesl`), its light added
//! to what is behind it. A point (a Go board's crossing) gets a ring so small it reads as a dot.
//! The glow is tuned twice: a little brighter than white where the camera blooms, so its core
//! blooms, and softer and wider where it does not (a headset's eyes, or `--no-bloom`:
//! `scene::BloomOn`).
//!
//! A lit position has a tone (`Tone`), which the board says: plain, or one where a click takes
//! a piece, whose glow is red.
//!
//! The outline's geometry (`walk`, `ring`, `ribbon`) has no systems in it.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::asset::io::embedded::EmbeddedAssetRegistry;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

use crate::scene::BloomOn;

pub struct HighlightPlugin;

/// Where the glow's shader is loaded from: compiled in, as every file the scene uses is.
const SHADER: &str = "shaders/cell_glow.wesl";

impl Plugin for HighlightPlugin {
    fn build(&self, app: &mut App) {
        app.world_mut()
            .resource_mut::<EmbeddedAssetRegistry>()
            .insert_asset(
                std::path::PathBuf::new(),
                std::path::Path::new(SHADER),
                include_bytes!("highlight/cell_glow.wesl").as_slice(),
            );
        app.add_plugins(MaterialPlugin::<GlowMaterial>::default())
            .add_plugins(HighlightSystems);
    }
}

/// The systems alone, without the shader and its material's renderer: what a test with no
/// window adds.
pub struct HighlightSystems;

impl Plugin for HighlightSystems {
    fn build(&self, app: &mut App) {
        app.init_resource::<HighlightStyle>()
            .init_resource::<BloomOn>()
            .init_resource::<Effects>()
            .add_systems(
                PostUpdate,
                (tune, unlight, light)
                    .chain()
                    .before(TransformSystems::Propagate),
            );
    }
}

/// How lit positions are shown. When it changes, what is lit is drawn again in the new style.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HighlightStyle {
    pub effect: Effect,
}

/// What is drawn on a lit position. Another effect is another variant here, and its own arm in
/// `light`: nothing outside this module changes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Effect {
    /// The traced edge and the streaks rising from it.
    #[default]
    Glow,
}

/// A position a click would be taken on, which the board marks while it is one.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lit;

/// What a click on a lit position does, as its light shows it. Without one, plain.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Tone {
    /// Anything: candlelight.
    #[default]
    Plain,
    /// Takes the piece standing there (Fist's pin): red.
    Take,
}

/// Each tone, in the order of `Effects::glow`.
const TONES: [Tone; 2] = [Tone::Plain, Tone::Take];

/// A position's outline as its highlight traces it, in its entity's own space: the corners round
/// its middle (x, z), the height of its top, and whether it is a space or a point.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Outline {
    pub corners: Vec<Vec2>,
    pub top: f32,
    pub trace: Trace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Trace {
    /// A cell, or a tile laid on one: its edge is traced.
    Space,
    /// A point where lines cross: a ring so small it reads as a dot.
    Point,
}

/// A point's ring: how far out from the crossing, in pitches.
const POINT_RADIUS: f32 = 0.075;
/// How wide the ring is, as a share of the space's inradius (or all but the middle of a point's).
const SPACE_BAND: f32 = 0.35;
const POINT_BAND: f32 = 0.85;
/// How high the streaks rise, as a share of the space's inradius (or of a point's radius): not
/// far, just enough to stand out of the board.
const SPACE_RISE: f32 = 0.6;
const POINT_RISE: f32 = 1.6;
/// About how far apart the streaks are round an outline, in metres; a point has a few.
const STREAK_SPACING: f32 = 0.006;
const POINT_STREAKS: u32 = 4;
/// How far over the top the ring lies, so it is not lost in it.
const RING_LIFT: f32 = 0.00015;

impl Outline {
    pub fn space(corners: Vec<Vec2>, top: f32) -> Self {
        Outline {
            corners,
            top,
            trace: Trace::Space,
        }
    }

    /// A point's outline, on a board whose points are `pitch` apart.
    pub fn point(pitch: f32, top: f32) -> Self {
        let r = pitch * POINT_RADIUS;
        Outline {
            corners: (0..20)
                .map(|i| Vec2::from_angle(-(i as f32) * std::f32::consts::TAU / 20.0) * r)
                .collect(),
            top,
            trace: Trace::Point,
        }
    }

    /// How far its edge is from its middle, at the nearest.
    pub fn inradius(&self) -> f32 {
        let n = self.corners.len();
        (0..n)
            .map(|i| {
                let (a, b) = (self.corners[i], self.corners[(i + 1) % n]);
                let along = (b - a).normalize_or_zero();
                (a - along * a.dot(along)).length()
            })
            .fold(f32::INFINITY, f32::min)
    }

    /// How wide its ring is and how high its streaks rise, in its own units.
    fn band_and_rise(&self) -> (f32, f32) {
        let r = self.inradius();
        match self.trace {
            Trace::Space => (r * SPACE_BAND, r * SPACE_RISE),
            Trace::Point => (r * POINT_BAND, r * POINT_RISE),
        }
    }

    /// How many streaks rise round it.
    pub fn streaks(&self) -> u32 {
        match self.trace {
            Trace::Space => ((perimeter(&self.corners) / STREAK_SPACING).round() as u32).max(6),
            Trace::Point => POINT_STREAKS,
        }
    }

    /// The same outline, by value, for a mesh's cache: corners in micrometres.
    fn key(&self) -> OutlineKey {
        let um = |v: f32| (v * 1e6).round() as i32;
        (
            self.corners.iter().map(|c| [um(c.x), um(c.y)]).collect(),
            self.trace,
        )
    }
}

/// An outline by value: its corners in micrometres, and what it traces.
type OutlineKey = (Vec<[i32; 2]>, Trace);

fn perimeter(corners: &[Vec2]) -> f32 {
    let n = corners.len();
    (0..n)
        .map(|i| corners[i].distance(corners[(i + 1) % n]))
        .sum()
}

/// The walk round an outline: each corner and the first again at the end, with how far along it
/// is in streaks' places (`u`, 0 to `streaks`) and as a share of the way round (0 to 1).
pub fn walk(corners: &[Vec2], streaks: u32) -> Vec<(Vec2, f32, f32)> {
    let total = perimeter(corners).max(f32::EPSILON);
    let mut out = Vec::with_capacity(corners.len() + 1);
    let mut gone = 0.0;
    for i in 0..=corners.len() {
        let c = corners[i % corners.len()];
        if i > 0 {
            gone += corners[i - 1].distance(c);
        }
        let share = if i == corners.len() {
            1.0
        } else {
            gone / total
        };
        out.push((c, share * streaks as f32, share));
    }
    out
}

/// A mesh from its rows: each a position, its normal and its two texture coordinates.
fn mesh(rows: Vec<(Vec3, Vec3, [f32; 2], [f32; 2])>, indices: Vec<u32>) -> Mesh {
    let mut m = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    m.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        rows.iter().map(|r| r.0.to_array()).collect::<Vec<_>>(),
    );
    m.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        rows.iter().map(|r| r.1.to_array()).collect::<Vec<_>>(),
    );
    m.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        rows.iter().map(|r| r.2).collect::<Vec<_>>(),
    );
    m.insert_attribute(
        Mesh::ATTRIBUTE_UV_1,
        rows.iter().map(|r| r.3).collect::<Vec<_>>(),
    );
    m.insert_indices(Indices::U32(indices));
    m
}

/// Two vertices for each step of the walk, joined into a strip of quads.
fn strip(n: usize) -> Vec<u32> {
    (0..n as u32 - 1)
        .flat_map(|i| {
            let (a, b, c, d) = (2 * i, 2 * i + 1, 2 * i + 2, 2 * i + 3);
            [a, c, b, b, c, d]
        })
        .collect()
}

/// The ring: a band lying on the top just inside the edge, its `v` 0 at the edge and 1 at its
/// inner side `band` in. The corners are drawn in towards the middle, so for a regular outline
/// the band is as wide all round.
pub fn ring(outline: &Outline) -> Mesh {
    let (band, _) = outline.band_and_rise();
    let inner = 1.0 - band / outline.inradius().max(f32::EPSILON);
    let y = outline.top + RING_LIFT;
    let walked = walk(&outline.corners, outline.streaks());
    let n = walked.len();
    let rows = walked
        .into_iter()
        .flat_map(|(c, u, share)| {
            [
                (Vec3::new(c.x, y, c.y), Vec3::Y, [u, 0.0], [share, 0.0]),
                (
                    Vec3::new(c.x * inner, y, c.y * inner),
                    Vec3::Y,
                    [u, 1.0],
                    [share, 0.0],
                ),
            ]
        })
        .collect();
    mesh(rows, strip(n))
}

/// The ribbon: a wall standing on the edge, `v` 0 at its foot and 1 at its top, the streaks'
/// height over it. Seen from both sides.
pub fn ribbon(outline: &Outline) -> Mesh {
    let (_, rise) = outline.band_and_rise();
    let walked = walk(&outline.corners, outline.streaks());
    let n = walked.len();
    let rows = walked
        .into_iter()
        .flat_map(|(c, u, share)| {
            let out = Vec3::new(c.x, 0.0, c.y).normalize_or_zero();
            [
                (
                    Vec3::new(c.x, outline.top, c.y),
                    out,
                    [u, 0.0],
                    [share, 0.0],
                ),
                (
                    Vec3::new(c.x, outline.top + rise, c.y),
                    out,
                    [u, 1.0],
                    [share, 1.0],
                ),
            ]
        })
        .collect();
    mesh(rows, strip(n))
}

/// The glow's material: one for the ring and one for the ribbon, tuned for bloom or without it.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct GlowMaterial {
    #[uniform(0)]
    pub glow: Glow,
}

/// What the shader is told (`cell_glow.wesl` says what each part means).
#[derive(ShaderType, Debug, Clone, Copy, PartialEq)]
pub struct Glow {
    pub color: Vec4,
    pub ring: Vec4,
    pub ring_motion: Vec4,
    pub ribbon: Vec4,
    pub part: Vec4,
}

/// The light's color, linear: a warm candlelight, and for a piece a click takes, red, a little
/// stronger, as red alone is darker to the eye.
const GLOW_COLOR: Vec4 = Vec4::new(1.0, 0.55, 0.16, 1.0);
const TAKE_COLOR: Vec4 = Vec4::new(1.25, 0.1, 0.06, 1.0);

impl Glow {
    /// The ring's or the ribbon's, of a space or a point, where the camera blooms or does not.
    /// With bloom, the line is a little brighter than white so its core blooms (the threshold is
    /// 1.2: `scene.rs`); brighter still, the tonemapping washes it out to white. Without, the
    /// line is no brighter than the picture can show, and a wider halo of its own stands in for
    /// the bloom. A space's line lies along its edge; a point's is drawn in and spread over its
    /// little ring, so the ring fills to a soft dot.
    pub fn tuned(ribbon: bool, trace: Trace, bloom: bool, tone: Tone) -> Self {
        let (line, halo, reach, streaks, foot) = if bloom {
            (1.6, 0.6, 0.35, 2.0, 0.9)
        } else {
            (1.15, 0.9, 0.45, 1.6, 0.9)
        };
        let (at, width) = match trace {
            // A little in from the edge, a tenth of the band wide.
            Trace::Space => (0.08, 0.1),
            Trace::Point => (0.5, 0.45),
        };
        Glow {
            color: match tone {
                Tone::Plain => GLOW_COLOR,
                Tone::Take => TAKE_COLOR,
            },
            ring: Vec4::new(line, at, width, halo),
            // Two brighter stretches go round every 5 s; the whole breathes every 3.
            ring_motion: Vec4::new(reach, 0.3, 0.2, 1.0 / 3.0),
            // A streak rises in a little under 2 s, a quarter of its place wide.
            ribbon: Vec4::new(streaks, foot, 0.55, 0.12),
            part: Vec4::new(if ribbon { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0),
        }
    }
}

impl Material for GlowMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Path(crate::assets::path(SHADER).into())
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // The ribbon is seen from inside across the space as well as from outside.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// What the effects are drawn with: the glow's materials (for each tone, a space's ring and
/// ribbon, then a point's), made on first use, and the meshes made for each outline so far.
#[derive(Resource, Default)]
struct Effects {
    glow: Option<[[[Handle<GlowMaterial>; 2]; 2]; 2]>,
    meshes: HashMap<OutlineKey, [Handle<Mesh>; 2]>,
}

/// Each kind of outline, in the order of `Effects::glow`.
const TRACES: [Trace; 2] = [Trace::Space, Trace::Point];

/// What an effect has drawn on a lit position.
#[derive(Component)]
struct EffectPart;

/// Keeps the glow's materials tuned to whether the camera blooms.
fn tune(
    bloom: Res<BloomOn>,
    mut effects: ResMut<Effects>,
    mut materials: Option<ResMut<Assets<GlowMaterial>>>,
) {
    let Some(materials) = materials.as_mut() else {
        return;
    };
    let fresh = effects.glow.is_none();
    if !fresh && !bloom.is_changed() {
        return;
    }
    let made = effects.glow.get_or_insert_with(|| {
        TONES.map(|tone| {
            TRACES.map(|trace| {
                [false, true].map(|ribbon| {
                    materials.add(GlowMaterial {
                        glow: Glow::tuned(ribbon, trace, bloom.0, tone),
                    })
                })
            })
        })
    });
    if !fresh {
        for (tone, traces) in TONES.iter().zip(made.iter()) {
            for (trace, pair) in TRACES.iter().zip(traces) {
                for (ribbon, handle) in [false, true].into_iter().zip(pair) {
                    if let Some(mut m) = materials.get_mut(handle) {
                        m.glow = Glow::tuned(ribbon, *trace, bloom.0, *tone);
                    }
                }
            }
        }
    }
}

/// Takes the effect off positions no longer lit, and off every one when the style changes.
fn unlight(
    mut commands: Commands,
    style: Res<HighlightStyle>,
    mut gone: RemovedComponents<Lit>,
    lit: Query<Entity, With<Lit>>,
    children: Query<&Children>,
    parts: Query<(), With<EffectPart>>,
) {
    let mut clear = |entity: Entity| {
        for child in children.get(entity).into_iter().flatten() {
            if parts.contains(*child) {
                commands.entity(*child).despawn();
            }
        }
    };
    for entity in gone.read() {
        clear(entity);
    }
    if style.is_changed() && !style.is_added() {
        for entity in &lit {
            clear(entity);
        }
    }
}

/// A lit position as `light` sees it: its outline, its light, its tone, and what is drawn on
/// it.
type Lighting = (
    Entity,
    &'static Outline,
    Ref<'static, Lit>,
    Option<Ref<'static, Tone>>,
    Option<&'static Children>,
);

/// Puts the effect on positions newly lit, on every lit one when the style changes, and again
/// on one whose tone changes.
fn light(
    mut commands: Commands,
    style: Res<HighlightStyle>,
    mut effects: ResMut<Effects>,
    mut meshes: ResMut<Assets<Mesh>>,
    added: Query<Lighting>,
    parts: Query<(), With<EffectPart>>,
) {
    let Some(glow) = effects.glow.clone() else {
        return;
    };
    for (entity, outline, lit, tone, children) in &added {
        let fresh = lit.is_added() || style.is_changed();
        let toned = tone.as_ref().is_some_and(|t| t.is_changed());
        if !fresh && !toned {
            continue;
        }
        // Lit already, in another tone: what was drawn goes first (`unlight` took it off
        // otherwise).
        if !fresh {
            for child in children.into_iter().flatten() {
                if parts.contains(*child) {
                    commands.entity(*child).despawn();
                }
            }
        }
        let tone = tone.map_or(Tone::Plain, |t| *t);
        let glow = &glow[TONES.iter().position(|t| *t == tone).unwrap_or(0)];
        match style.effect {
            Effect::Glow => {
                let made = effects
                    .meshes
                    .entry(outline.key())
                    .or_insert_with(|| [meshes.add(ring(outline)), meshes.add(ribbon(outline))]);
                let pair = &glow[TRACES.iter().position(|t| *t == outline.trace).unwrap_or(0)];
                for (mesh, material) in made.iter().zip(pair) {
                    commands.spawn((
                        Name::new("Glow"),
                        EffectPart,
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::default(),
                        NotShadowCaster,
                        ChildOf(entity),
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    fn square(h: f32) -> Vec<Vec2> {
        vec![
            Vec2::new(-h, -h),
            Vec2::new(-h, h),
            Vec2::new(h, h),
            Vec2::new(h, -h),
        ]
    }

    #[test]
    fn the_walk_goes_round_once_and_ends_where_it_began() {
        let w = walk(&square(0.01), 8);
        assert_eq!(w.len(), 5);
        assert_eq!(w[0].0, w[4].0);
        assert_eq!((w[0].1, w[0].2), (0.0, 0.0));
        assert_eq!((w[4].1, w[4].2), (8.0, 1.0));
        // Each side a quarter of the way, two streaks' places.
        assert!((w[1].1 - 2.0).abs() < 1e-5 && (w[2].2 - 0.5).abs() < 1e-5);
    }

    #[test]
    fn a_space_is_traced_at_its_edge_and_a_point_is_almost_a_dot() {
        let space = Outline::space(square(0.0134), 0.0003);
        assert!((space.inradius() - 0.0134).abs() < 1e-6);
        let (band, rise) = space.band_and_rise();
        assert!(
            band < 0.006,
            "a band near the edge, not over the cell: {band}"
        );
        assert!(rise > 0.005 && rise < 0.012, "not too tall: {rise}");
        assert!(space.streaks() >= 6);

        let point = Outline::point(0.028, 0.03);
        let r = point.inradius();
        assert!(r < 0.003, "a few millimetres across: {r}");
        let (band, _) = point.band_and_rise();
        assert!(band / r > 0.8, "so little hole it reads as a dot");
    }

    #[test]
    fn the_ring_lies_on_the_top_and_the_ribbon_stands_on_the_edge() {
        let outline = Outline::space(square(0.01), 0.002);
        let positions = |m: &Mesh| match m.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(VertexAttributeValues::Float32x3(p)) => p.clone(),
            _ => panic!("positions"),
        };
        let uvs = |m: &Mesh| match m.attribute(Mesh::ATTRIBUTE_UV_0) {
            Some(VertexAttributeValues::Float32x2(p)) => p.clone(),
            _ => panic!("uvs"),
        };
        let r = ring(&outline);
        let p = positions(&r);
        assert_eq!(p.len(), 10);
        assert!(p.iter().all(|v| (v[1] - 0.002 - RING_LIFT).abs() < 1e-7));
        // Outer vertices on the corners, inner ones drawn in.
        assert_eq!([p[0][0], p[0][2]], [-0.01, -0.01]);
        assert!(p[1][0] > -0.01 && p[1][0] < 0.0);
        assert_eq!(uvs(&r)[1][1], 1.0);

        let w = ribbon(&outline);
        let p = positions(&w);
        assert!(p.iter().all(|v| v[0].abs() == 0.01 && v[2].abs() == 0.01));
        assert_eq!(p[0][1], 0.002);
        assert!(p[1][1] > 0.002);
        assert_eq!(w.indices().unwrap().len(), 4 * 6);
    }

    #[test]
    fn bloom_tunes_the_line_brighter_than_white_and_without_it_no_brighter() {
        let plain = Tone::Plain;
        assert!(Glow::tuned(false, Trace::Space, true, plain).ring.x > 1.2);
        assert!(Glow::tuned(false, Trace::Space, false, plain).ring.x <= 1.2);
        assert_eq!(Glow::tuned(true, Trace::Space, false, plain).part.x, 1.0);
        // A point's line is drawn in from its edge, to fill its ring.
        assert!(Glow::tuned(false, Trace::Point, true, plain).ring.y > 0.3);
        // Where a click takes a piece, red.
        let red = Glow::tuned(false, Trace::Space, true, Tone::Take).color;
        assert!(red.x > 4.0 * red.y && red.x > 4.0 * red.z, "{red}");
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<GlowMaterial>()
            .add_plugins(HighlightSystems);
        app
    }

    fn parts(app: &mut App) -> usize {
        let mut q = app.world_mut().query_filtered::<(), With<EffectPart>>();
        q.iter(app.world()).count()
    }

    #[test]
    fn a_lit_position_glows_until_it_is_lit_no_more() {
        let mut app = app();
        let cell = app
            .world_mut()
            .spawn((Outline::space(square(0.01), 0.0), Transform::default()))
            .id();
        app.update();
        assert_eq!(parts(&mut app), 0);
        app.world_mut().entity_mut(cell).insert(Lit);
        app.update();
        assert_eq!(parts(&mut app), 2, "the ring and the ribbon");
        app.update();
        assert_eq!(parts(&mut app), 2, "drawn once");
        app.world_mut().entity_mut(cell).remove::<Lit>();
        app.update();
        assert_eq!(parts(&mut app), 0);
    }

    /// The color of each glow drawn.
    fn colors(app: &mut App) -> Vec<Vec4> {
        let mut q = app
            .world_mut()
            .query_filtered::<&MeshMaterial3d<GlowMaterial>, With<EffectPart>>();
        let handles: Vec<_> = q.iter(app.world()).map(|m| m.0.clone()).collect();
        let materials = app.world().resource::<Assets<GlowMaterial>>();
        handles
            .iter()
            .map(|h| materials.get(h).unwrap().glow.color)
            .collect()
    }

    #[test]
    fn a_position_where_a_click_takes_a_piece_glows_red_and_changes_with_its_tone() {
        let mut app = app();
        let cell = app
            .world_mut()
            .spawn((Outline::space(square(0.01), 0.0), Transform::default()))
            .id();
        app.update();
        app.world_mut().entity_mut(cell).insert((Lit, Tone::Take));
        app.update();
        assert_eq!(colors(&mut app), [TAKE_COLOR; 2]);
        // Plain now, still lit: drawn again, once, in candlelight.
        app.world_mut().entity_mut(cell).insert(Tone::Plain);
        app.update();
        assert_eq!(colors(&mut app), [GLOW_COLOR; 2]);
        app.world_mut().entity_mut(cell).remove::<(Lit, Tone)>();
        app.update();
        assert_eq!(parts(&mut app), 0);
    }

    #[test]
    fn the_glow_is_tuned_again_when_bloom_is_turned_off() {
        let mut app = app();
        app.update();
        let handle = app.world().resource::<Effects>().glow.clone().unwrap()[0][0][0].clone();
        let line = |app: &App| {
            app.world()
                .resource::<Assets<GlowMaterial>>()
                .get(&handle)
                .unwrap()
                .glow
                .ring
                .x
        };
        assert!(line(&app) > 1.2);
        app.world_mut().resource_mut::<BloomOn>().0 = false;
        app.update();
        assert!(line(&app) <= 1.2);
    }
}
