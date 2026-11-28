//! A portal that opens on a flat surface and closes again, and a piece's passage from one portal
//! to another: it sinks into the first and rises out of the second.
//!
//! A `Portal` is given a place (its entity's `Transform`, on the surface), a size and shape (an
//! outline in its own space: a board gives its position's) and a time (when it starts opening,
//! and how long it then stands open); this module draws it, opening from its middle out to its
//! outline and closing back, and takes it away once it has closed. It knows nothing of boards.
//! It is drawn by a shader of our own (`portal/portal.wesl`): the surface inside goes dark, as
//! an opening into the dark would, a bright line is traced round just inside the outline, and
//! arms of light turn inside. Tuned twice, as the highlight's glow is: a little brighter than
//! white where the camera blooms, softer and wider where it does not (`scene::BloomOn`).
//!
//! Nothing is cut out of a piece going in: the surface it sinks into is opaque, so what is under
//! it is not drawn, cut at that plane by the depth test alone, and the pieces keep their own
//! materials. Once wholly under, the piece is hidden until it rises.
//!
//! The geometry (`mesh`), how open a portal is (`Portal::openness`) and a passage's timing
//! (`Passage`) have no systems in them.

use bevy::asset::io::embedded::EmbeddedAssetRegistry;
use bevy::light::NotShadowCaster;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

use crate::highlight;
use crate::scene::BloomOn;

pub struct PortalPlugin;

/// Where the portal's shader is loaded from: compiled in, as every file the scene uses is.
const SHADER: &str = "shaders/portal.wesl";

impl Plugin for PortalPlugin {
    fn build(&self, app: &mut App) {
        app.world_mut()
            .resource_mut::<EmbeddedAssetRegistry>()
            .insert_asset(
                std::path::PathBuf::new(),
                std::path::Path::new(SHADER),
                include_bytes!("portal/portal.wesl").as_slice(),
            );
        app.add_plugins(MaterialPlugin::<PortalMaterial>::default())
            .add_plugins(PortalSystems);
    }
}

/// The systems alone, without the shader and its material's renderer: what a test with no
/// window adds.
pub struct PortalSystems;

impl Plugin for PortalSystems {
    fn build(&self, app: &mut App) {
        app.init_resource::<BloomOn>()
            .init_resource::<Drawn>()
            .add_systems(
                PostUpdate,
                (tune, dress, open)
                    .chain()
                    .before(TransformSystems::Propagate),
            );
    }
}

/// How long a portal takes to open, from nothing to its outline, and to close again.
pub const OPENING: f32 = 0.25;
pub const CLOSING: f32 = 0.3;

/// How far beyond its outline a portal's halo reaches, as a share of the way out to it.
const HALO: f32 = 1.3;
/// How far over the surface it lies, so it is not lost in it.
pub const LIFT: f32 = 0.0002;

/// A portal on a surface: its outline round its middle (x, z) in its entity's own space, when
/// it starts opening (`opens`, in seconds from when it was made) and how long it then stands
/// fully open before it closes (`stays`). Its entity's `Transform` places it; this module sets
/// only its scale, and despawns it once closed.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Portal {
    pub corners: Vec<Vec2>,
    pub opens: f32,
    pub stays: f32,
    /// Seconds since it was made.
    pub secs: f32,
}

impl Portal {
    pub fn new(corners: Vec<Vec2>, opens: f32, stays: f32) -> Self {
        Portal {
            corners,
            opens,
            stays,
            secs: 0.0,
        }
    }

    /// How far open it is at `secs`: 0 shut, 1 out to its outline, opening fast and slowing as
    /// it reaches it, and closing the other way; None once it has closed.
    pub fn openness(&self, secs: f32) -> Option<f32> {
        let s = secs - self.opens;
        let closing = OPENING + self.stays;
        if s <= 0.0 {
            Some(0.0)
        } else if s < OPENING {
            let f = s / OPENING;
            Some(1.0 - (1.0 - f).powi(3))
        } else if s < closing {
            Some(1.0)
        } else if s < closing + CLOSING {
            let f = (s - closing) / CLOSING;
            Some(1.0 - f * f * (3.0 - 2.0 * f))
        } else {
            None
        }
    }
}

/// How far a portal's edge is from its middle, at the nearest.
fn inradius(corners: &[Vec2]) -> f32 {
    highlight::Outline::space(corners.to_vec(), 0.0).inradius()
}

/// A portal's mesh, lying on y = 0: a fan from its middle out to its outline, and a band beyond
/// it for the halo, `HALO` of the way out. Its first texture coordinates' `y` is 0 at the
/// middle, 1 on the outline and `HALO` at the band's end, and `x` the share of the way round;
/// its second, where it is as a share of the inradius.
pub fn mesh(corners: &[Vec2]) -> Mesh {
    let r = inradius(corners).max(f32::EPSILON);
    let walked = highlight::walk(corners, 1);
    let n = walked.len();
    let row = |at: Vec2, d: f32, share: f32| {
        (
            Vec3::new(at.x, 0.0, at.y),
            Vec3::Y,
            [share, d],
            [at.x / r, at.y / r],
        )
    };
    let mut rows = vec![row(Vec2::ZERO, 0.0, 0.0)];
    for (c, _, share) in &walked {
        rows.push(row(*c, 1.0, *share));
        rows.push(row(*c * HALO, HALO, *share));
    }
    // The fan: the middle, then each corner and the next, turning as the strip does.
    let mut indices: Vec<u32> = (0..n as u32 - 1)
        .flat_map(|i| [0, 1 + 2 * i, 3 + 2 * i])
        .collect();
    indices.extend(highlight::strip(n).into_iter().map(|i| i + 1));
    highlight::mesh(rows, indices)
}

/// A piece's passage from one portal to another: it arrives on the first (`arrive` seconds
/// coming down onto it; none when it is there already), the first opens under it, it sinks into
/// it, the second opens, it rises out of the second, and each closes behind it. Which portal is
/// where is its owner's to say.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Passage {
    pub arrive: f32,
}

/// Where a piece on its passage is: coming down onto the first place (how far: 0 to 1), going
/// into it (how far down, as a share of its height), wholly under, coming up out of the second
/// (how far up), or there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Stage {
    Arriving(f32),
    Sinking(f32),
    Under,
    Rising(f32),
    Done,
}

/// A passage's timing, after the piece has arrived: a moment standing on the opening portal,
/// going down, under, and coming up.
const SETTLE: f32 = 0.15;
const SINK: f32 = 0.5;
const UNDER: f32 = 0.08;
const RISE: f32 = 0.5;

/// Eased in and out.
fn ease(f: f32) -> f32 {
    let f = f.clamp(0.0, 1.0);
    f * f * (3.0 - 2.0 * f)
}

impl Passage {
    /// When it starts going in: once it has stood a moment, and the first portal is open.
    fn sink_at(&self) -> f32 {
        (self.arrive + SETTLE).max(OPENING)
    }

    fn rise_at(&self) -> f32 {
        self.sink_at() + SINK + UNDER
    }

    /// When the piece is there, risen.
    pub fn risen(&self) -> f32 {
        self.rise_at() + RISE
    }

    /// How long the whole takes, until the second portal has closed.
    pub fn length(&self) -> f32 {
        self.risen() + CLOSING
    }

    /// Where the piece is `secs` into it.
    pub fn stage(&self, secs: f32) -> Stage {
        if secs < self.arrive {
            Stage::Arriving(secs / self.arrive)
        } else if secs < self.sink_at() {
            Stage::Sinking(0.0)
        } else if secs < self.sink_at() + SINK {
            Stage::Sinking(ease((secs - self.sink_at()) / SINK))
        } else if secs < self.rise_at() {
            Stage::Under
        } else if secs < self.risen() {
            Stage::Rising(ease((secs - self.rise_at()) / RISE))
        } else {
            Stage::Done
        }
    }

    /// Each portal's time (`opens`, `stays`), the first's then the second's: the first open
    /// as the piece starts going in, closing once it is under; the second open as it starts
    /// coming up, closing once it is out.
    pub fn portals(&self) -> [(f32, f32); 2] {
        let first = (self.sink_at() - OPENING).max(0.0);
        let second = self.rise_at() - OPENING - UNDER;
        [
            (first, self.sink_at() + SINK - first - OPENING),
            (second, self.risen() - second - OPENING),
        ]
    }
}

/// Where a piece on its passage is at `stage`, and whether it is seen: coming down onto `first`
/// from `drop` over it (falling faster and faster, as a dropped piece does), going `depth` down
/// into it, unseen as deep while wholly under, coming up out of `rest` from as deep, and at
/// `rest` once done.
pub fn pose(
    stage: Stage,
    first: Transform,
    rest: Transform,
    depth: f32,
    drop: f32,
) -> (Transform, bool) {
    let (mut at, by) = match stage {
        Stage::Arriving(f) => (first, drop * (1.0 - f * f)),
        Stage::Sinking(f) => (first, -depth * f),
        Stage::Under => (first, -depth),
        Stage::Rising(f) => (rest, -depth * (1.0 - f)),
        Stage::Done => (rest, 0.0),
    };
    at.translation.y += by;
    (at, stage != Stage::Under)
}

/// The portal's material, tuned for bloom or without it.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct PortalMaterial {
    #[uniform(0)]
    pub look: Look,
}

/// What the shader is told (`portal.wesl` says what each part means).
#[derive(ShaderType, Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub rim: Vec4,
    pub deep: Vec4,
    pub shape: Vec4,
    pub motion: Vec4,
    pub halo: Vec4,
}

/// The rim's light and the arms', linear: a cool silver, and a deeper blue.
const RIM_COLOR: Vec3 = Vec3::new(0.62, 0.8, 1.0);
const DEEP_COLOR: Vec3 = Vec3::new(0.32, 0.42, 1.0);

impl Look {
    /// Where the camera blooms or does not. With bloom, the rim's line is a little brighter
    /// than white so its core blooms (the threshold is 1.2: `scene.rs`); without, no brighter
    /// than the picture shows, and a wider halo of its own stands in for the bloom.
    pub fn tuned(bloom: bool) -> Self {
        let (line, width, halo, spread, arms) = if bloom {
            (1.7, 0.05, 0.5, 0.06, 0.55)
        } else {
            (0.95, 0.07, 0.45, 0.08, 0.6)
        };
        Look {
            rim: RIM_COLOR.extend(line),
            deep: DEEP_COLOR.extend(arms),
            // The line a little in from the outline; the inside nearly black.
            shape: Vec4::new(0.92, width, halo, 0.9),
            // Three arms, a turn of twist from the rim to the middle, a turn every 2.5 s; the
            // whole breathes every 2.
            motion: Vec4::new(3.0, std::f32::consts::TAU, 0.4, 0.5),
            halo: Vec4::new(HALO, spread, 0.0, 0.0),
        }
    }
}

impl Material for PortalMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Path(crate::assets::path(SHADER).into())
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
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
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// What portals are drawn with: the material, made on first use.
#[derive(Resource, Default)]
struct Drawn {
    material: Option<Handle<PortalMaterial>>,
}

/// Keeps the material tuned to whether the camera blooms.
fn tune(
    bloom: Res<BloomOn>,
    mut drawn: ResMut<Drawn>,
    materials: Option<ResMut<Assets<PortalMaterial>>>,
) {
    let Some(mut materials) = materials else {
        return;
    };
    match &drawn.material {
        None => {
            drawn.material = Some(materials.add(PortalMaterial {
                look: Look::tuned(bloom.0),
            }))
        }
        Some(handle) if bloom.is_changed() => {
            if let Some(mut m) = materials.get_mut(handle) {
                m.look = Look::tuned(bloom.0);
            }
        }
        Some(_) => {}
    }
}

/// Gives a portal just made its mesh and material, shut.
fn dress(
    mut commands: Commands,
    drawn: Res<Drawn>,
    mut meshes: ResMut<Assets<Mesh>>,
    portals: Query<(Entity, &Portal), Added<Portal>>,
) {
    for (entity, portal) in &portals {
        let mut e = commands.entity(entity);
        e.insert((
            Mesh3d(meshes.add(mesh(&portal.corners))),
            NotShadowCaster,
            Pickable::IGNORE,
            Visibility::Hidden,
        ));
        if let Some(material) = &drawn.material {
            e.insert(MeshMaterial3d(material.clone()));
        }
    }
}

/// The smallest a portal is drawn, as a share of its outline: shut is hidden, not a point.
const SHUT: f32 = 0.01;

/// Opens and closes each portal as its time says, and takes it away once it has closed.
fn open(
    mut commands: Commands,
    time: Res<Time>,
    mut portals: Query<(Entity, &mut Portal, &mut Transform, &mut Visibility)>,
) {
    for (entity, mut portal, mut t, mut visibility) in &mut portals {
        portal.secs += time.delta_secs();
        match portal.openness(portal.secs) {
            None => commands.entity(entity).despawn(),
            Some(open) => {
                let s = open.max(SHUT);
                t.scale = Vec3::new(s, 1.0, s);
                visibility.set_if_neq(if open < SHUT {
                    Visibility::Hidden
                } else {
                    Visibility::Inherited
                });
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
    fn a_portal_opens_stands_open_and_closes() {
        let p = Portal::new(square(0.01), 0.5, 1.0);
        assert_eq!(p.openness(0.0), Some(0.0));
        assert_eq!(p.openness(0.5), Some(0.0));
        let half = p.openness(0.5 + OPENING / 2.0).unwrap();
        assert!(half > 0.5 && half < 1.0, "fast, then slowing: {half}");
        assert_eq!(p.openness(0.5 + OPENING + 0.5), Some(1.0));
        let closing = p.openness(0.5 + OPENING + 1.0 + CLOSING / 2.0).unwrap();
        assert!((closing - 0.5).abs() < 1e-5, "{closing}");
        assert_eq!(p.openness(0.5 + OPENING + 1.0 + CLOSING + 0.01), None);
    }

    #[test]
    fn its_mesh_is_a_fan_to_the_outline_and_a_band_beyond_it() {
        let corners = square(0.01);
        let m = mesh(&corners);
        let Some(VertexAttributeValues::Float32x3(p)) = m.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions");
        };
        let Some(VertexAttributeValues::Float32x2(uv)) = m.attribute(Mesh::ATTRIBUTE_UV_0) else {
            panic!("uvs");
        };
        // The middle, then each corner (the first again at the end) and its halo's.
        assert_eq!(p.len(), 1 + 2 * 5);
        assert_eq!(p[0], [0.0, 0.0, 0.0]);
        assert_eq!([p[1][0], p[1][2]], [-0.01, -0.01]);
        assert_eq!(uv[1][1], 1.0);
        assert!((p[2][0] + 0.01 * HALO).abs() < 1e-7 && uv[2][1] == HALO);
        assert!(p.iter().all(|v| v[1] == 0.0), "flat");
        let indices: Vec<usize> = m.indices().unwrap().iter().collect();
        assert_eq!(indices.len(), 3 * 4 + 6 * 4);
        assert!(indices.iter().all(|i| *i < p.len()));
        // Every triangle of the fan faces up, as the board's top does.
        for t in indices[..12].chunks(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| Vec3::from_array(p[i]));
            assert!((b - a).cross(c - a).y > 0.0, "{t:?}");
        }
    }

    #[test]
    fn a_passage_goes_down_into_the_first_and_up_out_of_the_second() {
        let p = Passage { arrive: 0.28 };
        assert!(matches!(p.stage(0.1), Stage::Arriving(f) if f > 0.0 && f < 1.0));
        assert_eq!(p.stage(0.3), Stage::Sinking(0.0));
        let Stage::Sinking(down) = p.stage(p.sink_at() + SINK / 2.0) else {
            panic!("sinking");
        };
        assert!((down - 0.5).abs() < 1e-5);
        assert_eq!(p.stage(p.sink_at() + SINK + UNDER / 2.0), Stage::Under);
        assert!(matches!(p.stage(p.risen() - 0.01), Stage::Rising(f) if f > 0.9));
        assert_eq!(p.stage(p.risen()), Stage::Done);
        assert!(p.length() > 1.4 && p.length() < 2.2, "{}", p.length());
        // Already there: it starts at once.
        assert_eq!(Passage { arrive: 0.0 }.stage(0.0), Stage::Sinking(0.0));
    }

    #[test]
    fn the_piece_comes_down_goes_in_and_comes_up_where_it_rests() {
        let first = Transform::from_xyz(-0.1, 0.03, 0.0);
        let rest = Transform::from_xyz(0.1, 0.03, 0.0);
        let at = |stage| pose(stage, first, rest, 0.05, 0.06).0.translation;
        let near = |a: Vec3, b: Vec3| (a - b).length() < 1e-6;
        assert!(near(at(Stage::Arriving(0.0)), Vec3::new(-0.1, 0.09, 0.0)));
        assert_eq!(at(Stage::Sinking(0.0)), first.translation);
        assert!(
            near(at(Stage::Sinking(1.0)), Vec3::new(-0.1, -0.02, 0.0)),
            "its top under"
        );
        assert!(near(at(Stage::Under), Vec3::new(-0.1, -0.02, 0.0)));
        assert!(!pose(Stage::Under, first, rest, 0.05, 0.06).1, "unseen");
        assert!(pose(Stage::Rising(0.5), first, rest, 0.05, 0.06).1);
        assert!(near(at(Stage::Rising(0.0)), Vec3::new(0.1, -0.02, 0.0)));
        assert_eq!(at(Stage::Done), rest.translation);
    }

    #[test]
    fn each_portal_is_open_while_the_piece_goes_through_it() {
        for arrive in [0.0, 0.28] {
            let p = Passage { arrive };
            let [first, second] = p
                .portals()
                .map(|(opens, stays)| Portal::new(square(0.01), opens, stays));
            let open = |portal: &Portal, at: f32| portal.openness(at) == Some(1.0);
            // The first is wholly open from when the piece starts down until it is under.
            assert!(open(&first, p.sink_at()) && open(&first, p.sink_at() + SINK - 0.01));
            assert!(
                first.openness(p.rise_at() + CLOSING).is_none(),
                "closed behind it"
            );
            // The second from when it starts up until it is out, and closed when all is done.
            assert!(!open(&second, p.sink_at()));
            assert!(open(&second, p.rise_at()) && open(&second, p.risen() - 0.01));
            assert!(second.openness(p.length() + 0.01).is_none());
        }
    }

    #[test]
    fn bloom_tunes_the_rim_brighter_than_white_and_without_it_no_brighter() {
        assert!(Look::tuned(true).rim.w > 1.2);
        assert!(Look::tuned(false).rim.w <= 1.2);
        assert!(Look::tuned(false).halo.y > Look::tuned(true).halo.y);
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<PortalMaterial>()
            .add_plugins(PortalSystems);
        app
    }

    #[test]
    fn a_portal_is_drawn_until_it_has_closed() {
        let mut app = app();
        // No time to wait: open at once, and closed at once.
        let portal = app
            .world_mut()
            .spawn((Portal::new(square(0.01), 0.0, 0.0), Transform::default()))
            .id();
        app.update();
        let world = app.world();
        assert!(world.get::<Mesh3d>(portal).is_some());
        assert!(
            world
                .get::<MeshMaterial3d<PortalMaterial>>(portal)
                .is_some()
        );
        app.world_mut().get_mut::<Portal>(portal).unwrap().secs = OPENING + CLOSING + 0.1;
        app.update();
        assert!(app.world().get_entity(portal).is_err(), "taken away");
    }
}
