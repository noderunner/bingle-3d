//! Where the table stands, built from the environment the player has chosen (`environment.rs`):
//! its sun, which casts the shadows, its sky's light and what is seen behind everything, its
//! mist, its model or its plain table top, and a cushion for each seat of the game. The board of
//! the game being watched stands on the table (`board.rs`), and knows none of this.
//!
//! The sky's light is two cubemaps Bevy's `EnvironmentMapLight` reads, made by the asset
//! pipeline: the soft light from every side, and what polished things reflect. Every 3D camera
//! gets it, and what makes a photograph of a table look like one:
//!
//! - ambient occlusion (SSAO), so a stone is grounded on the board and the cut lines are dark;
//! - temporal antialiasing (TAA), which keeps fine grain from shimmering and smooths the shadows'
//!   edges (the sun's are filtered over time); SSAO and TAA both need MSAA off;
//! - a little bloom on the brightest highlights;
//! - a shallow depth of field, focused where the orbit camera looks.
//!
//! A model is drawn node by node from its glTF file, which has no materials of its own: each
//! part's material is named for one in the environment's file. So a texture is loaded once
//! through `textures.rs`, which gives it mipmaps, and a look is changed by editing data.
//!
//! One unit is one metre and the table top is at y = 0, so a board is the size of a real one.

use std::collections::{HashMap, HashSet};

use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::camera::Exposure;
use bevy::gltf::{Gltf, GltfMaterial, GltfMesh, GltfNode};
use bevy::light::{CascadeShadowConfigBuilder, NotShadowCaster, ShadowFilteringMethod, Skybox};
use bevy::pbr::{ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel};
use bevy::post_process::bloom::{Bloom, BloomPrefilter};
use bevy::post_process::dof::{DepthOfField, DepthOfFieldMode};
use bevy::prelude::*;

use crate::camera::{OrbitCamera, Reach};
use crate::environment::{self, Chosen, Environments};
use crate::net::WatchStatus;
use crate::{assets, look};

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        // The sky lights everything; there is no light from nowhere as well.
        app.insert_resource(GlobalAmbientLight {
            brightness: 0.0,
            ..default()
        })
        .init_resource::<Built>()
        .add_systems(
            Update,
            (
                build_environment.run_if(resource_changed::<Chosen>),
                light_cameras,
                focus_on_orbit,
                spawn_model,
                lay_seats,
                drift,
            )
                .chain(),
        );
    }
}

/// A part of the environment built: gone when another is chosen.
#[derive(Component)]
struct Part;

/// A cushion laid out for a seat.
#[derive(Component)]
struct SeatCushion;

/// The environment a camera has been given the light of, by id.
#[derive(Component)]
struct LitFor(String);

/// A part of a model: its mesh, its material, and whether it casts a shadow.
#[derive(Clone)]
struct Piece {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    shadow: bool,
}

/// What has been built of the environment chosen.
#[derive(Resource, Default)]
struct Built {
    id: String,
    /// The model's file, until its nodes have been drawn.
    loading: Option<Handle<Gltf>>,
    /// The cushion: the parts of the model's node that is one.
    cushion: Vec<Piece>,
    /// How many cushions are laid out.
    seats: Option<usize>,
    /// The materials whose texture moves, and how far a second.
    drifting: Vec<(Handle<StandardMaterial>, Vec2)>,
}

/// The exposure a room is seen at: Bevy's own.
const ROOM_EXPOSURE: f32 = 9.7;
/// The lens's aperture where an environment does not say: the far side of the board is
/// softened, not blurred.
const F_STOPS: f32 = 8.0;

fn srgb([r, g, b]: [f32; 3]) -> Color {
    Color::srgb(r, g, b)
}

#[expect(
    clippy::too_many_arguments,
    reason = "a system takes each resource it changes as a parameter"
)]
fn build_environment(
    mut commands: Commands,
    chosen: Res<Chosen>,
    all: Res<Environments>,
    server: Res<AssetServer>,
    mut built: ResMut<Built>,
    mut reach: ResMut<Reach>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    old: Query<Entity, With<Part>>,
) {
    let Some(env) = all.chosen(&chosen) else {
        return;
    };
    for entity in &old {
        commands.entity(entity).despawn();
    }
    *built = Built {
        id: env.id.clone(),
        loading: env
            .model
            .as_ref()
            .map(|file| server.load(assets::path(file))),
        ..default()
    };
    reach.0 = env.camera.max_radius;
    commands.insert_resource(ClearColor(srgb(env.background)));

    // Bevy's default cascades are spread over a scene a hundred metres deep. The first of ours
    // is the table, a metre or two from the camera, so the shadow map's texels are there, where
    // a stone's shadow is a few millimetres wide; a place's others reach its trees.
    let far = env.sun.shadows_to;
    commands.spawn((
        Name::new("Sun"),
        Part,
        DirectionalLight {
            illuminance: env.sun.illuminance,
            color: env.sun.colour.map_or(Color::WHITE, srgb),
            shadow_maps_enabled: true,
            // The shadows the shadow map is too coarse for: under a stone, in a cut line.
            contact_shadows_enabled: true,
            // Bevy's default bias (2 cm) is larger than our pieces: at table scale it hides
            // every shadow they would cast.
            shadow_depth_bias: 0.002,
            ..default()
        },
        CascadeShadowConfigBuilder {
            num_cascades: if far > 8.0 { 4 } else { 2 },
            minimum_distance: 0.05,
            first_cascade_far_bound: 1.0,
            maximum_distance: far,
            ..default()
        }
        .build(),
        Transform::from_translation(env.sun_from()).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    if let Some(ground) = &env.ground
        && let Some(spec) = env.materials.get(&ground.material)
    {
        // Wide enough that its far edge is lost in the mist from any angle the camera takes.
        let mesh = look::world_uvs(
            Plane3d::default()
                .mesh()
                .size(ground.size, ground.size)
                .build(),
            Vec3::ZERO,
        );
        commands.spawn((
            Name::new("Table"),
            Part,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(spec.build(&server))),
        ));
    }
}

/// A camera, the environment it is lit for, and where it orbits.
type CameraToLight = (
    Entity,
    Option<&'static LitFor>,
    Option<&'static OrbitCamera>,
    Has<Headset>,
);

/// A camera that is an eye of a headset (`vr.rs`). It gets the environment's light, but none
/// of what works on the picture afterwards: temporal anti-aliasing and ambient occlusion
/// swim when each eye has its own, and the eye focuses for itself.
#[derive(Component)]
#[cfg_attr(not(feature = "vr"), allow(dead_code))]
pub struct Headset;

/// Gives every 3D camera the light of the environment chosen: a new camera, and every camera
/// when another is chosen.
fn light_cameras(
    mut commands: Commands,
    chosen: Res<Chosen>,
    all: Res<Environments>,
    server: Res<AssetServer>,
    cameras: Query<CameraToLight, With<Camera3d>>,
) {
    let Some(env) = all.chosen(&chosen) else {
        return;
    };
    for (camera, lit, orbit, headset) in &cameras {
        if lit.is_some_and(|l| l.0 == env.id) {
            continue;
        }
        let rotation = env.sky_rotation();
        commands.entity(camera).insert((
            LitFor(env.id.clone()),
            EnvironmentMapLight {
                diffuse_map: server.load(assets::path(&env.sky.diffuse)),
                specular_map: server.load(assets::path(&env.sky.specular)),
                intensity: env.sky.intensity,
                rotation,
                ..default()
            },
            Exposure {
                ev100: env.exposure.unwrap_or(ROOM_EXPOSURE),
            },
            // The table fades into the room past arm's length, rather than ending; a garden
            // into its mist.
            DistanceFog {
                color: srgb(env.fog.colour),
                falloff: FogFalloff::Linear {
                    start: env.fog.start,
                    end: env.fog.end,
                },
                ..default()
            },
        ));
        if headset {
            commands.entity(camera).insert(Msaa::Sample4);
        } else {
            commands.entity(camera).insert((
                Msaa::Off,
                TemporalAntiAliasing::default(),
                ShadowFilteringMethod::Temporal,
                ScreenSpaceAmbientOcclusion {
                    quality_level: ScreenSpaceAmbientOcclusionQualityLevel::High,
                    // At table scale: a stone, a cut line, the board's edge.
                    radius: 0.02,
                    constant_object_thickness: 0.004,
                },
                // Only what is brighter than white glows (a highlight on a stone), so nothing else
                // is veiled.
                Bloom {
                    intensity: 0.12,
                    prefilter: BloomPrefilter {
                        threshold: 1.2,
                        threshold_softness: 0.3,
                    },
                    ..Bloom::OLD_SCHOOL
                },
                DepthOfField {
                    mode: DepthOfFieldMode::Gaussian,
                    focal_distance: orbit.map_or(0.75, |o| o.radius),
                    aperture_f_stops: env.camera.f_stops.unwrap_or(F_STOPS),
                    ..default()
                },
            ));
        }
        match &env.sky.backdrop {
            Some(file) => {
                commands.entity(camera).insert(Skybox {
                    image: Some(server.load(assets::path(file))),
                    brightness: env.sky.backdrop_brightness,
                    rotation,
                });
            }
            None => {
                commands.entity(camera).remove::<Skybox>();
            }
        }
    }
}

/// Keeps the depth of field focused on what the orbit camera looks at.
fn focus_on_orbit(mut cameras: Query<(&OrbitCamera, &mut DepthOfField), Changed<OrbitCamera>>) {
    for (orbit, mut dof) in &mut cameras {
        dof.focal_distance = orbit.radius;
    }
}

/// The assets a model is drawn from, as one system parameter.
#[derive(bevy::ecs::system::SystemParam)]
struct Models<'w> {
    files: Res<'w, Assets<Gltf>>,
    nodes: Res<'w, Assets<GltfNode>>,
    meshes: Res<'w, Assets<GltfMesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    server: Res<'w, AssetServer>,
}

/// Draws the model once its file has loaded: every node where it stands, but the cushion, which
/// is kept as a pattern for `lay_seats`.
fn spawn_model(
    mut commands: Commands,
    all: Res<Environments>,
    mut built: ResMut<Built>,
    mut models: Models,
) {
    let Some(handle) = built.loading.clone() else {
        return;
    };
    let (Some(file), Some(env)) = (models.files.get(&handle), all.get(&built.id)) else {
        return;
    };
    built.loading = None;

    // Each of the file's materials is only a name here: the environment says what it is.
    let names: HashMap<AssetId<GltfMaterial>, &str> = file
        .named_materials
        .iter()
        .map(|(name, handle)| (handle.id(), &**name))
        .collect();
    let mut made: HashMap<String, Handle<StandardMaterial>> = HashMap::new();
    let mut unknown: HashSet<String> = HashSet::new();
    let mut pieces_of = |node: &GltfNode, materials: &mut Assets<StandardMaterial>| -> Vec<Piece> {
        let Some(mesh) = node.mesh.as_ref().and_then(|m| models.meshes.get(m)) else {
            return Vec::new();
        };
        mesh.primitives
            .iter()
            .map(|primitive| {
                let name = primitive
                    .material
                    .as_ref()
                    .and_then(|m| names.get(&m.id()).copied())
                    .unwrap_or("");
                let material = made
                    .entry(name.to_string())
                    .or_insert_with(|| match env.materials.get(name) {
                        Some(spec) => materials.add(spec.build(&models.server)),
                        None => {
                            unknown.insert(name.to_string());
                            materials.add(StandardMaterial::default())
                        }
                    })
                    .clone();
                Piece {
                    mesh: primitive.mesh.clone(),
                    material,
                    shadow: !env.shadows.none.iter().any(|n| n == name),
                }
            })
            .collect()
    };

    // The nodes no other node holds stand in the world; the rest hang from them.
    let children: HashSet<AssetId<GltfNode>> = file
        .nodes
        .iter()
        .filter_map(|n| models.nodes.get(n))
        .flat_map(|n| n.children.iter().map(Handle::id))
        .collect();
    let seat = env.seating.as_ref().map(|s| s.node.as_str());
    let mut cushion = Vec::new();
    let mut todo: Vec<(Handle<GltfNode>, Option<Entity>)> = file
        .nodes
        .iter()
        .filter(|n| !children.contains(&n.id()))
        .map(|n| (n.clone(), None))
        .collect();
    while let Some((handle, parent)) = todo.pop() {
        let Some(node) = models.nodes.get(&handle) else {
            continue;
        };
        let pieces = pieces_of(node, &mut models.materials);
        if seat == Some(node.name.as_str()) {
            cushion = pieces;
            continue;
        }
        let mut entity = commands.spawn((
            Name::new(node.name.clone()),
            node.transform,
            Visibility::default(),
        ));
        match parent {
            Some(parent) => entity.insert(ChildOf(parent)),
            None => entity.insert(Part),
        };
        let entity = entity.id();
        spawn_pieces(&mut commands, entity, &pieces);
        todo.extend(node.children.iter().map(|c| (c.clone(), Some(entity))));
    }
    for name in unknown {
        warn!("{}: no material {name:?} for its model", env.id);
    }
    built.drifting = env
        .drift
        .iter()
        .filter_map(|(name, by)| Some((made.get(name)?.clone(), Vec2::from_array(*by))))
        .collect();
    built.cushion = cushion;
    built.seats = None;
}

fn spawn_pieces(commands: &mut Commands, parent: Entity, pieces: &[Piece]) {
    for piece in pieces {
        let mut part = commands.spawn((
            Mesh3d(piece.mesh.clone()),
            MeshMaterial3d(piece.material.clone()),
            ChildOf(parent),
        ));
        if !piece.shadow {
            part.insert(NotShadowCaster);
        }
    }
}

/// How many cushions there are when no game is on the table: two, facing each other.
const SEATS_WAITING: usize = 2;

/// A cushion for each seat of the game on the table, where the environment has cushions.
fn lay_seats(
    mut commands: Commands,
    all: Res<Environments>,
    watch: Option<Res<WatchStatus>>,
    mut built: ResMut<Built>,
    laid: Query<Entity, With<SeatCushion>>,
) {
    let Some(seating) = all.get(&built.id).and_then(|e| e.seating.as_ref()) else {
        return;
    };
    if built.cushion.is_empty() {
        return;
    }
    let seats = watch
        .as_deref()
        .and_then(|w| w.table.as_ref())
        .map_or(SEATS_WAITING, |t| t.info.seats.len().max(1));
    if built.seats == Some(seats) {
        return;
    }
    for entity in &laid {
        commands.entity(entity).despawn();
    }
    for (i, (at, turn)) in environment::seat_places(seats, seating.distance)
        .into_iter()
        .enumerate()
    {
        let entity = commands
            .spawn((
                Name::new(format!("Cushion of seat {}", i + 1)),
                SeatCushion,
                Part,
                Transform::from_xyz(at.x, seating.floor, at.y)
                    .with_rotation(Quat::from_rotation_y(turn)),
                Visibility::default(),
            ))
            .id();
        spawn_pieces(&mut commands, entity, &built.cushion);
    }
    built.seats = Some(seats);
}

/// Moves the textures that move: water's ripples.
fn drift(time: Res<Time>, built: Res<Built>, mut materials: ResMut<Assets<StandardMaterial>>) {
    for (handle, by) in &built.drifting {
        if let Some(mut material) = materials.get_mut(handle) {
            let moved = material.uv_transform.translation + *by * time.delta_secs();
            // Kept small: a texture repeats, and a large number loses the fraction.
            material.uv_transform.translation = moved.fract_gl();
        }
    }
}

/// The environment a camera is lit for, for tests elsewhere.
#[cfg(test)]
pub fn lit_for(world: &World, camera: Entity) -> Option<String> {
    world.get::<LitFor>(camera).map(|l| l.0.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::api::{Board, Game, GameInfo, GameState, SeatDef};
    use crate::net::session::Opened;
    use crate::table::Table;

    fn app(chosen: &str) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), ScenePlugin))
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Image>()
            .init_asset::<Gltf>()
            .init_asset::<GltfNode>()
            .init_asset::<GltfMesh>()
            .init_resource::<Environments>()
            .init_resource::<Reach>()
            .init_resource::<WatchStatus>()
            .insert_resource(Chosen(chosen.into()));
        app
    }

    /// A game with this many seats on the table.
    fn table_of(seats: u32) -> Table {
        Table::new(Opened {
            room_code: "AB12CD".into(),
            info: GameInfo {
                id: "g".into(),
                name: "A game".into(),
                seats: (1..=seats)
                    .map(|seat| SeatDef {
                        seat,
                        color: format!("c{seat}"),
                        label: String::new(),
                    })
                    .collect(),
                board: Board {
                    kind: "grid".into(),
                    positions: vec![],
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
                reservations: vec![],
                seats: vec![],
            },
            state: GameState::default(),
            bots: vec![],
        })
    }

    fn count<C: Component>(app: &mut App) -> usize {
        app.world_mut().query::<&C>().iter(app.world()).count()
    }

    #[test]
    fn the_plain_table_is_a_sun_and_a_top_and_another_place_replaces_them() {
        let mut app = app(environment::DEFAULT);
        let camera = app.world_mut().spawn(Camera3d::default()).id();
        app.update();
        app.update();
        assert_eq!(count::<DirectionalLight>(&mut app), 1);
        assert_eq!(count::<Part>(&mut app), 2, "the sun and the table top");
        assert_eq!(lit_for(app.world(), camera).as_deref(), Some("room"));
        assert!(app.world().get::<Skybox>(camera).is_none());
        assert_eq!(app.world().resource::<Reach>().0, OrbitCamera::MAX_RADIUS);

        app.world_mut().resource_mut::<Chosen>().0 = "garden".into();
        app.update();
        app.update();
        assert_eq!(count::<DirectionalLight>(&mut app), 1, "one sun at a time");
        assert_eq!(lit_for(app.world(), camera).as_deref(), Some("garden"));
        assert!(app.world().get::<Skybox>(camera).is_some(), "a sky to see");
        let garden = Environments::built_in().get("garden").unwrap().clone();
        assert_eq!(app.world().resource::<Reach>().0, garden.camera.max_radius);
        let sun = app
            .world_mut()
            .query::<&DirectionalLight>()
            .single(app.world())
            .unwrap();
        assert_eq!(sun.illuminance, garden.sun.illuminance);
    }

    #[test]
    fn a_camera_made_later_is_lit_too() {
        let mut app = app(environment::DEFAULT);
        app.update();
        let camera = app.world_mut().spawn(Camera3d::default()).id();
        app.update();
        app.update();
        assert!(app.world().get::<EnvironmentMapLight>(camera).is_some());
        assert!(app.world().get::<DistanceFog>(camera).is_some());
    }

    #[test]
    fn an_eye_of_a_headset_is_lit_but_nothing_works_on_its_picture() {
        let mut app = app("garden");
        let flat = app.world_mut().spawn(Camera3d::default()).id();
        let eye = app.world_mut().spawn((Camera3d::default(), Headset)).id();
        app.update();
        app.update();
        let world = app.world();
        for camera in [flat, eye] {
            assert!(world.get::<EnvironmentMapLight>(camera).is_some());
            assert!(world.get::<DistanceFog>(camera).is_some());
            assert!(world.get::<Skybox>(camera).is_some());
        }
        assert!(world.get::<TemporalAntiAliasing>(flat).is_some());
        assert!(world.get::<DepthOfField>(flat).is_some());
        assert!(world.get::<TemporalAntiAliasing>(eye).is_none());
        assert!(world.get::<ScreenSpaceAmbientOcclusion>(eye).is_none());
        assert!(world.get::<DepthOfField>(eye).is_none());
        assert_eq!(world.get::<Msaa>(eye), Some(&Msaa::Sample4));
    }

    #[test]
    fn a_cushion_is_laid_for_each_seat_of_the_game() {
        let mut app = app("garden");
        app.update();
        // As if the model had loaded: its cushion is known.
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Mesh::from(Cuboid::default()));
        {
            let mut built = app.world_mut().resource_mut::<Built>();
            built.loading = None;
            built.cushion = vec![Piece {
                mesh,
                material: Handle::default(),
                shadow: true,
            }];
        }
        app.update();
        assert_eq!(count::<SeatCushion>(&mut app), SEATS_WAITING);

        app.world_mut().resource_mut::<WatchStatus>().table = Some(table_of(4));
        app.update();
        app.update();
        assert_eq!(count::<SeatCushion>(&mut app), 4);
        let floor = Environments::built_in()
            .get("garden")
            .unwrap()
            .seating
            .clone()
            .unwrap()
            .floor;
        let mut cushions = app
            .world_mut()
            .query_filtered::<&Transform, With<SeatCushion>>();
        assert!(cushions.iter(app.world()).all(|t| t.translation.y == floor));

        // The plain table has none.
        app.world_mut().resource_mut::<Chosen>().0 = environment::DEFAULT.into();
        app.update();
        app.update();
        assert_eq!(count::<SeatCushion>(&mut app), 0);
    }
}
