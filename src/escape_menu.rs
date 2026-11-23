//! The menu Esc opens over a game or the main screen: *Resume*, *Main Menu* (back to the main
//! screen from a game; the game goes on at the server, as the game panel's *Back* leaves it),
//! *Settings* (a page of its own: fullscreen or a window, and the resolution, `display.rs`),
//! *Game Options* and *Profile* (not built yet, shown greyed), *Logout* (back to the login
//! screen, the account's session ended, auto-login off) and *Quit* (to the desktop). On the main
//! screen *Main Menu* and *Game Options* are greyed too. Not on the login screen.
//!
//! While it is open it covers the window: a click reaches neither the board nor the panels, and
//! the move keys and the camera's keys stand aside. Esc closes it again (from *Settings*, back to
//! the menu first); a text field that has the keyboard gives it up to the first Esc instead.

use bevy::input_focus::InputFocus;
use bevy::picking::Pickable;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button, observe};

use bevy::window::{Monitor, OnMonitor, PrimaryMonitor, PrimaryWindow};

use crate::browser::{Action, colours};
use crate::display::{self, DisplaySettings};
use crate::fonts::UiFonts;
use crate::login::{self, LoginScreen, Screen};
use crate::net::{Connection, Net, Request, ServerStatus, WatchStatus, reconnect, stop_watching};
use crate::sign_in::Signing;

pub struct EscapeMenuPlugin;

impl Plugin for EscapeMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EscapeMenu>()
            .add_systems(Startup, spawn_menu)
            .add_systems(
                Update,
                (
                    toggle_on_escape,
                    close_off_the_main_screen.run_if(resource_changed::<Screen>),
                    show.run_if(
                        resource_changed::<EscapeMenu>
                            .or_eager(resource_changed::<WatchStatus>)
                            .or_eager(resource_exists_and_changed::<DisplaySettings>),
                    ),
                    colour_items,
                )
                    .chain(),
            );
    }
}

/// Whether the menu is open, and at which page.
#[derive(Resource, Debug, Default)]
pub struct EscapeMenu {
    pub open: bool,
    pub page: Page,
}

/// The menu's pages.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Menu,
    Settings,
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
    Settings,
    Logout,
    Quit,
    /// From *Settings* back to the menu.
    Back,
    /// Fullscreen, or a window.
    Fullscreen,
    /// A resolution, or none: the desktop's.
    Resolution(Option<(u32, u32)>),
}

/// The menu's items, in order: a name, and what it does (`None`: greyed). In a game, or on the
/// main screen.
fn items(in_game: bool) -> [(&'static str, Option<MenuAction>); 7] {
    [
        ("Resume", Some(MenuAction::Resume)),
        ("Main Menu", in_game.then_some(MenuAction::MainMenu)),
        ("Settings", Some(MenuAction::Settings)),
        ("Game Options", None),
        ("Profile", None),
        ("Logout", Some(MenuAction::Logout)),
        ("Quit", Some(MenuAction::Quit)),
    ]
}

/// What the menu's items hang from, built again as it opens.
#[derive(Component)]
struct MenuItems;

/// What covers the window while the menu is open, with the menu in its middle. A headset hangs
/// it in the world (`vr_panels.rs`).
#[derive(Component)]
pub struct MenuOverlay;

/// How the window is dimmed behind the menu.
pub const DIM: Color = Color::srgba(0.0, 0.0, 0.0, 0.55);

/// An item that can be chosen (the others are greyed), and whether it is the choice made
/// among others (a setting's).
#[derive(Component)]
struct MenuItem {
    chosen: bool,
}

fn spawn_menu(mut commands: Commands) {
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
            BackgroundColor(DIM),
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
                        MenuItems,
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: px(10),
                            ..default()
                        },
                    ));
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
    choice(f, label, action, false, item_node())
}

/// An item, shown as the choice made (`chosen`) or not, laid out as `node` says.
fn choice(f: &UiFonts, label: &str, action: MenuAction, chosen: bool, node: Node) -> impl Bundle {
    (
        MenuItem { chosen },
        Button,
        Action::Menu(action),
        Hovered::default(),
        node,
        BackgroundColor(if chosen {
            colours::CHOSEN
        } else {
            colours::BUTTON
        }),
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
    greyed_in(f, label, item_node())
}

fn greyed_in(f: &UiFonts, label: &str, node: Node) -> impl Bundle {
    (
        node,
        BackgroundColor(colours::BLANK),
        children![(
            Text::new(label),
            f.semibold(16.0),
            TextColor(colours::DISABLED),
            Pickable::IGNORE,
        )],
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer takes what it touches as parameters"
)]
fn on_item(
    activate: On<Activate>,
    actions: Query<&Action>,
    net: Res<Net>,
    mut menu: ResMut<EscapeMenu>,
    mut status: ResMut<ServerStatus>,
    mut watching: ResMut<WatchStatus>,
    mut exit: MessageWriter<AppExit>,
    (mut screen, mut login, mut signing): (ResMut<Screen>, ResMut<LoginScreen>, ResMut<Signing>),
    display: Option<ResMut<DisplaySettings>>,
) {
    let Ok(Action::Menu(action)) = actions.get(activate.entity) else {
        return;
    };
    // A setting is changed with the menu open, and kept for the next run.
    let set = |change: &dyn Fn(&mut DisplaySettings)| {
        if let Some(mut display) = display {
            change(&mut display);
            net.send(Request::RememberDisplay {
                fullscreen: display.fullscreen,
                resolution: display.resolution,
            });
        }
    };
    match action {
        MenuAction::Settings => menu.page = Page::Settings,
        MenuAction::Back => menu.page = Page::Menu,
        MenuAction::Fullscreen => set(&|d| d.fullscreen = !d.fullscreen),
        MenuAction::Resolution(size) => set(&|d| d.resolution = *size),
        _ => menu.open = false,
    }
    match action {
        MenuAction::Resume
        | MenuAction::Settings
        | MenuAction::Back
        | MenuAction::Fullscreen
        | MenuAction::Resolution(_) => {}
        MenuAction::MainMenu => {
            // As the game panel's Back: stop watching, and read the server's lists again.
            stop_watching(&net, &mut watching);
            reconnect(&net, &mut status);
        }
        MenuAction::Logout => {
            if watching.is_watching() {
                stop_watching(&net, &mut watching);
            }
            if matches!(&status.connection, Connection::Ready(snap) if snap.account.is_some()) {
                net.send(Request::SignOut);
            }
            // Logged out, the next start asks again.
            net.send(Request::Remember {
                server: false,
                auto_login: false,
            });
            login.auto_login = false;
            login::go_to_login(&net, &mut screen, &mut login, &mut status, &mut signing);
        }
        MenuAction::Quit => {
            exit.write(AppExit::Success);
        }
    }
}

/// Esc opens the menu, over a game or the main screen, and closes it again. A text field that
/// has the keyboard is let go of first. Not on the login screen.
fn toggle_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    screen: Option<Res<Screen>>,
    focus: Option<ResMut<InputFocus>>,
    mut menu: ResMut<EscapeMenu>,
) {
    if !keys.just_pressed(KeyCode::Escape) || screen.as_deref() == Some(&Screen::Login) {
        return;
    }
    if let Some(mut focus) = focus
        && focus.get().is_some()
    {
        focus.clear();
        return;
    }
    if menu.open && menu.page == Page::Settings {
        menu.page = Page::Menu;
        return;
    }
    menu.open = !menu.open;
    menu.page = Page::Menu;
}

fn close_off_the_main_screen(screen: Res<Screen>, mut menu: ResMut<EscapeMenu>) {
    if *screen == Screen::Login && menu.open {
        menu.open = false;
    }
}

/// Shows the menu, with its items for where it opens, or its settings.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system takes what it reads as parameters"
)]
fn show(
    mut commands: Commands,
    menu: Res<EscapeMenu>,
    watching: Res<WatchStatus>,
    fonts: Res<UiFonts>,
    display: Option<Res<DisplaySettings>>,
    window: Query<Option<&OnMonitor>, With<PrimaryWindow>>,
    monitors: Query<(&Monitor, Has<PrimaryMonitor>)>,
    mut overlay: Single<&mut Node, With<MenuOverlay>>,
    list: Single<Entity, With<MenuItems>>,
) {
    overlay.display = if menu.open {
        Display::Flex
    } else {
        Display::None
    };
    if !menu.open {
        return;
    }
    let f = &*fonts;
    let title = |list: &mut ChildSpawnerCommands, text: &str| {
        list.spawn((
            Text::new(text),
            f.semibold(22.0),
            TextColor(colours::TEXT),
            Node {
                margin: UiRect::bottom(px(6)),
                ..default()
            },
        ));
    };
    if menu.page == Page::Settings {
        let display = display.as_deref().copied().unwrap_or(DisplaySettings {
            fullscreen: true,
            resolution: None,
        });
        let monitor = display::current_monitor(&window, &monitors);
        commands
            .entity(*list)
            .despawn_related::<Children>()
            .with_children(|list| {
                title(list, "Settings");
                settings(list, f, display, monitor);
                list.spawn(item(f, "Back", MenuAction::Back));
            });
        return;
    }
    commands
        .entity(*list)
        .despawn_related::<Children>()
        .with_children(|list| {
            title(list, "Menu");
            for (label, action) in items(watching.is_watching()) {
                match action {
                    Some(action) => {
                        list.spawn(item(f, label, action));
                    }
                    None => {
                        list.spawn(greyed(f, label));
                    }
                }
            }
        });
}

/// The settings' items: fullscreen or not, and the resolutions this monitor offers (`monitor`:
/// its size and modes, when known), in two columns under the desktop's own.
fn settings(
    list: &mut ChildSpawnerCommands,
    f: &UiFonts,
    display: DisplaySettings,
    monitor: Option<((u32, u32), Vec<bevy::window::VideoMode>)>,
) {
    list.spawn(choice(
        f,
        if display.fullscreen {
            "Fullscreen: On"
        } else {
            "Fullscreen: Off"
        },
        MenuAction::Fullscreen,
        display.fullscreen,
        item_node(),
    ));
    list.spawn((
        Text::new("Resolution"),
        f.regular(14.0),
        TextColor(colours::MUTED),
        Pickable::IGNORE,
        Node {
            margin: UiRect::top(px(4)),
            ..default()
        },
    ));
    let (size, modes) = monitor.map_or((None, Vec::new()), |(s, m)| (Some(s), m));
    let own = match (display.fullscreen, size) {
        (true, Some(size)) => format!("Desktop ({})", display::label(size)),
        (true, None) => "Desktop".to_string(),
        (false, _) => "Any size".to_string(),
    };
    list.spawn(choice(
        f,
        &own,
        MenuAction::Resolution(None),
        display.resolution.is_none(),
        item_node(),
    ));
    let cell = || Node {
        width: Val::Percent(48.0),
        padding: UiRect::axes(px(6), px(8)),
        border_radius: BorderRadius::all(px(6)),
        justify_content: JustifyContent::Center,
        ..default()
    };
    list.spawn(Node {
        flex_wrap: FlexWrap::Wrap,
        justify_content: JustifyContent::SpaceBetween,
        row_gap: px(8),
        ..default()
    })
    .with_children(|grid| {
        for (size, usable) in display::offered(size, &modes, display.fullscreen) {
            let label = display::label(size);
            if usable {
                grid.spawn(choice(
                    f,
                    &label,
                    MenuAction::Resolution(Some(size)),
                    display.resolution == Some(size),
                    cell(),
                ));
            } else {
                grid.spawn(greyed_in(f, &label, cell()));
            }
        }
    });
}

fn colour_items(mut items: Query<(&MenuItem, &Hovered, &mut BackgroundColor), Changed<Hovered>>) {
    for (item, hovered, mut bg) in &mut items {
        bg.0 = match (item.chosen, hovered.0) {
            (true, false) => colours::CHOSEN,
            (true, true) => colours::CHOSEN_HOVER,
            (false, false) => colours::BUTTON,
            (false, true) => colours::BUTTON_HOVER,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> (App, std::sync::mpsc::Receiver<Request>) {
        let (net, requests) = Net::for_test();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(net)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<WatchStatus>()
            .init_resource::<EscapeMenu>()
            .init_resource::<Signing>()
            .insert_resource(DisplaySettings {
                fullscreen: true,
                resolution: None,
            })
            .insert_resource(Screen::Main)
            .insert_resource(LoginScreen::new("127.0.0.1:8080", true))
            .insert_resource(ServerStatus {
                server: "http://127.0.0.1:8080".into(),
                connection: Connection::Connecting,
                notices: vec![],
                retry_in: None,
            })
            .add_message::<AppExit>()
            .add_systems(
                Update,
                (
                    toggle_on_escape,
                    close_off_the_main_screen.run_if(resource_changed::<Screen>),
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
    fn escape_opens_the_menu_over_a_game_or_the_main_screen_and_closes_it() {
        let (mut app, _requests) = app();
        press_escape(&mut app);
        assert!(open(&app), "on the main screen");
        press_escape(&mut app);
        assert!(!open(&app));
        watch_a_room(&mut app);
        press_escape(&mut app);
        assert!(open(&app));
        assert!(!closed(Some(app.world().resource::<EscapeMenu>())));
        press_escape(&mut app);
        assert!(!open(&app));
        assert!(closed(None), "an app with no menu is never held up by one");
    }

    #[test]
    fn not_on_the_login_screen() {
        let (mut app, _requests) = app();
        *app.world_mut().resource_mut::<Screen>() = Screen::Login;
        press_escape(&mut app);
        assert!(!open(&app));
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
    fn logout_ends_the_account_and_goes_to_the_login_screen() {
        let (mut app, requests) = app();
        watch_a_room(&mut app);
        app.world_mut().resource_mut::<ServerStatus>().connection =
            Connection::Ready(crate::net::session::Snapshot {
                server: "http://127.0.0.1:8080".into(),
                version: "1".into(),
                api_version: "1.7.0".into(),
                defs: vec![],
                games: vec![],
                account: Some(crate::net::session::Account {
                    handle: "ana".into(),
                    display_name: String::new(),
                    must_change_password: false,
                }),
                session_ended: false,
            });
        requests.try_iter().count();
        choose(&mut app, MenuAction::Logout);
        assert_eq!(*app.world().resource::<Screen>(), Screen::Login);
        assert!(!app.world().resource::<WatchStatus>().is_watching());
        let asked: Vec<Request> = requests.try_iter().collect();
        let sign_out = asked.iter().position(|r| *r == Request::SignOut);
        let probe = asked
            .iter()
            .position(|r| matches!(r, Request::Probe { .. }));
        assert!(
            sign_out.is_some() && sign_out < probe,
            "signed out before the server is checked again: {asked:?}"
        );
        assert!(asked.contains(&Request::Remember {
            server: false,
            auto_login: false
        }));
        assert!(!app.world().resource::<LoginScreen>().auto_login);
        let form = app.world().resource::<Signing>().form.clone().unwrap();
        assert_eq!(form.handle, "ana", "the username is filled in again");
    }

    #[test]
    fn settings_change_the_display_with_the_menu_open_and_keep_it() {
        let (mut app, requests) = app();
        press_escape(&mut app);
        choose(&mut app, MenuAction::Settings);
        let page = |app: &App| app.world().resource::<EscapeMenu>().page;
        assert!(open(&app));
        assert_eq!(page(&app), Page::Settings);

        choose(&mut app, MenuAction::Fullscreen);
        choose(&mut app, MenuAction::Resolution(Some((1600, 900))));
        assert!(open(&app), "still open to choose more");
        assert_eq!(
            *app.world().resource::<DisplaySettings>(),
            DisplaySettings {
                fullscreen: false,
                resolution: Some((1600, 900)),
            }
        );
        let asked: Vec<Request> = requests.try_iter().collect();
        assert_eq!(
            asked.last(),
            Some(&Request::RememberDisplay {
                fullscreen: false,
                resolution: Some((1600, 900)),
            })
        );

        // Esc goes back to the menu, then closes it; it opens again at the menu.
        press_escape(&mut app);
        assert!(open(&app));
        assert_eq!(page(&app), Page::Menu);
        choose(&mut app, MenuAction::Settings);
        choose(&mut app, MenuAction::Back);
        assert_eq!(page(&app), Page::Menu);
        press_escape(&mut app);
        assert!(!open(&app));
        app.world_mut().resource_mut::<EscapeMenu>().page = Page::Settings;
        press_escape(&mut app);
        assert_eq!(page(&app), Page::Menu, "opened at the menu");
    }

    #[test]
    fn quit_ends_the_app() {
        let (mut app, _requests) = app();
        choose(&mut app, MenuAction::Quit);
        app.update();
        assert_eq!(app.should_exit(), Some(AppExit::Success));
    }

    #[test]
    fn what_is_greyed_in_a_game_and_on_the_main_screen() {
        let greyed = |in_game| -> Vec<&str> {
            items(in_game)
                .iter()
                .filter(|(_, a)| a.is_none())
                .map(|(n, _)| *n)
                .collect()
        };
        assert_eq!(greyed(true), ["Game Options", "Profile"]);
        assert_eq!(greyed(false), ["Main Menu", "Game Options", "Profile"]);
        assert!(items(false).iter().any(|(n, _)| *n == "Logout"));
    }
}
