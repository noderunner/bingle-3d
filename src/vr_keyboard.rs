//! A keyboard in the world, for a headset (`vr.rs`): it stands before the player while a
//! field that takes typing has the keyboard focus, and its keys are pressed with a
//! controller's beam.
//!
//! A key is sent as the real keyboard's would be: a `KeyboardInput` message, which Bevy hands
//! to the field with the focus. So it types into every field the window has (the login
//! screen's, the main screen's, the friends panel's, the masked password), with no field
//! knowing. Pressing a key does not take the focus from the field, as a click elsewhere would.
//! *Hide* lets go of the field, and the keyboard goes.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput, NativeKeyCode};
use bevy::input_focus::{AcquireFocus, InputFocus};
use bevy::picking::Pickable;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::{Activate, Button, observe};
use bevy::window::PrimaryWindow;

use crate::browser::colours;
use crate::fonts::UiFonts;
use crate::sign_in::Field;
use crate::vr_panels::InHeadset;
use crate::world_panel::{self, Hung, Look};

pub struct VrKeyboardPlugin;

impl Plugin for VrKeyboardPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Shifted>()
            .add_systems(Startup, spawn_keyboard)
            .add_systems(Update, (show, relabel, colour_keys));
    }
}

/// What a key is.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum Pressed {
    /// Types the first, or with Shift the second.
    Char(&'static str, &'static str),
    Backspace,
    Enter,
    Tab,
    Space,
    /// The next key typed is the shifted one.
    Shift,
    /// Lets go of the field: the keyboard goes.
    Hide,
}

use Pressed::*;

const fn c(plain: &'static str, shifted: &'static str) -> Pressed {
    Char(plain, shifted)
}

/// The keys, row by row, with how wide each is (1 is a letter's width). Letters shift to
/// capitals; the rest to what a US keyboard has over them, and what a server's address, an
/// e-mail address or a password needs is on the face of a key or under Shift.
const ROWS: &[&[(Pressed, f32)]] = &[
    &[
        (c("1", "!"), 1.0),
        (c("2", "@"), 1.0),
        (c("3", "#"), 1.0),
        (c("4", "$"), 1.0),
        (c("5", "%"), 1.0),
        (c("6", "^"), 1.0),
        (c("7", "&"), 1.0),
        (c("8", "*"), 1.0),
        (c("9", "("), 1.0),
        (c("0", ")"), 1.0),
        (c("-", "_"), 1.0),
        (Backspace, 1.6),
    ],
    &[
        (Tab, 1.3),
        (c("q", "Q"), 1.0),
        (c("w", "W"), 1.0),
        (c("e", "E"), 1.0),
        (c("r", "R"), 1.0),
        (c("t", "T"), 1.0),
        (c("y", "Y"), 1.0),
        (c("u", "U"), 1.0),
        (c("i", "I"), 1.0),
        (c("o", "O"), 1.0),
        (c("p", "P"), 1.0),
        (c("=", "+"), 1.0),
    ],
    &[
        (c("a", "A"), 1.0),
        (c("s", "S"), 1.0),
        (c("d", "D"), 1.0),
        (c("f", "F"), 1.0),
        (c("g", "G"), 1.0),
        (c("h", "H"), 1.0),
        (c("j", "J"), 1.0),
        (c("k", "K"), 1.0),
        (c("l", "L"), 1.0),
        (c(":", ";"), 1.0),
        (Enter, 1.8),
    ],
    &[
        (Shift, 1.6),
        (c("z", "Z"), 1.0),
        (c("x", "X"), 1.0),
        (c("c", "C"), 1.0),
        (c("v", "V"), 1.0),
        (c("b", "B"), 1.0),
        (c("n", "N"), 1.0),
        (c("m", "M"), 1.0),
        (c(".", ","), 1.0),
        (c("/", "?"), 1.0),
        (c("@", "'"), 1.0),
    ],
    &[
        (Hide, 2.0),
        (Space, 6.0),
        (c("[", "{"), 1.0),
        (c("]", "}"), 1.0),
    ],
];

impl Pressed {
    /// What the key shows.
    fn label(self, shifted: bool) -> &'static str {
        match self {
            Char(plain, upper) => {
                if shifted {
                    upper
                } else {
                    plain
                }
            }
            Backspace => "⌫",
            Enter => "Enter",
            Tab => "Tab",
            Space => "",
            Shift => "Shift",
            Hide => "Hide",
        }
    }

    /// What the real keyboard would send for it; `None` for the keys that are this
    /// keyboard's own.
    fn input(self, shifted: bool) -> Option<(Key, KeyCode, Option<&'static str>)> {
        Some(match self {
            Char(..) => {
                let text = self.label(shifted);
                (
                    Key::Character(text.into()),
                    // Where the key is on the real keyboard is not known, nor needed: the
                    // fields read what it types.
                    KeyCode::Unidentified(NativeKeyCode::Unidentified),
                    Some(text),
                )
            }
            Space => (Key::Space, KeyCode::Space, Some(" ")),
            Backspace => (Key::Backspace, KeyCode::Backspace, None),
            Enter => (Key::Enter, KeyCode::Enter, None),
            Tab => (Key::Tab, KeyCode::Tab, None),
            Shift | Hide => return None,
        })
    }
}

/// Whether the next key is shifted.
#[derive(Resource, Debug, Default)]
struct Shifted(bool);

/// The keyboard hung in the world.
#[derive(Resource)]
struct KeyboardInWorld(Hung);

/// A key's label.
#[derive(Component)]
struct Label(Pressed);

/// The keyboard: 0.62 by 0.25 metres.
const KEYBOARD: Look = Look {
    size: UVec2::new(780, 316),
    scale: 2.0,
    metres: 0.0008,
    background: Color::srgb(0.07, 0.08, 0.10),
};
/// A letter key's width and height, and the gap between keys, in the UI's pixels.
const KEY: f32 = 56.0;
const GAP: f32 = 6.0;

/// Where the keyboard stands: before a player sat at the table, low over its near edge, tilted
/// to face their eyes.
fn before_player() -> Transform {
    let at = Vec3::new(0.0, 0.16, 0.36);
    // Where the eyes of someone sat at the table are, about (`vr.rs`).
    let eyes = Vec3::new(0.0, 0.45, 0.75);
    Transform::from_translation(at).looking_to(at - eyes, Vec3::Y)
}

fn spawn_keyboard(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let hung = world_panel::hang(
        &mut commands,
        &mut images,
        &mut meshes,
        &mut materials,
        KEYBOARD,
    );
    commands.entity(hung.quad).insert(before_player());
    commands.insert_resource(KeyboardInWorld(hung));
    let f = &*fonts;
    commands
        .spawn((
            UiTargetCamera(hung.camera),
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(GAP),
                ..default()
            },
        ))
        // A key pressed is not a click away from the field: the field keeps the focus.
        .observe(|mut acquire: On<AcquireFocus>| acquire.propagate(false))
        .with_children(|keyboard| {
            for row in ROWS {
                keyboard
                    .spawn(Node {
                        column_gap: px(GAP),
                        ..default()
                    })
                    .with_children(|keys| {
                        for (key, width) in *row {
                            keys.spawn(key_button(f, *key, *width));
                        }
                    });
            }
        });
}

fn key_button(f: &UiFonts, key: Pressed, width: f32) -> impl Bundle {
    (
        key,
        Button,
        Hovered::default(),
        Node {
            width: px(KEY * width + GAP * (width - 1.0).max(0.0)),
            height: px(KEY),
            border_radius: BorderRadius::all(px(8)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(colours::BUTTON),
        observe(on_key),
        children![(
            Label(key),
            Text::new(key.label(false)),
            f.semibold(if matches!(key, Char(..)) { 26.0 } else { 20.0 }),
            TextColor(colours::TEXT),
            Pickable::IGNORE,
        )],
    )
}

/// The messages the real keyboard would send for a key, down and up.
fn messages(key: Pressed, shifted: bool, window: Entity) -> Vec<KeyboardInput> {
    let Some((logical_key, key_code, text)) = key.input(shifted) else {
        return vec![];
    };
    [ButtonState::Pressed, ButtonState::Released]
        .into_iter()
        .map(|state| KeyboardInput {
            key_code,
            logical_key: logical_key.clone(),
            state,
            text: (state == ButtonState::Pressed)
                .then_some(text)
                .flatten()
                .map(Into::into),
            repeat: false,
            window,
        })
        .collect()
}

fn on_key(
    activate: On<Activate>,
    keys: Query<&Pressed>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut shifted: ResMut<Shifted>,
    mut focus: ResMut<InputFocus>,
    mut typed: MessageWriter<KeyboardInput>,
) {
    let Ok(&key) = keys.get(activate.entity) else {
        return;
    };
    match key {
        Shift => shifted.0 = !shifted.0,
        Hide => {
            focus.clear();
            shifted.0 = false;
        }
        _ => {
            if let Ok(window) = windows.single() {
                typed.write_batch(messages(key, shifted.0, window));
            }
            // Shift is for one key.
            if shifted.0 && matches!(key, Char(..)) {
                shifted.0 = false;
            }
        }
    }
}

/// The fields that take typing: Bevy's, and the sign-in form's masked password.
type TypingFields<'w, 's> = Query<'w, 's, (), Or<(With<EditableText>, With<Field>)>>;

/// Whether the field with the focus takes typing.
fn typing_into(focus: &InputFocus, fields: &TypingFields) -> bool {
    focus.get().is_some_and(|field| fields.contains(field))
}

/// Shows the keyboard while in a headset and a field that takes typing has the focus.
fn show(
    in_headset: Option<Res<InHeadset>>,
    focus: Res<InputFocus>,
    fields: TypingFields,
    hung: Res<KeyboardInWorld>,
    mut cameras: Query<&mut Camera>,
    mut visibilities: Query<&mut Visibility>,
) {
    let shown = in_headset.is_some_and(|h| h.0) && typing_into(&focus, &fields);
    world_panel::show(hung.0, shown, &mut cameras, &mut visibilities);
}

fn relabel(shifted: Res<Shifted>, mut labels: Query<(&Label, &mut Text)>) {
    if !shifted.is_changed() {
        return;
    }
    for (label, mut text) in &mut labels {
        let wanted = label.0.label(shifted.0);
        if text.0 != wanted {
            text.0 = wanted.into();
        }
    }
}

fn colour_keys(shifted: Res<Shifted>, mut keys: Query<(&Pressed, &Hovered, &mut BackgroundColor)>) {
    for (key, hovered, mut bg) in &mut keys {
        let on = *key == Shift && shifted.0;
        let colour = match (on, hovered.0) {
            (true, false) => colours::CHOSEN,
            (true, true) => colours::CHOSEN_HOVER,
            (false, false) => colours::BUTTON,
            (false, true) => colours::BUTTON_HOVER,
        };
        if bg.0 != colour {
            bg.0 = colour;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::InputPlugin;
    use bevy::input_focus::InputDispatchPlugin;
    use bevy::ui_widgets::TextInput;

    #[test]
    fn a_letter_types_itself_and_shifted_its_capital() {
        let a = c("a", "A");
        let (key, _, text) = a.input(false).unwrap();
        assert_eq!((key, text), (Key::Character("a".into()), Some("a")));
        assert_eq!(a.input(true).unwrap().2, Some("A"));
        assert_eq!(Backspace.input(true).unwrap().0, Key::Backspace);
        assert_eq!(Space.input(false).unwrap().2, Some(" "));
        assert!(Shift.input(false).is_none() && Hide.input(false).is_none());
        // Down, with what it types, then up.
        let sent = messages(a, true, Entity::PLACEHOLDER);
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].state, ButtonState::Pressed);
        assert_eq!(sent[0].text.as_deref(), Some("A"));
        assert_eq!(
            (sent[1].state, sent[1].text.as_deref()),
            (ButtonState::Released, None)
        );
    }

    #[test]
    fn what_an_address_and_a_password_need_is_on_it() {
        let all: Vec<&str> = ROWS
            .iter()
            .flat_map(|row| row.iter())
            .flat_map(|(key, _)| [key.label(false), key.label(true)])
            .collect();
        for needed in [
            ":", "/", ".", "-", "_", "@", "!", "#", "?", "+", "=", "Q", "z", "0",
        ] {
            assert!(all.contains(&needed), "{needed}");
        }
    }

    #[test]
    fn keys_type_into_the_field_with_the_focus_and_shift_is_for_one_key() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, InputPlugin, InputDispatchPlugin))
            .init_resource::<InputFocus>()
            .init_resource::<Shifted>()
            .init_resource::<Typed>()
            .add_observer(record);
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        let field = app
            .world_mut()
            .spawn((EditableText::default(), TextInput))
            .id();
        app.insert_resource(InputFocus::from_entity(field));
        for key in [Shift, c("h", "H"), c("i", "I"), Backspace] {
            let entity = app.world_mut().spawn(key).observe(on_key).id();
            app.world_mut().trigger(Activate { entity });
            app.update();
        }
        let typed = &app.world().resource::<Typed>().0;
        assert_eq!(typed, &["H", "i", "Backspace"]);
        assert!(!app.world().resource::<Shifted>().0);

        // Hide lets go of the field.
        let hide = app.world_mut().spawn(Hide).observe(on_key).id();
        app.world_mut().trigger(Activate { entity: hide });
        app.update();
        assert_eq!(app.world().resource::<InputFocus>().get(), None);
    }

    /// What reached the field with the focus.
    #[derive(Resource, Default)]
    struct Typed(Vec<String>);

    fn record(input: On<bevy::input_focus::FocusedInput<KeyboardInput>>, mut typed: ResMut<Typed>) {
        if input.input.state != ButtonState::Pressed {
            return;
        }
        typed.0.push(match &input.input.logical_key {
            Key::Character(_) => input.input.text.as_deref().unwrap_or("").to_string(),
            other => format!("{other:?}"),
        });
    }
}
