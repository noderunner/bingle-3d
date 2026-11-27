//! The panels in a headset, hung in the world (`world_panel.rs`) and pressed with a
//! controller's beam: Bevy UI like the window's, so their buttons are `Button`s and do what
//! they say through `Activate`, as the window's do.
//!
//! - **The menu** opens in front of the player (`vr.rs` opens it, from a controller's menu
//!   button): resume; play the move and go back (as Enter and Backspace); sit at the table
//!   again; whether the legal moves and the coordinates are shown; the place; quit.
//! - **The game's panel** (`game_panel.rs`) stands beside the table, at the player's front
//!   left, while a headset's session runs (`InHeadset`): the same panel as the window's,
//!   drawn into the world instead of the window, and back in the window when the session ends.
//! - **The escape menu** (`escape_menu.rs`) opens in front of the player (`vr.rs` places it),
//!   from a controller or Esc: the window's own, hung in the world, with nothing dimmed.
//! - **The screen**: everything else the window shows (the login screen, the main screen, the
//!   friends panel) is drawn on a screen as big as the window across the table, shown while no
//!   game is being watched. Each of the window's UI roots is given to it while a session runs.

use bevy::picking::Pickable;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button, observe};

use crate::board::ShowLegalMoves;
use crate::browser::colors;
use crate::coordinates::ShowCoordinates;
use crate::environment::{Chosen, Environments};
use crate::escape_menu::{self, EscapeMenu, MenuOverlay};
use crate::fonts::UiFonts;
use crate::game_panel::{self, GamePanel};
use crate::net::{Net, Request, ServerStatus, WatchStatus, reconnect, stop_watching};
use crate::play;
use crate::world_panel::{self, Hung, Look};

pub struct VrPanelsPlugin;

impl Plugin for VrPanelsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VrMenu>()
            .init_resource::<SitAgain>()
            .init_resource::<InHeadset>()
            .add_systems(
                Startup,
                (spawn_menu, hang_game_panel, hang_screen, hang_escape_menu),
            )
            .add_systems(
                Update,
                (
                    show_menu,
                    color_items,
                    move_game_panel.run_if(resource_changed::<InHeadset>),
                    show_game_panel,
                    (move_to_screen, show_screen).chain(),
                    move_escape_menu.run_if(resource_changed::<InHeadset>),
                    show_escape_menu,
                ),
            );
    }
}

/// Whether a headset's session is running (`vr.rs` says): the game's panel is in the world
/// then, and in the window otherwise.
#[derive(Resource, Debug, Default)]
pub struct InHeadset(pub bool);

/// Whether the menu is open.
#[derive(Resource, Debug, Default)]
pub struct VrMenu {
    pub open: bool,
}

/// Asked for, from the menu or a controller: sit at the table again (`vr.rs`).
#[derive(Resource, Debug, Default)]
pub struct SitAgain(pub bool);

/// The panel the menu is shown on, for `vr.rs` to place in front of the player.
#[derive(Component)]
pub struct MenuPanel;

/// The menu: 0.32 by 0.38 metres.
const MENU: Look = Look {
    size: UVec2::new(640, 760),
    scale: 1.0,
    metres: 0.0005,
    background: BACKGROUND,
};

/// The game's panel: as wide as the window's, and tall enough for a lobby, 0.32 by 0.60
/// metres; drawn sharper than the window's, and nothing where the panel does not reach.
const GAME: Look = Look {
    size: UVec2::new(game_panel::WIDTH as u32, 600),
    scale: 2.0,
    metres: 0.001,
    background: Color::NONE,
};

/// Where the game's panel stands: at the front left of a player sat at the table, its top a
/// little above the table top, turned towards them.
fn beside_table() -> Transform {
    let top = 0.42;
    let at = Vec3::new(-0.58, top - GAME.in_world().y / 2.0, 0.32);
    // Where the eyes of someone sat at the table are, about (`vr.rs`).
    let eyes = Vec3::new(0.0, 0.45, 0.75);
    let away = Vec3::new(at.x - eyes.x, 0.0, at.z - eyes.z).normalize();
    // The panel faces its +z, so its −z looks away from the player.
    Transform::from_translation(at).looking_to(away, Vec3::Y)
}

/// The screen: the window's size, the window's pixels drawn twice as fine, a millimetre to
/// each, so 1.28 by 0.72 metres; nothing where the window's UI draws nothing.
const SCREEN: Look = Look {
    size: UVec2::new(1280, 720),
    scale: 2.0,
    metres: 0.001,
    background: Color::NONE,
};

/// Where the screen stands: across the table from a player sat at it, a little above their
/// eyes, facing them.
fn across_table() -> Transform {
    Transform::from_xyz(0.0, 0.55, -0.5)
}

/// The escape menu's panel: its menu (300 pixels wide) and a little room about it; nothing
/// where the menu is not. 0.31 by 0.41 metres.
const ESCAPE: Look = Look {
    size: UVec2::new(340, 460),
    scale: 2.0,
    metres: 0.0009,
    background: Color::NONE,
};

/// The panel the escape menu is shown on, for `vr.rs` to place in front of the player.
#[derive(Component)]
pub struct EscapePanel;

/// The escape menu hung in the world.
#[derive(Resource)]
struct EscapeInWorld(Hung);

/// The screen hung in the world.
#[derive(Resource)]
struct ScreenInWorld(Hung);

/// A UI root of the window's, given to the screen.
#[derive(Component)]
struct OnScreen;

/// The game's panel hung in the world.
#[derive(Resource)]
struct GamePanelInWorld(Hung);

/// The menu hung in the world.
#[derive(Resource)]
struct MenuInWorld(Hung);
/// How far ahead of the eyes the menu opens, and how far below them, in metres.
const AHEAD: f32 = 0.5;
const BELOW: f32 = 0.12;

const BACKGROUND: Color = Color::srgb(0.07, 0.08, 0.10);

/// Where the menu opens, for a head at `head` in the world: ahead of it along the ground, a
/// little below the eyes, upright and facing it.
pub fn place_in_front(head: Transform) -> Transform {
    let ahead = head.rotation * Vec3::NEG_Z;
    let flat = Vec3::new(ahead.x, 0.0, ahead.z)
        .try_normalize()
        .unwrap_or(Vec3::NEG_Z);
    let at = head.translation + flat * AHEAD - Vec3::Y * BELOW;
    // The panel faces its +z: towards the head, so its −z looks away.
    Transform::from_translation(at).looking_to(flat, Vec3::Y)
}

/// What an item does.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
enum Item {
    Resume,
    /// Back to the main screen, as the escape menu's *Main Menu*.
    MainMenu,
    Play,
    Back,
    Sit,
    LegalMoves,
    Coordinates,
    Place(String),
    Quit,
}

impl Item {
    /// Whether choosing it closes the menu: all but what is turned on and off, or chosen
    /// among others, where the player may want to try another.
    fn closes(&self) -> bool {
        !matches!(self, Item::LegalMoves | Item::Coordinates | Item::Place(_))
    }
}

fn spawn_menu(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    all: Res<Environments>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let hung = world_panel::hang(
        &mut commands,
        &mut images,
        &mut meshes,
        &mut materials,
        MENU,
    );
    commands.entity(hung.quad).insert(MenuPanel);
    commands.insert_resource(MenuInWorld(hung));
    let f = &*fonts;
    commands
        .spawn((
            UiTargetCamera(hung.camera),
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(32)),
                row_gap: px(14),
                ..default()
            },
        ))
        .with_children(|menu| {
            menu.spawn(heading(f, "Bingle", 34.0));
            menu.spawn(row()).with_children(|row| {
                row.spawn(item(f, "Resume", Item::Resume));
                row.spawn(item(f, "Main Menu", Item::MainMenu));
            });
            menu.spawn(row()).with_children(|row| {
                row.spawn(item(f, "Play the move", Item::Play));
                row.spawn(item(f, "Go back", Item::Back));
            });
            menu.spawn(row()).with_children(|row| {
                row.spawn(item(f, "Sit at the table again", Item::Sit));
            });
            menu.spawn(heading(f, "Show", 22.0));
            menu.spawn(row()).with_children(|row| {
                row.spawn(item(f, "Legal moves", Item::LegalMoves));
                row.spawn(item(f, "Coordinates", Item::Coordinates));
            });
            menu.spawn(heading(f, "Place", 22.0));
            menu.spawn(row()).with_children(|row| {
                for environment in &all.0 {
                    row.spawn(item(
                        f,
                        &environment.name,
                        Item::Place(environment.id.clone()),
                    ));
                }
            });
            // Apart from the rest, so that it is not pressed by mistake.
            menu.spawn(Node {
                height: px(24),
                ..default()
            });
            menu.spawn(row()).with_children(|row| {
                row.spawn(item(f, "Quit", Item::Quit));
            });
        });
}

fn hang_game_panel(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let hung = world_panel::hang(
        &mut commands,
        &mut images,
        &mut meshes,
        &mut materials,
        GAME,
    );
    commands.entity(hung.quad).insert(beside_table());
    commands.insert_resource(GamePanelInWorld(hung));
}

fn heading(f: &UiFonts, text: &str, size: f32) -> impl Bundle {
    (
        Text::new(text),
        f.semibold(size),
        TextColor(colors::TEXT),
        Pickable::IGNORE,
    )
}

fn row() -> Node {
    Node {
        column_gap: px(14),
        ..default()
    }
}

fn item(f: &UiFonts, label: &str, what: Item) -> impl Bundle {
    (
        what,
        Button,
        Hovered::default(),
        Node {
            // Buttons in a row share it equally; one alone in the column keeps its height.
            flex_basis: px(0),
            flex_grow: 1.0,
            padding: UiRect::axes(px(18), px(14)),
            border_radius: BorderRadius::all(px(10)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(colors::BUTTON),
        observe(on_item),
        children![(
            Text::new(label),
            f.semibold(26.0),
            TextColor(colors::TEXT),
            Pickable::IGNORE,
        )],
    )
}

/// What is shown on and off, and the place, as one system parameter.
#[derive(bevy::ecs::system::SystemParam)]
struct Settings<'w> {
    legal_moves: ResMut<'w, ShowLegalMoves>,
    coordinates: ResMut<'w, ShowCoordinates>,
    chosen: ResMut<'w, Chosen>,
}

impl Settings<'_> {
    /// Whether an item is on, or chosen.
    fn is_on(&self, item: &Item) -> bool {
        match item {
            Item::LegalMoves => self.legal_moves.0,
            Item::Coordinates => self.coordinates.0,
            Item::Place(id) => self.chosen.0 == *id,
            _ => false,
        }
    }
}

fn on_item(
    activate: On<Activate>,
    items: Query<&Item>,
    net: Option<Res<Net>>,
    mut watch: ResMut<WatchStatus>,
    mut settings: Settings,
    (mut menu, mut sit, status): (
        ResMut<VrMenu>,
        ResMut<SitAgain>,
        Option<ResMut<ServerStatus>>,
    ),
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(what) = items.get(activate.entity) else {
        return;
    };
    match what {
        Item::Resume => {}
        Item::MainMenu => {
            if let (Some(net), Some(mut status)) = (&net, status)
                && watch.is_watching()
            {
                // As the escape menu's: stop watching, and read the server's lists again.
                stop_watching(net, &mut watch);
                reconnect(net, &mut status);
            }
        }
        Item::Play => {
            if let Some(net) = &net {
                play::submit(net, &mut watch);
            }
        }
        Item::Back => play::back(&mut watch),
        Item::Sit => sit.0 = true,
        Item::LegalMoves => settings.legal_moves.0 = !settings.legal_moves.0,
        Item::Coordinates => settings.coordinates.0 = !settings.coordinates.0,
        Item::Place(id) => {
            if settings.chosen.0 != *id {
                settings.chosen.0 = id.clone();
                if let Some(net) = &net {
                    net.send(Request::RememberEnvironment(id.clone()));
                }
            }
        }
        Item::Quit => {
            exit.write(AppExit::Success);
        }
    }
    if what.closes() {
        menu.open = false;
    }
}

fn hang_screen(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let hung = world_panel::hang(
        &mut commands,
        &mut images,
        &mut meshes,
        &mut materials,
        SCREEN,
    );
    commands.entity(hung.quad).insert(across_table());
    commands.insert_resource(ScreenInWorld(hung));
}

fn hang_escape_menu(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let hung = world_panel::hang(
        &mut commands,
        &mut images,
        &mut meshes,
        &mut materials,
        ESCAPE,
    );
    commands.entity(hung.quad).insert(EscapePanel);
    commands.insert_resource(EscapeInWorld(hung));
}

/// Gives the escape menu to its panel while a headset's session runs, with nothing dimmed
/// about it, and back to the window when it ends.
fn move_escape_menu(
    mut commands: Commands,
    in_headset: Res<InHeadset>,
    hung: Res<EscapeInWorld>,
    mut overlays: Query<(Entity, &mut BackgroundColor), With<MenuOverlay>>,
) {
    for (overlay, mut background) in &mut overlays {
        if in_headset.0 {
            commands
                .entity(overlay)
                .insert(UiTargetCamera(hung.0.camera));
            background.0 = Color::NONE;
        } else {
            commands.entity(overlay).remove::<UiTargetCamera>();
            background.0 = escape_menu::DIM;
        }
    }
}

/// Shows the escape menu in the world while it is open there.
fn show_escape_menu(
    in_headset: Res<InHeadset>,
    escape: Res<EscapeMenu>,
    hung: Res<EscapeInWorld>,
    mut cameras: Query<&mut Camera>,
    mut visibilities: Query<&mut Visibility>,
) {
    let shown = in_headset.0 && escape.open;
    world_panel::show(hung.0, shown, &mut cameras, &mut visibilities);
}

/// The window's UI roots that are not given anywhere yet: all but the game's panel, the
/// escape menu and what is already in the world.
type WindowRoots = (
    With<Node>,
    Without<ChildOf>,
    Without<UiTargetCamera>,
    Without<GamePanel>,
    Without<MenuOverlay>,
);

/// Gives the window's UI to the screen while a headset's session runs (a root made later
/// too), and back to the window when it ends.
fn move_to_screen(
    mut commands: Commands,
    in_headset: Res<InHeadset>,
    hung: Res<ScreenInWorld>,
    window_roots: Query<Entity, WindowRoots>,
    on_screen: Query<Entity, With<OnScreen>>,
) {
    if in_headset.0 {
        for root in &window_roots {
            commands
                .entity(root)
                .insert((UiTargetCamera(hung.0.camera), OnScreen));
        }
    } else if in_headset.is_changed() {
        for root in &on_screen {
            commands.entity(root).remove::<(UiTargetCamera, OnScreen)>();
        }
    }
}

/// Shows the screen in the world while it has the window's UI and no game is watched.
fn show_screen(
    in_headset: Res<InHeadset>,
    watching: Res<WatchStatus>,
    hung: Res<ScreenInWorld>,
    mut cameras: Query<&mut Camera>,
    mut visibilities: Query<&mut Visibility>,
) {
    let shown = in_headset.0 && !watching.is_watching();
    world_panel::show(hung.0, shown, &mut cameras, &mut visibilities);
}

/// Shows the menu while it is open.
fn show_menu(
    menu: Res<VrMenu>,
    hung: Res<MenuInWorld>,
    mut cameras: Query<&mut Camera>,
    mut visibilities: Query<&mut Visibility>,
) {
    world_panel::show(hung.0, menu.open, &mut cameras, &mut visibilities);
}

/// Gives the game's panel to the world while a headset's session runs, and back to the
/// window when it ends.
fn move_game_panel(
    mut commands: Commands,
    in_headset: Res<InHeadset>,
    hung: Res<GamePanelInWorld>,
    mut panels: Query<(Entity, &mut Node), With<GamePanel>>,
) {
    for (panel, mut node) in &mut panels {
        if in_headset.0 {
            commands.entity(panel).insert(UiTargetCamera(hung.0.camera));
            (node.left, node.top) = (px(0), px(0));
        } else {
            commands.entity(panel).remove::<UiTargetCamera>();
            (node.left, node.top) = (px(game_panel::MARGIN), px(game_panel::MARGIN));
        }
    }
}

/// Shows the game's panel in the world while it is there and a game is being watched.
fn show_game_panel(
    in_headset: Res<InHeadset>,
    watching: Res<WatchStatus>,
    hung: Res<GamePanelInWorld>,
    mut cameras: Query<&mut Camera>,
    mut visibilities: Query<&mut Visibility>,
) {
    let shown = in_headset.0 && watching.is_watching();
    world_panel::show(hung.0, shown, &mut cameras, &mut visibilities);
}

fn color_items(settings: Settings, mut items: Query<(&Item, &Hovered, &mut BackgroundColor)>) {
    for (item, hovered, mut bg) in &mut items {
        let color = match (settings.is_on(item), hovered.0) {
            (true, false) => colors::CHOSEN,
            (true, true) => colors::CHOSEN_HOVER,
            (false, false) => colors::BUTTON,
            (false, true) => colors::BUTTON_HOVER,
        };
        if bg.0 != color {
            bg.0 = color;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < 1e-4
    }

    #[test]
    fn the_game_panel_stands_at_the_front_left_turned_to_the_player() {
        let at = beside_table();
        assert!(at.translation.x < -0.4 && at.translation.z > 0.0);
        // Its top a little above the table top, and upright.
        let top = at.translation.y + GAME.in_world().y / 2.0;
        assert!((0.3..0.5).contains(&top));
        assert!(close(at.rotation * Vec3::Y, Vec3::Y));
        // Its face towards the eyes, along the ground.
        let face = at.rotation * Vec3::Z;
        let to_eyes = (Vec3::new(0.0, 0.45, 0.75) - at.translation) * Vec3::new(1.0, 0.0, 1.0);
        assert!(face.dot(to_eyes.normalize()) > 0.999);
    }

    #[test]
    fn the_window_s_ui_goes_to_the_screen_in_a_headset_and_comes_back() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<InHeadset>()
            .add_systems(Update, move_to_screen);
        let camera = app.world_mut().spawn_empty().id();
        let quad = app.world_mut().spawn_empty().id();
        app.insert_resource(ScreenInWorld(Hung { camera, quad }));
        let world = app.world_mut();
        let login = world.spawn(Node::default()).id();
        let inner = world.spawn((Node::default(), ChildOf(login))).id();
        let game = world.spawn((Node::default(), GamePanel)).id();
        let escape = world.spawn((Node::default(), MenuOverlay)).id();
        let elsewhere = world.spawn_empty().id();
        let menu = world
            .spawn((Node::default(), UiTargetCamera(elsewhere)))
            .id();
        app.update();
        let target = |app: &App, e| app.world().get::<UiTargetCamera>(e).map(|t| t.0);
        assert_eq!(target(&app, login), None, "not in a headset");

        app.world_mut().resource_mut::<InHeadset>().0 = true;
        app.update();
        assert_eq!(target(&app, login), Some(camera));
        assert_eq!(target(&app, inner), None, "only roots");
        assert_eq!(target(&app, game), None, "the game's panel has its own");
        assert_eq!(target(&app, escape), None, "and so has the escape menu");
        assert_eq!(target(&app, menu), Some(elsewhere));
        // A root made later goes there too.
        let later = app.world_mut().spawn(Node::default()).id();
        app.update();
        assert_eq!(target(&app, later), Some(camera));

        app.world_mut().resource_mut::<InHeadset>().0 = false;
        app.update();
        assert_eq!(target(&app, login), None);
        assert_eq!(target(&app, later), None);
        assert_eq!(target(&app, menu), Some(elsewhere));
    }

    #[test]
    fn the_menu_opens_ahead_of_the_eyes_upright_and_facing_them() {
        // Turned to +x and looking down at the table.
        let head = Transform::from_xyz(1.0, 0.5, 0.3).with_rotation(
            Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2) * Quat::from_rotation_x(-0.6),
        );
        let at = place_in_front(head);
        assert!(close(
            at.translation,
            Vec3::new(1.0 + AHEAD, 0.5 - BELOW, 0.3)
        ));
        // Its face (+z) towards the head, and upright.
        assert!(close(at.rotation * Vec3::Z, Vec3::NEG_X));
        assert!(close(at.rotation * Vec3::Y, Vec3::Y));
    }

    fn app() -> (App, std::sync::mpsc::Receiver<Request>) {
        let (net, requests) = Net::for_test();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(net)
            .init_resource::<WatchStatus>()
            .init_resource::<ShowLegalMoves>()
            .init_resource::<ShowCoordinates>()
            .insert_resource(Chosen("room".into()))
            .insert_resource(VrMenu { open: true })
            .init_resource::<SitAgain>()
            .add_message::<AppExit>();
        (app, requests)
    }

    fn choose(app: &mut App, what: Item) {
        let entity = app.world_mut().spawn(what).observe(on_item).id();
        app.world_mut().trigger(Activate { entity });
        app.update();
    }

    #[test]
    fn showing_and_places_leave_it_open_and_the_rest_close_it() {
        let (mut app, requests) = app();
        choose(&mut app, Item::LegalMoves);
        choose(&mut app, Item::Coordinates);
        assert!(app.world().resource::<ShowLegalMoves>().0);
        assert!(app.world().resource::<ShowCoordinates>().0);
        choose(&mut app, Item::Place("garden".into()));
        assert_eq!(app.world().resource::<Chosen>().0, "garden");
        assert!(matches!(
            requests.try_recv(),
            Ok(Request::RememberEnvironment(id)) if id == "garden"
        ));
        assert!(app.world().resource::<VrMenu>().open);

        choose(&mut app, Item::Sit);
        assert!(app.world().resource::<SitAgain>().0);
        assert!(!app.world().resource::<VrMenu>().open);
    }

    #[test]
    fn quit_quits() {
        let (mut app, _requests) = app();
        choose(&mut app, Item::Quit);
        let exits = app.world().resource::<Messages<AppExit>>();
        assert_eq!(exits.len(), 1);
    }
}
