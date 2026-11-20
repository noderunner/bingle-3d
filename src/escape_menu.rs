//! The menu Esc opens over a game: *Resume*, *Main Menu* (back to the main screen; the game goes
//! on at the server, as the game panel's *Back* leaves it), *Settings*, *Game Options* and
//! *Profile* (not built yet, shown greyed), and *Quit* (to the desktop).
//!
//! While it is open it covers the window: a click reaches neither the board nor the panels, and
//! the move keys and the camera's keys stand aside. Esc closes it again; a text field that has
//! the keyboard gives it up to the first Esc instead.

use bevy::input_focus::InputFocus;
use bevy::picking::Pickable;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button, observe};

use crate::browser::{Action, colours};
use crate::fonts::UiFonts;
use crate::net::{Net, ServerStatus, WatchStatus, reconnect, stop_watching};

pub struct EscapeMenuPlugin;

impl Plugin for EscapeMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EscapeMenu>()
            .add_systems(Startup, spawn_menu)
            .add_systems(
                Update,
                (
                    toggle_on_escape,
                    close_when_not_watching.run_if(resource_changed::<WatchStatus>),
                    show.run_if(resource_changed::<EscapeMenu>),
                    colour_items,
                )
                    .chain(),
            );
    }
}

/// Whether the menu is open.
#[derive(Resource, Debug, Default)]
pub struct EscapeMenu {
    pub open: bool,
}

/// Whether the menu is closed, for what stands aside while it is open (`play.rs`, the camera);
/// true where there is no menu at all, as in a test's app.
pub fn closed(menu: Option<&EscapeMenu>) -> bool {
    !menu.is_some_and(|m| m.open)
}

/// What the menu's items do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    Resume,
    MainMenu,
    Quit,
}

/// The menu's items, in order: a name, and what it does (`None`: not built yet, greyed).
const ITEMS: [(&str, Option<MenuAction>); 6] = [
    ("Resume", Some(MenuAction::Resume)),
    ("Main Menu", Some(MenuAction::MainMenu)),
    ("Settings", None),
    ("Game Options", None),
    ("Profile", None),
    ("Quit", Some(MenuAction::Quit)),
];

#[derive(Component)]
struct MenuOverlay;

/// An item that can be chosen (the others are greyed).
#[derive(Component)]
struct MenuItem;

fn spawn_menu(mut commands: Commands, fonts: Res<UiFonts>) {
    let f = &*fonts;
    commands
        .spawn((
            MenuOverlay,
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: px(0),
                bottom: px(0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            // The table dimmed behind it; the overlay takes every click outside the menu.
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            GlobalZIndex(100),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: px(300),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(20)),
                        row_gap: px(10),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(12)),
                        ..default()
                    },
                    // Opaque: blended in linear light, even a little see-through shows the
                    // table plainly.
                    BackgroundColor(Color::srgb(0.07, 0.08, 0.10)),
                    BorderColor::all(colours::BORDER),
                ))
                .with_children(|menu| {
                    menu.spawn((
                        Text::new("Menu"),
                        f.semibold(22.0),
                        TextColor(colours::TEXT),
                        Node {
                            margin: UiRect::bottom(px(6)),
                            ..default()
                        },
                    ));
                    for (label, action) in ITEMS {
                        match action {
                            Some(action) => {
                                menu.spawn(item(f, label, action));
                            }
                            None => {
                                menu.spawn(greyed(f, label));
                            }
                        }
                    }
                });
        });
}

fn item_node() -> Node {
    Node {
        padding: UiRect::axes(px(14), px(9)),
        border_radius: BorderRadius::all(px(6)),
        justify_content: JustifyContent::Center,
        ..default()
    }
}

fn item(f: &UiFonts, label: &str, action: MenuAction) -> impl Bundle {
    (
        MenuItem,
        Button,
        Action::Menu(action),
        Hovered::default(),
        item_node(),
        BackgroundColor(colours::BUTTON),
        observe(on_item),
        children![(
            Text::new(label),
            f.semibold(16.0),
            TextColor(colours::TEXT),
            Pickable::IGNORE,
        )],
    )
}

/// An item not built yet: shown, and greyed.
fn greyed(f: &UiFonts, label: &str) -> impl Bundle {
    (
        item_node(),
        BackgroundColor(colours::BLANK),
        children![(
            Text::new(label),
            f.semibold(16.0),
            TextColor(colours::DISABLED),
            Pickable::IGNORE,
        )],
    )
}

fn on_item(
    activate: On<Activate>,
    actions: Query<&Action>,
    net: Res<Net>,
    mut menu: ResMut<EscapeMenu>,
    mut status: ResMut<ServerStatus>,
    mut watching: ResMut<WatchStatus>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(Action::Menu(action)) = actions.get(activate.entity) else {
        return;
    };
    menu.open = false;
    match action {
        MenuAction::Resume => {}
        MenuAction::MainMenu => {
            // As the game panel's Back: stop watching, and read the server's lists again.
            stop_watching(&net, &mut watching);
            reconnect(&net, &mut status);
        }
        MenuAction::Quit => {
            exit.write(AppExit::Success);
        }
    }
}

/// Esc opens the menu over a game and closes it again. A text field that has the keyboard is
/// let go of first.
fn toggle_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    watching: Res<WatchStatus>,
    focus: Option<ResMut<InputFocus>>,
    mut menu: ResMut<EscapeMenu>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    if let Some(mut focus) = focus
        && focus.get().is_some()
    {
        focus.clear();
        return;
    }
    if menu.open || watching.is_watching() {
        menu.open = !menu.open;
    }
}

fn close_when_not_watching(watching: Res<WatchStatus>, mut menu: ResMut<EscapeMenu>) {
    if !watching.is_watching() && menu.open {
        menu.open = false;
    }
}

fn show(menu: Res<EscapeMenu>, mut overlay: Single<&mut Node, With<MenuOverlay>>) {
    overlay.display = if menu.open {
        Display::Flex
    } else {
        Display::None
    };
}

type HoverChanged = (With<MenuItem>, Changed<Hovered>);

fn colour_items(mut items: Query<(&Hovered, &mut BackgroundColor), HoverChanged>) {
    for (hovered, mut bg) in &mut items {
        bg.0 = if hovered.0 {
            colours::BUTTON_HOVER
        } else {
            colours::BUTTON
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::Request;

    fn app() -> (App, std::sync::mpsc::Receiver<Request>) {
        let (net, requests) = Net::for_test();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(net)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<WatchStatus>()
            .init_resource::<EscapeMenu>()
            .insert_resource(ServerStatus {
                server: "http://127.0.0.1:8080".into(),
                connection: crate::net::Connection::Connecting,
                notices: vec![],
                retry_in: None,
            })
            .add_message::<AppExit>()
            .add_systems(
                Update,
                (
                    toggle_on_escape,
                    close_when_not_watching.run_if(resource_changed::<WatchStatus>),
                )
                    .chain(),
            );
        (app, requests)
    }

    fn press_escape(app: &mut App) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(KeyCode::Escape);
        keys.clear();
    }

    fn watch_a_room(app: &mut App) {
        app.world_mut().resource_scope(|world, net: Mut<Net>| {
            let mut watching = world.resource_mut::<WatchStatus>();
            crate::net::watch(
                &net,
                &mut watching,
                crate::net::Target::Room("ABC123".into()),
            );
        });
    }

    fn open(app: &App) -> bool {
        app.world().resource::<EscapeMenu>().open
    }

    fn choose(app: &mut App, action: MenuAction) {
        let item = app
            .world_mut()
            .spawn(Action::Menu(action))
            .observe(on_item)
            .id();
        app.world_mut().trigger(Activate { entity: item });
    }

    #[test]
    fn escape_opens_the_menu_over_a_game_and_closes_it() {
        let (mut app, _requests) = app();
        press_escape(&mut app);
        assert!(!open(&app), "nothing to open it over on the main screen");
        watch_a_room(&mut app);
        press_escape(&mut app);
        assert!(open(&app));
        assert!(!closed(Some(app.world().resource::<EscapeMenu>())));
        press_escape(&mut app);
        assert!(!open(&app));
        assert!(closed(None), "an app with no menu is never held up by one");
    }

    #[test]
    fn escape_lets_go_of_a_text_field_first() {
        let (mut app, _requests) = app();
        watch_a_room(&mut app);
        let field = app.world_mut().spawn_empty().id();
        app.insert_resource(InputFocus::from_entity(field));
        press_escape(&mut app);
        assert!(!open(&app));
        assert_eq!(app.world().resource::<InputFocus>().get(), None);
        press_escape(&mut app);
        assert!(open(&app));
    }

    #[test]
    fn resume_closes_the_menu_and_the_game_goes_on() {
        let (mut app, requests) = app();
        watch_a_room(&mut app);
        app.world_mut().resource_mut::<EscapeMenu>().open = true;
        requests.try_iter().count();
        choose(&mut app, MenuAction::Resume);
        assert!(!open(&app));
        assert!(app.world().resource::<WatchStatus>().is_watching());
        assert!(
            requests.try_recv().is_err(),
            "nothing is asked of the server"
        );
    }

    #[test]
    fn main_menu_leaves_the_game_for_the_main_screen() {
        let (mut app, requests) = app();
        watch_a_room(&mut app);
        app.world_mut().resource_mut::<EscapeMenu>().open = true;
        requests.try_iter().count();
        choose(&mut app, MenuAction::MainMenu);
        assert!(!open(&app));
        assert!(!app.world().resource::<WatchStatus>().is_watching());
        let asked: Vec<Request> = requests.try_iter().collect();
        assert!(
            asked.contains(&Request::Connect),
            "the lists are read again: {asked:?}"
        );
    }

    #[test]
    fn quit_ends_the_app() {
        let (mut app, _requests) = app();
        watch_a_room(&mut app);
        choose(&mut app, MenuAction::Quit);
        app.update();
        assert_eq!(app.should_exit(), Some(AppExit::Success));
    }

    #[test]
    fn the_menu_closes_when_the_game_is_left_another_way() {
        let (mut app, _requests) = app();
        watch_a_room(&mut app);
        app.world_mut().resource_mut::<EscapeMenu>().open = true;
        app.update();
        app.world_mut().resource_scope(|world, net: Mut<Net>| {
            stop_watching(&net, &mut world.resource_mut::<WatchStatus>());
        });
        app.update();
        assert!(!open(&app));
    }

    #[test]
    fn three_items_are_not_built_yet() {
        let greyed: Vec<&str> = ITEMS
            .iter()
            .filter(|(_, a)| a.is_none())
            .map(|(n, _)| *n)
            .collect();
        assert_eq!(greyed, ["Settings", "Game Options", "Profile"]);
    }
}
