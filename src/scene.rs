//! The room: a table, lit with shadows. The board of the game being watched stands on it
//! (`board.rs`).
//!
//! One unit is one metre and the table top is at y = 0, so a board is the size of a real one.

use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::srgb(0.08, 0.09, 0.11)))
            .add_systems(Startup, (spawn_lights, spawn_table));
    }
}

fn spawn_lights(mut commands: Commands) {
    commands.spawn((
        DirectionalLight {
            illuminance: 8_000.0,
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
        Transform::from_xyz(0.6, 1.2, 0.4).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        brightness: 300.0,
        ..default()
    });
}

fn spawn_table(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Name::new("Table"),
        Mesh3d(meshes.add(Plane3d::default().mesh().size(2.0, 2.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.32, 0.22, 0.15),
            perceptual_roughness: 0.8,
            ..default()
        })),
    ));
}
