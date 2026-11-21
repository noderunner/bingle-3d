//! The menu in a headset: a panel that opens in front of the player (`vr.rs` opens it, from a
//! controller's menu button), pressed with a controller's beam.
//!
//! It is Bevy UI like the window's, drawn by a camera of its own into an image, and the image
//! is shown on a `Panel` in the world (`ray_pointer.rs`), so its buttons are `Button`s and an
//! item does what it says through `Activate`, as the window's do. The camera draws only while
//! the menu is open.
//!
//! What is in it: resume; play the move and go back (as Enter and Backspace); sit at the table
//! again; whether the legal moves and the coordinates are shown; the place; quit.

use bevy::camera::{ClearColorConfig, ImageRenderTarget, RenderTarget};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::picking::Pickable;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::ui_widgets::{Activate, Button, observe};

use crate::board::ShowLegalMoves;
use crate::browser::colours;
use crate::coordinates::ShowCoordinates;
use crate::environment::{Chosen, Environments};
use crate::fonts::UiFonts;
use crate::net::{Net, Request, WatchStatus};
use crate::play;
use crate::ray_pointer::Panel;

pub struct VrMenuPlugin;

impl Plugin for VrMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VrMenu>()
            .init_resource::<SitAgain>()
            .add_systems(Startup, spawn_menu)
            .add_systems(
                Update,
                (show.run_if(resource_changed::<VrMenu>), colour_items),
            );
    }
}

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

/// The menu's image, in pixels, and how big a pixel is in the world, in metres: 0.32 by 0.38.
const PIXELS: UVec2 = UVec2::new(640, 760);
const PIXEL: f32 = 0.0005;
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
    let image = images.add(Image::new_target_texture(
        PIXELS.x,
        PIXELS.y,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    let camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: -1,
                is_active: false,
                clear_color: ClearColorConfig::Custom(BACKGROUND),
                ..default()
            },
            RenderTarget::Image(ImageRenderTarget {
                handle: image.clone(),
                scale_factor: 1.0,
            }),
        ))
        .id();
    let f = &*fonts;
    commands
        .spawn((
            UiTargetCamera(camera),
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

    let size = PIXELS.as_vec2() * PIXEL;
    commands.spawn((
        MenuPanel,
        Panel {
            size,
            pixels: PIXELS,
            image: image.clone(),
        },
        Mesh3d(meshes.add(Rectangle::from_size(size))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(image),
            unlit: true,
            fog_enabled: false,
            ..default()
        })),
        NotShadowCaster,
        NotShadowReceiver,
        Transform::default(),
        Visibility::Hidden,
    ));
}

fn heading(f: &UiFonts, text: &str, size: f32) -> impl Bundle {
    (
        Text::new(text),
        f.semibold(size),
        TextColor(colours::TEXT),
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
        BackgroundColor(colours::BUTTON),
        observe(on_item),
        children![(
            Text::new(label),
            f.semibold(26.0),
            TextColor(colours::TEXT),
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

#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer takes what it touches as parameters"
)]
fn on_item(
    activate: On<Activate>,
    items: Query<&Item>,
    net: Option<Res<Net>>,
    mut watch: ResMut<WatchStatus>,
    mut settings: Settings,
    mut menu: ResMut<VrMenu>,
    mut sit: ResMut<SitAgain>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(what) = items.get(activate.entity) else {
        return;
    };
    match what {
        Item::Resume => {}
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

/// Shows the panel while the menu is open, and has its camera draw only then.
fn show(
    menu: Res<VrMenu>,
    mut panels: Query<&mut Visibility, With<MenuPanel>>,
    mut cameras: Query<&mut Camera, With<Camera2d>>,
    targets: Query<&UiTargetCamera>,
) {
    for mut visibility in &mut panels {
        *visibility = if menu.open {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    for target in &targets {
        if let Ok(mut camera) = cameras.get_mut(target.0) {
            camera.is_active = menu.open;
        }
    }
}

fn colour_items(settings: Settings, mut items: Query<(&Item, &Hovered, &mut BackgroundColor)>) {
    for (item, hovered, mut bg) in &mut items {
        let colour = match (settings.is_on(item), hovered.0) {
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

    fn close(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < 1e-4
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
