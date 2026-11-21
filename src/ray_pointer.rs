//! A pointer that is a ray in the world rather than a place on a screen: a controller in a
//! headset (`vr.rs`), aimed along its own −z.
//!
//! It is one of Bevy's pointers, as the mouse is. What it points at is found here with a ray
//! cast against what is `Pickable`, and its trigger is the pointer's primary button, so a pull
//! and a release on a cell is the same click the mouse makes, and nothing that listens for
//! clicks knows the difference (`play.rs`). **A new input device becomes gestures here.**

use bevy::asset::uuid::Uuid;
use bevy::camera::NormalizedRenderTarget;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::picking::PickingSystems;
use bevy::picking::backend::{HitData, PointerHits};
use bevy::picking::mesh_picking::MeshPickingSettings;
use bevy::picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings, RayCastVisibility};
use bevy::picking::pointer::{Location, PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;

pub struct RayPointerPlugin;

impl Plugin for RayPointerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(First, feed.in_set(PickingSystems::Input))
            .add_systems(PreUpdate, cast.in_set(PickingSystems::Backend))
            .add_systems(Update, draw_beams);
    }
}

/// How far a trigger must be pulled to press (of 1), and how far let go to release: apart, so
/// that a finger resting at the edge does not click over and over.
const PRESS_AT: f32 = 0.6;
const RELEASE_AT: f32 = 0.35;

/// How long the beam is drawn when it points at nothing, in metres, and how thick.
const BEAM_AT_REST: f32 = 0.35;
const BEAM_WIDTH: f32 = 0.0025;

/// On an entity with a place in the world: it points along its −z.
#[derive(Component, Debug, Default)]
#[require(Transform)]
pub struct RayPointer {
    /// Whether the device knows where it is. A pointer that does not points at nothing.
    pub tracked: bool,
    /// Whether its trigger is down, and whether Bevy's picking has been told so.
    pressed: bool,
    told: bool,
    /// How far along the ray the nearest thing it could pick is.
    pub hit: Option<f32>,
}

impl RayPointer {
    /// The pointer and its identity among Bevy's pointers: `number` tells one hand from the
    /// other.
    pub fn bundle(number: u8) -> impl Bundle {
        (
            RayPointer::default(),
            PointerId::Custom(Uuid::from_u128(
                0x6b1f_3d2a_9c44_4e07_8a51_b1e6_7c00_0000 + number as u128,
            )),
        )
    }

    /// The trigger is this far in, from 0 to 1.
    pub fn squeeze(&mut self, value: f32) {
        if value >= PRESS_AT {
            self.pressed = true;
        } else if value <= RELEASE_AT {
            self.pressed = false;
        }
    }
}

/// Where Bevy's picking thinks the pointer is. It needs somewhere, to send what happens to
/// it; this is a target no camera draws to, so no screen's picking takes the pointer for its
/// own.
fn nowhere() -> Location {
    Location {
        target: NormalizedRenderTarget::None {
            width: 1,
            height: 1,
        },
        position: Vec2::ZERO,
    }
}

/// Tells Bevy's picking that the pointer is there, and when its trigger goes down and up. One
/// that is lost lets go.
fn feed(
    mut pointers: Query<(&PointerId, &mut RayPointer)>,
    mut inputs: MessageWriter<PointerInput>,
) {
    for (id, mut pointer) in &mut pointers {
        if pointer.tracked {
            inputs.write(PointerInput::new(
                *id,
                nowhere(),
                PointerAction::Move { delta: Vec2::ZERO },
            ));
        }
        let down = pointer.pressed && pointer.tracked;
        // One lost with its trigger down lets go only once it points at nothing, a frame
        // later: letting go over what it pointed at would be a click.
        let still_over = !pointer.tracked && pointer.hit.is_some();
        if down != pointer.told && !still_over {
            let action = if down {
                PointerAction::Press(PointerButton::Primary)
            } else {
                PointerAction::Release(PointerButton::Primary)
            };
            inputs.write(PointerInput::new(*id, nowhere(), action));
            pointer.told = down;
        }
    }
}

/// What each pointer points at: everything pickable along its ray, nearest first, up to the
/// first thing that hides what is behind it.
fn cast(
    settings: Option<Res<MeshPickingSettings>>,
    mut pointers: Query<(Entity, &PointerId, &GlobalTransform, &mut RayPointer)>,
    pickables: Query<&Pickable>,
    mut ray_cast: MeshRayCast,
    mut hits: MessageWriter<PointerHits>,
) {
    let visibility = settings.map_or(RayCastVisibility::VisibleInView, |s| s.ray_cast_visibility);
    for (entity, id, place, mut pointer) in &mut pointers {
        if !pointer.tracked {
            if pointer.hit.is_some() {
                pointer.hit = None;
            }
            continue;
        }
        let ray = Ray3d::new(place.translation(), place.forward());
        let settings = MeshRayCastSettings {
            visibility,
            filter: &|e| pickables.get(e).is_ok_and(|p| p.is_hoverable),
            early_exit_test: &|e| pickables.get(e).is_ok_and(|p| p.should_block_lower),
        };
        let picks: Vec<(Entity, HitData)> = ray_cast
            .cast_ray(ray, &settings)
            .iter()
            .map(|(hit_entity, hit)| {
                (
                    *hit_entity,
                    HitData::new(entity, hit.distance, Some(hit.point), Some(hit.normal)),
                )
            })
            .collect();
        let nearest = picks.iter().map(|(_, hit)| hit.depth).reduce(f32::min);
        if pointer.hit != nearest {
            pointer.hit = nearest;
        }
        if !picks.is_empty() {
            hits.write(PointerHits::new(*id, picks, 0.0));
        }
    }
}

/// The line drawn along a pointer's ray, as its child.
#[derive(Component)]
pub struct Beam;

/// A beam to give a pointer as a child: thin, unlit, and no part of the light.
pub fn beam(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> impl Bundle {
    (
        Beam,
        Mesh3d(meshes.add(Cuboid::new(BEAM_WIDTH, BEAM_WIDTH, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.92, 0.96, 1.0),
            unlit: true,
            ..default()
        })),
        NotShadowCaster,
        NotShadowReceiver,
        Visibility::Hidden,
    )
}

/// A beam's place and length: from the pointer to what it points at.
fn beam_transform(hit: Option<f32>) -> Transform {
    let length = hit.unwrap_or(BEAM_AT_REST).max(0.001);
    Transform::from_xyz(0.0, 0.0, -length / 2.0).with_scale(Vec3::new(1.0, 1.0, length))
}

fn draw_beams(
    pointers: Query<(&RayPointer, &Children), Changed<RayPointer>>,
    mut beams: Query<(&mut Transform, &mut Visibility), With<Beam>>,
) {
    for (pointer, children) in &pointers {
        for child in children.iter() {
            if let Ok((mut transform, mut visibility)) = beams.get_mut(child) {
                *transform = beam_transform(pointer.hit);
                *visibility = if pointer.tracked {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::camera::primitives::Aabb;
    use bevy::picking::events::{PointerClick, PointerOut, PointerOver};
    use bevy::picking::{InteractionPlugin, PickingPlugin};

    #[derive(Resource, Default)]
    struct Seen {
        over: Vec<Entity>,
        out: Vec<Entity>,
        clicks: Vec<Entity>,
    }

    /// A pickable square a metre ahead of the origin, facing it, and a pointer at the origin.
    fn app() -> (App, Entity, Entity) {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            TransformPlugin,
            PickingPlugin,
            InteractionPlugin,
            RayPointerPlugin,
        ))
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .insert_resource(MeshPickingSettings {
            require_markers: true,
            ray_cast_visibility: RayCastVisibility::Any,
        })
        .init_resource::<Seen>()
        .add_observer(|e: On<PointerOver>, mut seen: ResMut<Seen>| seen.over.push(e.entity))
        .add_observer(|e: On<PointerOut>, mut seen: ResMut<Seen>| seen.out.push(e.entity))
        .add_observer(|e: On<PointerClick>, mut seen: ResMut<Seen>| {
            if e.button == PointerButton::Primary {
                seen.clicks.push(e.entity);
            }
        });
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Rectangle::new(0.2, 0.2));
        // What the renderer's plugins would work out: the mesh's bounds, and that it is shown.
        let drawn = || {
            (
                Aabb::from_min_max(Vec3::new(-0.1, -0.1, 0.0), Vec3::new(0.1, 0.1, 0.0)),
                Visibility::default(),
            )
        };
        let square = app
            .world_mut()
            .spawn((
                Mesh3d(mesh.clone()),
                Transform::from_xyz(0.0, 0.0, -1.0),
                Pickable::default(),
                drawn(),
            ))
            .id();
        // Nearer, in the way, and not pickable: the ray goes through it.
        app.world_mut()
            .spawn((Mesh3d(mesh), Transform::from_xyz(0.0, 0.0, -0.5), drawn()));
        let pointer = app.world_mut().spawn(RayPointer::bundle(0)).id();
        app.update();
        (app, square, pointer)
    }

    fn pointer(app: &mut App, entity: Entity) -> Mut<'_, RayPointer> {
        app.world_mut().get_mut::<RayPointer>(entity).unwrap()
    }

    fn settle(app: &mut App) {
        for _ in 0..3 {
            app.update();
        }
    }

    #[test]
    fn a_trigger_presses_late_and_lets_go_later() {
        let mut p = RayPointer::default();
        p.squeeze(0.5);
        assert!(!p.pressed);
        p.squeeze(0.7);
        assert!(p.pressed);
        p.squeeze(0.5);
        assert!(p.pressed, "still held between the two");
        p.squeeze(0.2);
        assert!(!p.pressed);
    }

    #[test]
    fn a_pull_and_a_release_on_what_it_points_at_is_a_click() {
        let (mut app, square, hand) = app();
        pointer(&mut app, hand).tracked = true;
        settle(&mut app);
        assert_eq!(app.world().resource::<Seen>().over, vec![square]);
        let hit = pointer(&mut app, hand).hit.unwrap();
        assert!(
            (hit - 1.0).abs() < 1e-4,
            "the square, not what is in the way"
        );

        pointer(&mut app, hand).squeeze(1.0);
        settle(&mut app);
        assert!(app.world().resource::<Seen>().clicks.is_empty());
        pointer(&mut app, hand).squeeze(0.0);
        settle(&mut app);
        assert_eq!(app.world().resource::<Seen>().clicks, vec![square]);
    }

    #[test]
    fn pointing_away_leaves_it_and_a_pull_there_clicks_nothing() {
        let (mut app, square, hand) = app();
        pointer(&mut app, hand).tracked = true;
        settle(&mut app);
        *app.world_mut().get_mut::<Transform>(hand).unwrap() =
            Transform::from_rotation(Quat::from_rotation_y(1.0));
        settle(&mut app);
        assert_eq!(app.world().resource::<Seen>().out, vec![square]);
        assert_eq!(pointer(&mut app, hand).hit, None);
        pointer(&mut app, hand).squeeze(1.0);
        settle(&mut app);
        pointer(&mut app, hand).squeeze(0.0);
        settle(&mut app);
        assert!(app.world().resource::<Seen>().clicks.is_empty());
    }

    #[test]
    fn a_pointer_that_is_lost_points_at_nothing_and_lets_go() {
        let (mut app, square, hand) = app();
        pointer(&mut app, hand).tracked = true;
        settle(&mut app);
        pointer(&mut app, hand).squeeze(1.0);
        settle(&mut app);
        pointer(&mut app, hand).tracked = false;
        settle(&mut app);
        assert_eq!(app.world().resource::<Seen>().out, vec![square]);
        assert!(!pointer(&mut app, hand).told);
        // Found again with the trigger still down: no click from that.
        assert!(app.world().resource::<Seen>().clicks.is_empty());
    }

    #[test]
    fn the_beam_reaches_what_is_pointed_at() {
        let at_rest = beam_transform(None);
        assert_eq!(at_rest.scale.z, BEAM_AT_REST);
        let to = beam_transform(Some(0.8));
        assert_eq!(to.scale.z, 0.8);
        assert_eq!(to.translation.z, -0.4);
    }
}
