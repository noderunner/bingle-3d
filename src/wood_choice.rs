//! A row of buttons choosing the board's wood (`board::BoardWood`), at the bottom right above the
//! hint line. A stopgap until the interface has a place for settings like this one.

use bevy::picking::Pickable;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button, observe};

use crate::board::BoardWood;
use crate::browser::colours;
use crate::fonts::UiFonts;
use crate::look::BOARD_WOODS;

pub struct WoodChoicePlugin;

impl Plugin for WoodChoicePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_buttons)
            .add_systems(Update, colour_buttons);
    }
}

/// A button that makes the board this wood.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct WoodButton(&'static str);

fn spawn_buttons(mut commands: Commands, fonts: Res<UiFonts>) {
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            right: px(12),
            bottom: px(34),
            column_gap: px(6),
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new("Board"),
                fonts.regular(14.0),
                TextColor(colours::MUTED),
                Pickable::IGNORE,
            ));
            for wood in BOARD_WOODS {
                row.spawn((
                    Button,
                    WoodButton(wood),
                    Hovered::default(),
                    Node {
                        padding: UiRect::axes(px(10), px(4)),
                        border_radius: BorderRadius::all(px(6)),
                        ..default()
                    },
                    BackgroundColor(colours::BUTTON),
                    observe(choose),
                    children![(
                        Text::new(label(wood)),
                        fonts.semibold(13.0),
                        TextColor(colours::TEXT),
                        // Clicks land on the button, not its label.
                        Pickable::IGNORE,
                    )],
                ));
            }
        });
}

/// A wood's name as a button says it.
fn label(wood: &str) -> String {
    let mut c = wood.chars();
    c.next()
        .map(|first| first.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

fn choose(activate: On<Activate>, buttons: Query<&WoodButton>, mut wood: ResMut<BoardWood>) {
    if let Ok(button) = buttons.get(activate.entity)
        && wood.0 != button.0
    {
        wood.0 = button.0;
    }
}

fn colour_buttons(
    wood: Res<BoardWood>,
    mut buttons: Query<(&Hovered, &WoodButton, &mut BackgroundColor)>,
) {
    for (hovered, button, mut background) in &mut buttons {
        let colour = match (button.0 == wood.0, hovered.0) {
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

    #[test]
    fn a_button_chooses_its_wood_and_shows_it_is_chosen() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<BoardWood>()
            .add_systems(Update, colour_buttons);
        let oak = app
            .world_mut()
            .spawn((
                WoodButton("oak"),
                Hovered::default(),
                BackgroundColor(colours::BUTTON),
                observe(choose),
            ))
            .id();
        app.world_mut().trigger(Activate { entity: oak });
        app.update();
        assert_eq!(app.world().resource::<BoardWood>().0, "oak");
        assert_eq!(
            app.world().get::<BackgroundColor>(oak).unwrap().0,
            colours::CHOSEN
        );
    }

    #[test]
    fn a_wood_is_named_with_a_capital() {
        assert_eq!(label("walnut"), "Walnut");
        assert_eq!(label(""), "");
    }
}
