//! The room: a wooden table, lit by the light of a real room and a key light that casts the
//! shadows. The board of the game being watched stands on it (`board.rs`).
//!
//! The room's light is an HDR photograph of a room, filtered by the asset pipeline into the two
//! cubemaps Bevy's `EnvironmentMapLight` reads (`assets-src/blender/environment.py`): the soft
//! light from every side, and what polished things reflect. Every 3D camera gets it.
//!
//! One unit is one metre and the table top is at y = 0, so a board is the size of a real one.

use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;

use crate::{assets, look};

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::srgb(0.08, 0.09, 0.11)))
            // The room lights everything; there is no light from nowhere as well.
            .insert_resource(GlobalAmbientLight {
                brightness: 0.0,
                ..default()
            })
            .add_systems(Startup, (spawn_lights, spawn_table))
            .add_systems(Update, light_cameras);
    }
}

/// Where the key light comes from: above, to the right and a little towards the player.
const KEY_LIGHT_FROM: Vec3 = Vec3::new(0.6, 1.2, 0.4);
/// Where most of the room's light comes from in its photograph (its windows), as the pipeline
/// reports it.
const ROOM_LIGHT_FROM: Vec3 = Vec3::new(0.427, 0.238, -0.872);
/// The room's brightness, in cd/m² for a value of 1 in its photograph.
const ROOM_INTENSITY: f32 = 800.0;

fn spawn_lights(mut commands: Commands) {
    commands.spawn((
        DirectionalLight {
            illuminance: 5_000.0,
            shadow_maps_enabled: true,
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
        commands.entity(camera).insert(EnvironmentMapLight {
            diffuse_map: server.load(assets::path("environment/loft_hall_diffuse.ktx2")),
            specular_map: server.load(assets::path("environment/loft_hall_specular.ktx2")),
            intensity: ROOM_INTENSITY,
            rotation: room_rotation(),
            ..default()
        });
    }
}

fn spawn_table(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Wide enough that its far edge is not seen from any angle the camera takes.
    let mesh = look::world_uvs(Plane3d::default().mesh().size(6.0, 6.0).build(), Vec3::ZERO);
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
