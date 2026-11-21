//! A row of buttons choosing where the table stands (`environment::Chosen`), at the bottom right
//! above the other stopgap rows. A stopgap until the interface has a place for settings like this
//! one. The choice is kept in `config.toml` for the next run.

use bevy::picking::Pickable;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button, observe};

use crate::browser::colours;
use crate::environment::{Chosen, Environments};
use crate::fonts::UiFonts;
use crate::net::{Net, Request};

pub struct EnvironmentChoicePlugin;

impl Plugin for EnvironmentChoicePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_buttons)
            .add_systems(Update, colour_buttons);
    }
}

/// A button that takes the table to this environment, by id.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
struct PlaceButton(String);

fn spawn_buttons(mut commands: Commands, fonts: Res<UiFonts>, all: Res<Environments>) {
    commands
        .spawn((
            crate::login::MainOnly,
            Node {
                position_type: PositionType::Absolute,
                right: px(12),
                bottom: px(98),
                column_gap: px(6),
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|row| {
            row.spawn((
                Text::new("Place"),
                fonts.regular(14.0),
                TextColor(colours::MUTED),
                Pickable::IGNORE,
            ));
            for environment in &all.0 {
                row.spawn((
                    Button,
                    PlaceButton(environment.id.clone()),
                    Hovered::default(),
                    Node {
                        padding: UiRect::axes(px(10), px(4)),
                        border_radius: BorderRadius::all(px(6)),
                        ..default()
                    },
                    BackgroundColor(colours::BUTTON),
                    observe(choose),
                    children![(
                        Text::new(environment.name.clone()),
                        fonts.semibold(13.0),
                        TextColor(colours::TEXT),
                        // Clicks land on the button, not its label.
                        Pickable::IGNORE,
                    )],
                ));
            }
        });
}

fn choose(
    activate: On<Activate>,
    buttons: Query<&PlaceButton>,
    mut chosen: ResMut<Chosen>,
    net: Option<Res<Net>>,
) {
    if let Ok(button) = buttons.get(activate.entity)
        && chosen.0 != button.0
    {
        chosen.0 = button.0.clone();
        if let Some(net) = net {
            net.send(Request::RememberEnvironment(button.0.clone()));
        }
    }
}

fn colour_buttons(
    chosen: Res<Chosen>,
    mut buttons: Query<(&Hovered, &PlaceButton, &mut BackgroundColor)>,
) {
    for (hovered, button, mut background) in &mut buttons {
        let colour = match (button.0 == chosen.0, hovered.0) {
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
    fn a_button_chooses_its_place_and_shows_it_is_chosen() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Chosen>()
            .add_systems(Update, colour_buttons);
        let garden = app
            .world_mut()
            .spawn((
                PlaceButton("garden".into()),
                Hovered::default(),
                BackgroundColor(colours::BUTTON),
                observe(choose),
            ))
            .id();
        app.world_mut().trigger(Activate { entity: garden });
        app.update();
        assert_eq!(app.world().resource::<Chosen>().0, "garden");
        assert_eq!(
            app.world().get::<BackgroundColor>(garden).unwrap().0,
            colours::CHOSEN
        );
    }
}
