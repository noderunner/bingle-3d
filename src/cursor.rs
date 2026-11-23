//! The pointer's own hand, drawn in place of the system's cursor, which is never shown over the
//! window: a small glove of capsules, open over nothing, its fingers relaxed to pick up over
//! what a click picks up (a piece on the board, a bowl's stone, a seat's supply, a card), and
//! pointing over what a click chooses but does not pick up (a position, a button). It is hidden
//! while anything is held under the pointer (`hand::Held`), which is the pointer then, wherever
//! the pointer is but over the interface, where a button still wants it. Not by whether the
//! pointer is on the board: that changes from one cell to the next, and the hand flickered.
//!
//! What a click on the table would do is the table's to say: the board's (`play.rs`) and the
//! cards' (`card_table.rs`) systems in [`Sense`] read what the pointer is over ([`Under`]) and
//! write [`TableFeel`]. A button of the interface is pointed at here. The hand is drawn by a
//! camera of its own into a small image, shown as the topmost node of the interface at the
//! pointer; its pose and where it is are plain functions (`pose`, `bones`, `placed`).

use std::f32::consts::FRAC_PI_2;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, ImageRenderTarget, RenderTarget, ScalingMode};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::picking::Pickable;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::prelude::*;
use bevy::render::render_resource::{Face, TextureFormat};
use bevy::render::view::Tonemapping;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Button;
use bevy::window::{CursorOptions, PrimaryWindow};

use crate::hand::{Held, PointerOnBoard, approach};

pub struct CursorPlugin;

impl Plugin for CursorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(SensePlugin)
            .add_systems(Startup, spawn_hand)
            .add_systems(Update, (show, pose_hand).chain().after(Decide));
    }
}

/// What the hand is, without drawing it: what the pointer is over, what a click there would do,
/// and where the hand is. The tests run this alone.
pub struct SensePlugin;

impl Plugin for SensePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Under>()
            .init_resource::<TableFeel>()
            .init_resource::<PointerOnBoard>()
            .init_resource::<HoverMap>()
            .init_resource::<HandShown>()
            .configure_sets(Update, (Sense.after(find_under), Decide.after(Sense)))
            .add_systems(Update, (find_under, decide.in_set(Decide)));
    }
}

/// The systems that say what a click on the table under the pointer would do.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Sense;

/// Where the hand's pose and place are decided, once [`Sense`] has said.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
struct Decide;

/// What a click on what the pointer is over would do, as the hand shows it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Feel {
    /// Nothing: the hand open.
    #[default]
    Open,
    /// Chooses it: the index finger pointing.
    Point,
    /// Picks it up: palm down, the fingers relaxed to take it.
    Grab,
}

/// What the mouse's pointer is over: the nearest thing hit, then what it is part of, outwards;
/// and whether that is the interface.
#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
pub struct Under {
    pub chain: Vec<Entity>,
    pub ui: bool,
}

/// What a click on the table where the pointer is would do, as the table says. Open wherever
/// it says nothing.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TableFeel(pub Feel);

/// The hand as it is to be drawn: how it feels, and where the pointer is in the window, in
/// logical pixels (`None`: hidden).
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct HandShown {
    pub feel: Feel,
    pub at: Option<Vec2>,
}

fn find_under(
    hovers: Res<HoverMap>,
    nodes: Query<(), With<Node>>,
    parents: Query<&ChildOf>,
    mut under: ResMut<Under>,
    mut feel: ResMut<TableFeel>,
) {
    let hits = hovers.get(&PointerId::Mouse);
    // The interface is over the table: a node hit is what the pointer is over.
    let top = hits.and_then(|hits| {
        let node = hits.keys().find(|e| nodes.contains(**e));
        node.map(|e| (*e, true)).or_else(|| {
            hits.iter()
                .min_by(|a, b| a.1.depth.total_cmp(&b.1.depth))
                .map(|(e, _)| (*e, false))
        })
    });
    let now = match top {
        Some((entity, ui)) => Under {
            chain: std::iter::once(entity)
                .chain(parents.iter_ancestors(entity))
                .collect(),
            ui,
        },
        None => Under::default(),
    };
    if *under != now {
        *under = now;
        // Somewhere else: the table says again what a click there would do.
        feel.set_if_neq(TableFeel::default());
    }
}

/// How the hand feels and where it is. The system's cursor is hidden over the window, always;
/// the hand is hidden off it, and while anything is held under the pointer, but over the
/// interface.
fn decide(
    under: Res<Under>,
    table: Res<TableFeel>,
    held: Query<(), With<Held>>,
    buttons: Query<Has<InteractionDisabled>, With<Button>>,
    mut windows: Query<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    mut shown: ResMut<HandShown>,
) {
    let Ok((window, mut cursor)) = windows.single_mut() else {
        return;
    };
    if cursor.visible {
        cursor.visible = false;
    }
    let holding = !held.is_empty() && !under.ui;
    let feel = if under.ui {
        let button = under.chain.iter().find_map(|e| buttons.get(*e).ok());
        if button == Some(false) {
            Feel::Point
        } else {
            Feel::Open
        }
    } else {
        table.0
    };
    shown.set_if_neq(HandShown {
        feel,
        at: window.cursor_position().filter(|_| !holding),
    });
}

/// The render layer the hand alone is on.
const LAYER: usize = 7;
/// The hand's image: its pixels each way, how wide it is shown (logical pixels), and how much
/// of the hand's space it shows each way (the palm is about 0.9 across).
const PIXELS: u32 = 128;
const SHOWN: f32 = 52.0;
const VIEW: f32 = 3.0;
/// Where in the image the index fingertip is, the pointer's hot spot: a share of the image
/// from its top left.
const HOT: Vec2 = Vec2::new(0.4, 0.14);
/// How thick the dark rim round the glove is, in the hand's space.
const RIM: f32 = 0.085;

/// A hand's pose: each finger's, index to little, how far it leans towards the thumb and how
/// far each of its joints is bent towards the palm; the thumb's, how far it is spread from the
/// palm, swung under it, and its two joints bent; and the whole hand's, how far it leans
/// towards the thumb and how far its fingers are tipped away from the eye. Radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub fingers: [[f32; 4]; 4],
    pub thumb: [f32; 4],
    pub lean: f32,
    pub tip: f32,
}

impl Pose {
    pub fn lerp(&self, to: &Pose, t: f32) -> Pose {
        let l = |a: f32, b: f32| a * (1.0 - t) + b * t;
        let mut out = *self;
        for (f, g) in out.fingers.iter_mut().zip(&to.fingers) {
            for (a, b) in f.iter_mut().zip(g) {
                *a = l(*a, *b);
            }
        }
        for (a, b) in out.thumb.iter_mut().zip(&to.thumb) {
            *a = l(*a, *b);
        }
        out.lean = l(self.lean, to.lean);
        out.tip = l(self.tip, to.tip);
        out
    }
}

/// The pose that shows a feel.
pub fn pose(feel: Feel) -> Pose {
    match feel {
        // Fingers straight but easy, a little apart; the thumb out.
        Feel::Open => Pose {
            fingers: [
                [0.06, 0.1, 0.1, 0.05],
                [0.0, 0.12, 0.12, 0.06],
                [-0.05, 0.14, 0.14, 0.08],
                [-0.12, 0.18, 0.16, 0.1],
            ],
            thumb: [0.75, 0.25, 0.1, 0.1],
            lean: 0.18,
            tip: 0.0,
        },
        // The index finger straight, the others tucked under the palm, the thumb across them.
        Feel::Point => Pose {
            fingers: [
                [0.04, 0.0, 0.03, 0.0],
                [0.0, 1.55, 1.6, 0.9],
                [-0.04, 1.6, 1.6, 0.9],
                [-0.08, 1.6, 1.5, 0.9],
            ],
            thumb: [0.45, 0.9, 0.35, 0.2],
            lean: 0.18,
            tip: 0.0,
        },
        // Palm down over the piece, the fingers relaxed and hanging to take it, the thumb
        // under them: seen from above and in front, the fingers tipped away.
        Feel::Grab => Pose {
            fingers: [
                [0.22, 0.6, 0.7, 0.4],
                [0.04, 0.65, 0.75, 0.42],
                [-0.14, 0.65, 0.75, 0.42],
                [-0.32, 0.65, 0.7, 0.4],
            ],
            thumb: [1.0, 0.65, 0.3, 0.25],
            lean: 0.12,
            tip: 0.2,
        },
    }
}

/// The shape of one part of the hand.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Shape {
    /// Along its own y, its middle at its origin.
    Capsule { radius: f32, length: f32 },
    /// A ball of radius one, stretched by its transform's scale.
    Ball,
}

/// The parts of the hand, in the order [`bones`] gives their places: the palm, the knuckles'
/// ridge, the wrist, the four fingers' three bones each (index first, from the knuckle), and
/// the thumb's three.
const FINGERS: usize = 4;
const PARTS: usize = 3 + FINGERS * 3 + 3;
/// Where each finger leaves the palm, and its bones' lengths and thickness.
const KNUCKLES: [Vec3; FINGERS] = [
    Vec3::new(-0.3, 0.4, 0.0),
    Vec3::new(-0.1, 0.44, 0.0),
    Vec3::new(0.1, 0.42, 0.0),
    Vec3::new(0.29, 0.36, 0.0),
];
const LENGTHS: [[f32; 3]; FINGERS] = [
    [0.4, 0.25, 0.18],
    [0.44, 0.28, 0.19],
    [0.41, 0.26, 0.18],
    [0.32, 0.2, 0.16],
];
const RADII: [f32; FINGERS] = [0.1, 0.105, 0.098, 0.085];
const THUMB_ROOT: Vec3 = Vec3::new(-0.36, -0.12, -0.04);
const THUMB_LENGTHS: [f32; 3] = [0.3, 0.24, 0.19];
const THUMB_RADIUS: f32 = 0.115;
/// The palm's half sizes: across, along, and thick.
const PALM: Vec3 = Vec3::new(0.43, 0.46, 0.2);

fn shapes() -> [Shape; PARTS] {
    let mut out = [Shape::Ball; PARTS];
    out[1] = Shape::Capsule {
        radius: 0.16,
        length: 0.5,
    };
    out[2] = Shape::Capsule {
        radius: 0.27,
        length: 0.05,
    };
    for f in 0..FINGERS {
        for j in 0..3 {
            out[3 + f * 3 + j] = Shape::Capsule {
                radius: RADII[f] * (1.0 - 0.06 * j as f32),
                length: LENGTHS[f][j],
            };
        }
    }
    for j in 0..3 {
        out[3 + FINGERS * 3 + j] = Shape::Capsule {
            radius: THUMB_RADIUS * (1.0 - 0.06 * j as f32),
            length: THUMB_LENGTHS[j],
        };
    }
    out
}

/// A chain of bones from `root` setting off along `start`'s y, each bent by its turn first:
/// each bone's place (its middle), and the chain's end.
fn chain(root: Vec3, start: Quat, bends: &[Quat], lengths: &[f32]) -> (Vec<Transform>, Vec3) {
    let (mut at, mut q) = (root, start);
    let mut out = Vec::new();
    for (bend, length) in bends.iter().zip(lengths) {
        q *= *bend;
        let along = q * Vec3::Y * *length;
        out.push(Transform::from_translation(at + along / 2.0).with_rotation(q));
        at += along;
    }
    (out, at)
}

/// The fingers' chains in the hand's own space (fingers up its y, its back towards +z, the
/// thumb towards -x): each part's place, and the index fingertip.
fn hand_space(pose: &Pose) -> ([Transform; PARTS], Vec3) {
    let mut out = [Transform::IDENTITY; PARTS];
    out[0] = Transform::from_scale(PALM);
    // The knuckles' ridge, kept within the palm's thickness so its rim is not drawn across it.
    out[1] = Transform::from_xyz(0.0, 0.34, 0.0)
        .with_rotation(Quat::from_rotation_z(FRAC_PI_2))
        .with_scale(Vec3::new(1.0, 1.0, 0.55));
    out[2] = Transform::from_xyz(0.03, -0.42, -0.02).with_scale(Vec3::new(1.0, 1.0, 0.6));
    let curl = |c: f32| Quat::from_rotation_x(-c);
    let mut tip = Vec3::ZERO;
    for f in 0..FINGERS {
        let [lean, a, b, c] = pose.fingers[f];
        // Each joint above the knuckle's bends the finger more towards the palm.
        let (bones, end) = chain(
            KNUCKLES[f],
            Quat::from_rotation_z(lean),
            &[curl(a), curl(b), curl(c)],
            &LENGTHS[f],
        );
        out[3 + f * 3..3 + f * 3 + 3].copy_from_slice(&bones);
        if f == 0 {
            // The end of the bone, and its round tip beyond it.
            let last = bones[2].rotation * Vec3::Y;
            tip = end + last * RADII[0] * (1.0 - 0.12);
        }
    }
    let [spread, under, a, b] = pose.thumb;
    // Out from the palm's side, swung under it, and bent across towards the fingers.
    let start = Quat::from_rotation_z(spread) * Quat::from_rotation_x(-under);
    let across = |c: f32| Quat::from_rotation_z(-c);
    let (bones, _) = chain(
        THUMB_ROOT,
        start,
        &[Quat::IDENTITY, across(a), across(b)],
        &THUMB_LENGTHS,
    );
    out[3 + FINGERS * 3..].copy_from_slice(&bones);
    (out, tip)
}

/// How the whole hand is turned in the image: leaning, and its fingers tipped away from the
/// eye (which looks down -z).
fn turned(pose: &Pose) -> Quat {
    Quat::from_rotation_z(pose.lean) * Quat::from_rotation_x(-pose.tip)
}

/// Where in the hand camera's view (x right, y up, the eye along +z) the hot spot is.
fn hot_spot() -> Vec2 {
    Vec2::new((HOT.x - 0.5) * VIEW, (0.5 - HOT.y) * VIEW)
}

/// Each part of the hand in the camera's view, placed so that the index fingertip is at the
/// image's hot spot, whatever the pose.
pub fn bones(pose: &Pose) -> [Transform; PARTS] {
    let (parts, tip) = hand_space(pose);
    let root = placed(pose, tip);
    parts.map(|p| root * p)
}

/// The hand's own transform in the view: turned, and moved so that `tip` (in its space) is at
/// the hot spot.
fn placed(pose: &Pose, tip: Vec3) -> Transform {
    let q = turned(pose);
    let at = q * tip;
    Transform::from_translation((hot_spot() - at.truncate()).extend(-at.z)).with_rotation(q)
}

/// The index fingertip in the view.
#[cfg(test)]
fn tip_in_view(pose: &Pose) -> Vec3 {
    let (_, tip) = hand_space(pose);
    placed(pose, tip) * tip
}

/// The pose now, eased towards the one wanted.
#[derive(Component)]
struct HandRoot {
    pose: Pose,
}

/// A part of the hand, by its place in [`bones`].
#[derive(Component)]
struct Part(usize);

/// The node that shows the hand's image.
#[derive(Component)]
struct HandImage;

fn spawn_hand(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let layer = RenderLayers::layer(LAYER);
    let mut image = Image::new_target_texture(PIXELS, PIXELS, TextureFormat::Rgba8UnormSrgb, None);
    image.asset_usage = RenderAssetUsages::RENDER_WORLD;
    let image = images.add(image);
    commands.spawn((
        Name::new("pointer's hand camera"),
        Camera3d::default(),
        Camera {
            // Drawn before the window's cameras, whose interface shows it.
            order: -2,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
        RenderTarget::Image(ImageRenderTarget {
            handle: image.clone(),
            scale_factor: 1.0,
        }),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: VIEW,
                height: VIEW,
            },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(0.0, 0.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
        Tonemapping::None,
        Msaa::Sample4,
        AmbientLight {
            color: Color::WHITE,
            brightness: 500.0,
            affects_lightmapped_meshes: false,
        },
        crate::scene::Apart,
        layer.clone(),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 2500.0,
            shadow_maps_enabled: false,
            ..default()
        },
        // From the upper left, in front.
        Transform::from_xyz(-1.0, 1.4, 2.0).looking_at(Vec3::ZERO, Vec3::Y),
        layer.clone(),
    ));

    let glove = materials.add(StandardMaterial {
        base_color: Color::srgb(0.96, 0.95, 0.92),
        perceptual_roughness: 0.6,
        reflectance: 0.3,
        ..default()
    });
    // The rim: the inside of a slightly larger copy, drawn black, seen only round the edge.
    let rim = materials.add(StandardMaterial {
        base_color: Color::srgb(0.04, 0.035, 0.03),
        unlit: true,
        cull_mode: Some(Face::Front),
        ..default()
    });
    let pose = pose(Feel::Open);
    let places = bones(&pose);
    let ball = meshes.add(Sphere::new(1.0).mesh().uv(32, 18));
    commands
        .spawn((
            Name::new("pointer's hand"),
            HandRoot { pose },
            Transform::default(),
            Visibility::default(),
        ))
        .with_children(|hand| {
            for (i, shape) in shapes().into_iter().enumerate() {
                let (fill, edge, edge_at) = match shape {
                    Shape::Capsule { radius, length } => (
                        meshes.add(Capsule3d::new(radius, length).mesh().rings(4)),
                        meshes.add(Capsule3d::new(radius + RIM, length).mesh().rings(4)),
                        Transform::IDENTITY,
                    ),
                    Shape::Ball => {
                        let s = places[i].scale;
                        (
                            ball.clone(),
                            ball.clone(),
                            Transform::from_scale((s + RIM) / s),
                        )
                    }
                };
                hand.spawn((
                    Part(i),
                    Mesh3d(fill),
                    MeshMaterial3d(glove.clone()),
                    places[i],
                    NotShadowCaster,
                    NotShadowReceiver,
                    layer.clone(),
                ))
                .with_child((
                    Mesh3d(edge),
                    MeshMaterial3d(rim.clone()),
                    edge_at,
                    NotShadowCaster,
                    NotShadowReceiver,
                    layer.clone(),
                ));
            }
        });
    commands.spawn((
        Name::new("pointer"),
        HandImage,
        ImageNode::new(image),
        Node {
            position_type: PositionType::Absolute,
            width: px(SHOWN),
            height: px(SHOWN),
            ..default()
        },
        // Over every panel and menu.
        GlobalZIndex(i32::MAX),
        Pickable::IGNORE,
        Visibility::Hidden,
    ));
}

/// The hand's image at the pointer, its hot spot on it; hidden where the hand is.
fn show(
    shown: Res<HandShown>,
    scale: Res<UiScale>,
    mut images: Query<(&mut Node, &mut Visibility), With<HandImage>>,
) {
    for (mut node, mut vis) in &mut images {
        let Some(at) = shown.at else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };
        // Nodes are laid out in the interface's scale; the pointer is in the window's.
        let corner = at / scale.0 - HOT * SHOWN;
        let (left, top) = (px(corner.x), px(corner.y));
        if node.left != left || node.top != top {
            node.left = left;
            node.top = top;
        }
        vis.set_if_neq(Visibility::Inherited);
    }
}

/// The hand eased into the pose its feel wants.
fn pose_hand(
    time: Res<Time>,
    shown: Res<HandShown>,
    mut roots: Query<&mut HandRoot>,
    mut parts: Query<(&Part, &mut Transform)>,
) {
    let Ok(mut root) = roots.single_mut() else {
        return;
    };
    let wanted = pose(shown.feel);
    if root.pose == wanted {
        return;
    }
    // Twice as quick as a piece goes: a hand's change of mind.
    let k = (approach(time.delta_secs()) * 2.0).min(1.0);
    let mut now = root.pose.lerp(&wanted, k);
    if (0..FINGERS).all(|f| (0..4).all(|j| (now.fingers[f][j] - wanted.fingers[f][j]).abs() < 1e-3))
        && (now.tip - wanted.tip).abs() < 1e-3
    {
        now = wanted;
    }
    root.pose = now;
    let places = bones(&now);
    for (part, mut t) in &mut parts {
        *t = places[part.0];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::picking::backend::HitData;

    fn tips(pose: &Pose) -> [Vec3; FINGERS] {
        let (parts, _) = hand_space(pose);
        std::array::from_fn(|f| {
            let last = parts[3 + f * 3 + 2];
            last.translation + last.rotation * Vec3::Y * LENGTHS[f][2] / 2.0
        })
    }

    #[test]
    fn pointing_the_index_finger_is_straight_and_the_others_are_tucked_away() {
        let point = pose(Feel::Point);
        let t = tips(&point);
        let reach = |f: usize| t[f].distance(KNUCKLES[f]);
        let full: f32 = LENGTHS[0].iter().sum();
        assert!(reach(0) > full * 0.98, "straight: {}", reach(0));
        for f in 1..FINGERS {
            assert!(t[f].y < t[0].y - 0.5, "finger {f} is not up with the index");
            assert!(
                reach(f) < LENGTHS[f].iter().sum::<f32>() * 0.6,
                "finger {f}"
            );
        }
    }

    #[test]
    fn to_pick_up_the_palm_is_down_and_every_finger_hangs_from_it() {
        let grab = pose(Feel::Grab);
        let q = turned(&grab);
        let (parts, _) = hand_space(&grab);
        // The back of the hand is towards the eye, tipped: the fingers go away from it.
        assert!((q * Vec3::Z).z > 0.5);
        for (f, tip) in tips(&grab).iter().enumerate() {
            assert!(tip.z < -0.25, "finger {f} curls under the palm: {tip}");
            let knuckle = (q * KNUCKLES[f]).z;
            assert!((q * *tip).z < knuckle, "and away from the eye");
        }
        // The thumb under them too.
        let thumb = parts[PARTS - 1].translation;
        assert!(thumb.z < THUMB_ROOT.z, "{thumb}");
        // Open, it does not.
        assert!(tips(&pose(Feel::Open)).iter().all(|t| t.z > -0.25));
    }

    #[test]
    fn the_index_fingertip_is_the_hot_spot_in_every_pose_and_on_the_way_between() {
        let (open, point, grab) = (pose(Feel::Open), pose(Feel::Point), pose(Feel::Grab));
        for p in [
            open,
            point,
            grab,
            open.lerp(&grab, 0.5),
            point.lerp(&grab, 0.3),
        ] {
            let tip = tip_in_view(&p);
            assert!(tip.truncate().distance(hot_spot()) < 1e-5, "{tip}");
            // The whole hand is in the picture.
            for b in bones(&p) {
                let at = b.translation.truncate();
                assert!(at.abs().max_element() < VIEW / 2.0 - 0.12, "{at}");
            }
        }
        assert_eq!(open.lerp(&grab, 0.0), open);
        assert_eq!(open.lerp(&grab, 1.0), grab);
    }

    fn app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, SensePlugin));
        let mut window = Window::default();
        window.set_physical_cursor_position(Some(bevy::math::DVec2::new(100.0, 50.0)));
        let window = app
            .world_mut()
            .spawn((window, CursorOptions::default(), PrimaryWindow))
            .id();
        (app, window)
    }

    fn hover(app: &mut App, entity: Option<Entity>) {
        let mut hits = bevy::ecs::entity::EntityHashMap::default();
        if let Some(e) = entity {
            hits.insert(e, HitData::new(Entity::PLACEHOLDER, 1.0, None, None));
        }
        app.world_mut()
            .resource_mut::<HoverMap>()
            .insert(PointerId::Mouse, hits);
    }

    #[test]
    fn the_system_cursor_is_hidden_and_the_hand_points_at_a_button_it_may_press() {
        let (mut app, window) = app();
        app.update();
        assert!(!app.world().get::<CursorOptions>(window).unwrap().visible);
        let shown = *app.world().resource::<HandShown>();
        assert_eq!(shown.feel, Feel::Open);
        assert_eq!(shown.at, Some(Vec2::new(100.0, 50.0)));

        // The label on a button: pointing at it, as at the button.
        let button = app.world_mut().spawn((Node::default(), Button)).id();
        let label = app
            .world_mut()
            .spawn((Node::default(), ChildOf(button)))
            .id();
        hover(&mut app, Some(label));
        app.update();
        assert_eq!(app.world().resource::<HandShown>().feel, Feel::Point);
        // Greyed: nothing to press.
        app.world_mut()
            .entity_mut(button)
            .insert(InteractionDisabled);
        app.update();
        assert_eq!(app.world().resource::<HandShown>().feel, Feel::Open);
        // A panel that is not a button.
        let panel = app.world_mut().spawn(Node::default()).id();
        hover(&mut app, Some(panel));
        app.update();
        assert_eq!(app.world().resource::<HandShown>().feel, Feel::Open);
    }

    #[test]
    fn over_the_table_the_hand_feels_as_the_table_says_and_gives_way_to_what_it_holds() {
        fn say_grab(under: Res<Under>, mut feel: ResMut<TableFeel>) {
            if !under.chain.is_empty() {
                feel.0 = Feel::Grab;
            }
        }
        let (mut app, _) = app();
        app.add_systems(Update, say_grab.in_set(Sense));
        let piece = app.world_mut().spawn_empty().id();
        hover(&mut app, Some(piece));
        app.update();
        let shown = *app.world().resource::<HandShown>();
        assert_eq!(shown.feel, Feel::Grab);
        assert!(shown.at.is_some());
        assert!(!app.world().resource::<Under>().ui);

        // Something held in the pointer's place: the hand gives way to it, and stays away
        // whether the pointer is on a cell, between two, or off the board, as it is from one
        // frame to the next in a quick move.
        let held = app
            .world_mut()
            .spawn(Held {
                board: 0.0,
                lift: 0.02,
                scale: 1.0,
            })
            .id();
        for on in [
            Some((Vec3::ZERO, Vec3::Y)),
            None,
            Some((Vec3::X, Vec3::Y)),
            None,
        ] {
            app.world_mut().resource_mut::<PointerOnBoard>().0 = on;
            app.update();
            assert_eq!(app.world().resource::<HandShown>().at, None, "{on:?}");
        }
        hover(&mut app, None);
        app.update();
        assert_eq!(app.world().resource::<HandShown>().at, None, "over nothing");
        // Over a panel it is back, for its buttons.
        let panel = app.world_mut().spawn(Node::default()).id();
        hover(&mut app, Some(panel));
        app.update();
        assert!(app.world().resource::<HandShown>().at.is_some());
        // Back over the table, and once nothing is held, it is back there too.
        hover(&mut app, Some(piece));
        app.update();
        assert_eq!(app.world().resource::<HandShown>().at, None);
        app.world_mut().despawn(held);
        app.update();
        assert!(app.world().resource::<HandShown>().at.is_some());

        // Over nothing: open, whatever the table said before.
        hover(&mut app, None);
        app.update();
        assert_eq!(app.world().resource::<TableFeel>().0, Feel::Open);
    }
}
