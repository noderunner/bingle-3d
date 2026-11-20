//! The room: a felt-covered table, lit by the light of a real room and a key light that casts the
//! shadows. The board of the game being watched stands on it (`board.rs`).
//!
//! The room's light is an HDR photograph of a room, filtered by the asset pipeline into the two
//! cubemaps Bevy's `EnvironmentMapLight` reads (`assets-src/blender/environment.py`): the soft
//! light from every side, and what polished things reflect. Every 3D camera gets it, and what
//! makes a photograph of a table look like one:
//!
//! - ambient occlusion (SSAO), so a stone is grounded on the board and the cut lines are dark;
//! - temporal antialiasing (TAA), which keeps fine grain from shimmering and smooths the shadows'
//!   edges (the key light's are filtered over time); SSAO and TAA both need MSAA off;
//! - a little bloom on the brightest highlights;
//! - a shallow depth of field, focused where the orbit camera looks.
//!
//! One unit is one metre and the table top is at y = 0, so a board is the size of a real one.

use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::light::{CascadeShadowConfigBuilder, ShadowFilteringMethod};
use bevy::pbr::{ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel};
use bevy::post_process::bloom::{Bloom, BloomPrefilter};
use bevy::post_process::dof::{DepthOfField, DepthOfFieldMode};
use bevy::prelude::*;

use crate::camera::OrbitCamera;
use crate::{assets, look};

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(ROOM_DARK))
            // The room lights everything; there is no light from nowhere as well.
            .insert_resource(GlobalAmbientLight {
                brightness: 0.0,
                ..default()
            })
            .add_systems(Startup, (spawn_lights, spawn_table))
            .add_systems(Update, (light_cameras, focus_on_orbit).chain());
    }
}

/// Where the key light comes from: the far right, and low enough (40° up) that stones cast
/// shadows a player sees. From beyond the board rather than behind the player, so what faces the
/// player (the board's near rim, the stones' fronts) reflects the dim room, not the window: dark
/// polish stays dark, and highlights stand out on it.
const KEY_LIGHT_FROM: Vec3 = Vec3::new(0.7, 0.8, -0.6);
/// The key light's strength, in lux: it is the sun through the room's window, so most of the
/// light, and the shadows, are its.
const KEY_LIGHT: f32 = 9_000.0;
/// Where most of the room's light comes from in its photograph (its window), as the pipeline
/// reports it.
const ROOM_LIGHT_FROM: Vec3 = Vec3::new(0.242, 0.851, -0.467);
/// The dim room beyond the table, which the felt fades into with distance.
const ROOM_DARK: Color = Color::srgb(0.035, 0.045, 0.045);
/// The room's brightness, in cd/m² for a value of 1 in its photograph: the fill light. Low, so
/// that the key light's highlights and shadows stand apart.
const ROOM_INTENSITY: f32 = 220.0;

fn spawn_lights(mut commands: Commands) {
    commands.spawn((
        DirectionalLight {
            illuminance: KEY_LIGHT,
            shadow_maps_enabled: true,
            // The shadows the shadow map is too coarse for: under a stone, in a cut line.
            contact_shadows_enabled: true,
            // Bevy's default bias (2 cm) is larger than our pieces: at table scale it hides
            // every shadow they would cast.
            shadow_depth_bias: 0.002,
            ..default()
        },
        // Bevy's default cascades are spread over a scene a hundred metres deep; ours is a
        // table a metre or two from the camera, so put the shadow map's texels there.
        CascadeShadowConfigBuilder {
            num_cascades: 2,
            minimum_distance: 0.05,
            first_cascade_far_bound: 1.0,
            maximum_distance: 4.0,
            ..default()
        }
        .build(),
        Transform::from_translation(KEY_LIGHT_FROM).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// Turns the room about the vertical so its windows are on the key light's side: the light
/// reflected in a polished stone then agrees with the shadow it casts.
pub fn room_rotation() -> Quat {
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z).normalize();
    // Bevy looks the room up in the direction this rotation gives a world direction.
    Quat::from_rotation_arc(flat(KEY_LIGHT_FROM), flat(ROOM_LIGHT_FROM))
}

fn light_cameras(
    mut commands: Commands,
    server: Res<AssetServer>,
    cameras: Query<Entity, (With<Camera3d>, Without<EnvironmentMapLight>)>,
) {
    for camera in &cameras {
        commands.entity(camera).insert((
            EnvironmentMapLight {
                diffuse_map: server.load(assets::path("environment/room_diffuse.ktx2")),
                specular_map: server.load(assets::path("environment/room_specular.ktx2")),
                intensity: ROOM_INTENSITY,
                rotation: room_rotation(),
                ..default()
            },
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
                focal_distance: 0.75,
                aperture_f_stops: 4.0,
                ..default()
            },
            // The table fades into the room past arm's length, rather than ending.
            DistanceFog {
                color: ROOM_DARK,
                falloff: FogFalloff::Linear {
                    start: 1.2,
                    end: 4.0,
                },
                ..default()
            },
        ));
    }
}

/// Keeps the depth of field focused on what the orbit camera looks at.
fn focus_on_orbit(mut cameras: Query<(&OrbitCamera, &mut DepthOfField), Changed<OrbitCamera>>) {
    for (orbit, mut dof) in &mut cameras {
        dof.focal_distance = orbit.radius;
    }
}

fn spawn_table(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Wide enough that its far edge is lost in the fog from any angle the camera takes. Felt, on
    // which the board stands out.
    let mesh = look::world_uvs(
        Plane3d::default().mesh().size(12.0, 12.0).build(),
        Vec3::ZERO,
    );
    commands.spawn((
        Name::new("Table"),
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(look::table().build(&server))),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rooms_windows_are_turned_to_the_key_lights_side() {
        let r = room_rotation();
        let key = Vec3::new(KEY_LIGHT_FROM.x, 0.0, KEY_LIGHT_FROM.z).normalize();
        let room = Vec3::new(ROOM_LIGHT_FROM.x, 0.0, ROOM_LIGHT_FROM.z).normalize();
        // Looking towards the key light, Bevy reads the room where its windows are.
        assert!((r * key).distance(room) < 1e-5);
        assert!((r * Vec3::Y).distance(Vec3::Y) < 1e-5, "up stays up");
    }
}
