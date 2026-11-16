//! The placeholder scene for stage 1: a table, a 7×7 board and a few pieces, lit with shadows.
//!
//! One unit is one metre and the table top is at y = 0, so the board is the size of a real one.
//! Stage 3 replaces the fixed board with one built from a game definition.

use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::srgb(0.08, 0.09, 0.11)))
            .add_systems(Startup, (spawn_lights, spawn_table, spawn_board));
    }
}

/// A board's squares and where they sit on the table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoardLayout {
    pub cols: u32,
    pub rows: u32,
    /// Centre to centre, in metres.
    pub pitch: f32,
}

impl BoardLayout {
    /// The board's width and depth.
    pub fn size(&self) -> Vec2 {
        Vec2::new(self.cols as f32, self.rows as f32) * self.pitch
    }

    /// The centre of a square on the table plane, with the whole board centred on the origin.
    /// Column 0 is on the left (−X) and row 0 nearest the default camera (+Z).
    pub fn centre(&self, col: u32, row: u32) -> Vec3 {
        let half = (Vec2::new(self.cols as f32, self.rows as f32) - 1.0) / 2.0;
        Vec3::new(
            (col as f32 - half.x) * self.pitch,
            0.0,
            (half.y - row as f32) * self.pitch,
        )
    }

    /// Every square's column, row and centre, row by row.
    pub fn squares(&self) -> impl Iterator<Item = (u32, u32, Vec3)> + '_ {
        (0..self.rows)
            .flat_map(move |row| (0..self.cols).map(move |col| (col, row, self.centre(col, row))))
    }
}

const BOARD: BoardLayout = BoardLayout {
    cols: 7,
    rows: 7,
    pitch: 0.05,
};
const BOARD_BASE_HEIGHT: f32 = 0.02;
const BOARD_MARGIN: f32 = 0.02;
const TILE_HEIGHT: f32 = 0.003;
const TILE_GAP: f32 = 0.003;

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

fn spawn_board(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let size = BOARD.size() + 2.0 * BOARD_MARGIN;
    let base = commands
        .spawn((
            Name::new("Board"),
            Mesh3d(meshes.add(Cuboid::new(size.x, BOARD_BASE_HEIGHT, size.y))),
            MeshMaterial3d(materials.add(Color::srgb(0.55, 0.42, 0.28))),
            Transform::from_xyz(0.0, BOARD_BASE_HEIGHT / 2.0, 0.0),
        ))
        .id();

    let tile = meshes.add(Cuboid::new(
        BOARD.pitch - TILE_GAP,
        TILE_HEIGHT,
        BOARD.pitch - TILE_GAP,
    ));
    let light = materials.add(Color::srgb(0.86, 0.80, 0.68));
    let dark = materials.add(Color::srgb(0.42, 0.30, 0.20));
    let tile_y = BOARD_BASE_HEIGHT / 2.0 + TILE_HEIGHT / 2.0;
    commands.entity(base).with_children(|board| {
        for (col, row, centre) in BOARD.squares() {
            let material = if (col + row) % 2 == 0 { &dark } else { &light };
            board.spawn((
                Mesh3d(tile.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(centre + Vec3::Y * tile_y),
            ));
        }
    });

    // A few pieces in two seats' colours, and one stack, to show depth, shadows and stacking.
    let surface = BOARD_BASE_HEIGHT + TILE_HEIGHT;
    let stone = meshes.add(Cylinder::new(0.02, 0.012));
    let flat = meshes.add(Cuboid::new(0.036, 0.008, 0.036));
    let seat_a = materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.20, 0.18),
        perceptual_roughness: 0.4,
        ..default()
    });
    let seat_b = materials.add(StandardMaterial {
        base_color: Color::srgb(0.16, 0.32, 0.80),
        perceptual_roughness: 0.4,
        ..default()
    });
    for (col, row, material) in [
        (3, 3, &seat_a),
        (2, 4, &seat_b),
        (4, 2, &seat_a),
        (1, 1, &seat_b),
    ] {
        commands.spawn((
            Name::new("Stone"),
            Mesh3d(stone.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(BOARD.centre(col, row) + Vec3::Y * (surface + 0.006)),
        ));
    }
    for (i, material) in [&seat_a, &seat_b, &seat_a].into_iter().enumerate() {
        commands.spawn((
            Name::new("Stacked piece"),
            Mesh3d(flat.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(
                BOARD.centre(5, 5) + Vec3::Y * (surface + 0.004 + 0.0085 * i as f32),
            ),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-6;

    #[test]
    fn the_board_is_centred_on_the_origin() {
        let sum: Vec3 = BOARD.squares().map(|(_, _, c)| c).sum();
        assert!(sum.length() < EPS);
        assert!(
            BOARD.centre(3, 3).length() < EPS,
            "a 7×7 board's middle square is at 0"
        );
    }

    #[test]
    fn neighbours_are_one_pitch_apart() {
        let layout = BoardLayout {
            cols: 4,
            rows: 3,
            pitch: 0.1,
        };
        let d_col = layout.centre(1, 0) - layout.centre(0, 0);
        let d_row = layout.centre(0, 1) - layout.centre(0, 0);
        assert!(
            (d_col - Vec3::new(0.1, 0.0, 0.0)).length() < EPS,
            "columns run to +X"
        );
        assert!(
            (d_row - Vec3::new(0.0, 0.0, -0.1)).length() < EPS,
            "rows run away from the viewer"
        );
        assert_eq!(layout.squares().count(), 12);
        assert_eq!(layout.size(), Vec2::new(0.4, 0.3));
    }

    #[test]
    fn the_placeholder_board_is_table_sized() {
        let size = BOARD.size();
        assert!((0.2..0.6).contains(&size.x) && (0.2..0.6).contains(&size.y));
    }
}
