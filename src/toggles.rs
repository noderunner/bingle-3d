//! Buttons and keys that turn parts of the table on and off, at the bottom right above the woods:
//! a stopgap until the interface has a place for settings like these.
//!
//! - Coordinates (C): the board's coordinates, engraved along its edges (`coordinates.rs`).
//! - Legal moves (L): lighting every position a click would be taken on (`board.rs`). Off by
//!   default: at the start of a game every position is one, and the whole board lights up.

use bevy::input_focus::InputFocus;
use bevy::picking::Pickable;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button, observe};

use crate::board::ShowLegalMoves;
use crate::browser::colours;
use crate::coordinates::ShowCoordinates;
use crate::fonts::UiFonts;

pub struct TogglesPlugin;

impl Plugin for TogglesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_buttons)
            .add_systems(Update, (toggle_with_keys, colour_buttons).chain());
    }
}

/// What a button or key turns on and off.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum Toggle {
    Coordinates,
    LegalMoves,
}

impl Toggle {
    const ALL: [Toggle; 2] = [Toggle::LegalMoves, Toggle::Coordinates];

    fn label(self) -> &'static str {
        match self {
            Toggle::Coordinates => "Coordinates",
            Toggle::LegalMoves => "Legal moves",
        }
    }

    fn key(self) -> KeyCode {
        match self {
            Toggle::Coordinates => KeyCode::KeyC,
            Toggle::LegalMoves => KeyCode::KeyL,
        }
    }
}

/// The settings the toggles turn on and off, as one system parameter.
#[derive(bevy::ecs::system::SystemParam)]
struct Settings<'w> {
    coordinates: ResMut<'w, ShowCoordinates>,
    legal_moves: ResMut<'w, ShowLegalMoves>,
}

impl Settings<'_> {
    fn is_on(&self, toggle: Toggle) -> bool {
        match toggle {
            Toggle::Coordinates => self.coordinates.0,
            Toggle::LegalMoves => self.legal_moves.0,
        }
    }

    fn flip(&mut self, toggle: Toggle) {
        match toggle {
            Toggle::Coordinates => self.coordinates.0 = !self.coordinates.0,
            Toggle::LegalMoves => self.legal_moves.0 = !self.legal_moves.0,
        }
    }
}

fn spawn_buttons(mut commands: Commands, fonts: Res<UiFonts>) {
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            right: px(12),
            bottom: px(66),
            column_gap: px(6),
            ..default()
        })
        .with_children(|row| {
            for toggle in Toggle::ALL {
                row.spawn((
                    Button,
                    toggle,
                    Hovered::default(),
                    Node {
                        padding: UiRect::axes(px(10), px(4)),
                        border_radius: BorderRadius::all(px(6)),
                        ..default()
                    },
                    BackgroundColor(colours::BUTTON),
                    observe(
                        |activate: On<Activate>,
                         toggles: Query<&Toggle>,
                         mut settings: Settings| {
                            if let Ok(toggle) = toggles.get(activate.entity) {
                                settings.flip(*toggle);
                            }
                        },
                    ),
                    children![(
                        Text::new(toggle.label()),
                        fonts.semibold(13.0),
                        TextColor(colours::TEXT),
                        Pickable::IGNORE,
                    )],
                ));
            }
        });
}

fn toggle_with_keys(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Option<Res<InputFocus>>,
    mut settings: Settings,
) {
    if focus.is_some_and(|f| f.get().is_some()) {
        return;
    }
    for toggle in Toggle::ALL {
        if keys.just_pressed(toggle.key()) {
            settings.flip(toggle);
        }
    }
}

fn colour_buttons(
    settings: Settings,
    mut buttons: Query<(&Toggle, &Hovered, &mut BackgroundColor)>,
) {
    for (toggle, hovered, mut background) in &mut buttons {
        let colour = match (settings.is_on(*toggle), hovered.0) {
            (true, false) => colours::CHOSEN,
            (true, true) => colours::CHOSEN_HOVER,
            (false, false) => colours::BUTTON,
            (false, true) => colours::BUTTON_HOVER,
        };
        if background.0 != colour {
            background.0 = colour;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<ShowCoordinates>()
            .init_resource::<ShowLegalMoves>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, toggle_with_keys);
        app
    }

    fn press(app: &mut App, key: KeyCode) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release_all();
        keys.clear();
        keys.press(key);
        app.update();
    }

    #[test]
    fn legal_moves_start_off_and_l_turns_them_on_and_off() {
        let mut app = app();
        app.update();
        assert!(!app.world().resource::<ShowLegalMoves>().0, "off at first");
        press(&mut app, KeyCode::KeyL);
        assert!(app.world().resource::<ShowLegalMoves>().0);
        press(&mut app, KeyCode::KeyL);
        assert!(!app.world().resource::<ShowLegalMoves>().0);
        assert!(
            !app.world().resource::<ShowCoordinates>().0,
            "the other untouched"
        );
    }

    #[test]
    fn c_shows_and_hides_the_coordinates() {
        let mut app = app();
        press(&mut app, KeyCode::KeyC);
        assert!(app.world().resource::<ShowCoordinates>().0);
        press(&mut app, KeyCode::KeyC);
        assert!(!app.world().resource::<ShowCoordinates>().0);
    }
}
