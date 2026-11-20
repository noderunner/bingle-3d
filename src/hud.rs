//! Text over the scene: how to move the camera, and the frame rate in debug builds.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::picking::Pickable;
use bevy::prelude::*;

use crate::fonts::UiFonts;

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

const HINT: &str = "Play: click, Enter, Backspace  ·  Menu: Esc  ·  Orbit: right-drag or arrows  ·  \
                    Pan: middle-drag or W A S D  ·  Zoom: wheel  ·  Coordinates: C  ·  Legal moves: L  ·  \
                    Reconnect: F5";

fn spawn_hud(mut commands: Commands, fonts: Res<UiFonts>) {
    let font = fonts.regular(15.0);
    let colour = TextColor(Color::srgba(1.0, 1.0, 1.0, 0.7));
    commands.spawn((
        Text::new(HINT),
        font.clone(),
        colour,
        // Only words: the wheel and clicks go through to the table.
        Pickable::IGNORE,
        Node {
            position_type: PositionType::Absolute,
            left: px(16),
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
            Pickable::IGNORE,
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
