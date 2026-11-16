//! Text over the scene: how to move the camera, and the frame rate in debug builds.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_hud);
        if cfg!(debug_assertions) {
            app.add_plugins(FrameTimeDiagnosticsPlugin::default())
                .add_systems(Update, update_fps);
        }
    }
}

#[derive(Component)]
struct FpsText;

// Bevy's built-in font is ASCII only, so no "·" or "–" here.
const HINT: &str = "Orbit: right-drag or arrow keys    Zoom: mouse wheel";

fn spawn_hud(mut commands: Commands) {
    let font = TextFont {
        font_size: FontSize::Px(16.0),
        ..default()
    };
    let colour = TextColor(Color::srgba(1.0, 1.0, 1.0, 0.7));
    commands.spawn((
        Text::new(HINT),
        font.clone(),
        colour,
        Node {
            position_type: PositionType::Absolute,
            left: px(12),
            bottom: px(10),
            ..default()
        },
    ));
    if cfg!(debug_assertions) {
        commands.spawn((
            FpsText,
            Text::new("- fps"),
            font,
            colour,
            Node {
                position_type: PositionType::Absolute,
                right: px(12),
                top: px(10),
                ..default()
            },
        ));
    }
}

fn update_fps(diagnostics: Res<DiagnosticsStore>, mut text: Single<&mut Text, With<FpsText>>) {
    if let Some(fps) = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
    {
        text.0 = format!("{fps:.0} fps");
    }
}
