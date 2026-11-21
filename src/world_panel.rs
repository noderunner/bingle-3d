//! Bevy UI hung in the world: a camera of its own draws it into an image, and a flat quad
//! shows the image where it is put. A `RayPointer` (`ray_pointer.rs`) points at the quad, as
//! a `Panel`, and presses the buttons drawn on it. Used in a headset (`vr_panels.rs`).
//!
//! The UI is given to the camera with `UiTargetCamera`. The camera draws only while the panel
//! is shown (`show`).

use bevy::camera::{ClearColorConfig, ImageRenderTarget, RenderTarget};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use crate::ray_pointer::Panel;

/// A panel hung in the world: the camera that draws its UI, and the quad that shows it.
#[derive(Debug, Clone, Copy)]
pub struct Hung {
    pub camera: Entity,
    pub quad: Entity,
}

/// What a panel is: its size in the UI's own pixels (logical, as a window's), how many of the
/// image's pixels there are to each (as a window's scale factor: more is sharper), how big one
/// is in the world, in metres, and what is drawn where the UI draws nothing. A see-through
/// background leaves only what the UI draws.
#[derive(Debug, Clone, Copy)]
pub struct Look {
    pub size: UVec2,
    pub scale: f32,
    pub metres: f32,
    pub background: Color,
}

impl Look {
    /// How big the panel is in the world, in metres.
    pub fn in_world(&self) -> Vec2 {
        self.size.as_vec2() * self.metres
    }
}

/// Hangs a panel, hidden, at the world's origin facing +z; the caller puts it somewhere.
pub fn hang(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    look: Look,
) -> Hung {
    let pixels = (look.size.as_vec2() * look.scale).round().as_uvec2();
    let image = images.add(Image::new_target_texture(
        pixels.x,
        pixels.y,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    let camera = commands
        .spawn((
            Camera2d,
            Camera {
                // Before the cameras that show it.
                order: -1,
                is_active: false,
                clear_color: ClearColorConfig::Custom(look.background),
                ..default()
            },
            RenderTarget::Image(ImageRenderTarget {
                handle: image.clone(),
                scale_factor: look.scale,
            }),
        ))
        .id();
    let see_through = look.background.alpha() < 1.0;
    let quad = commands
        .spawn((
            Panel {
                size: look.in_world(),
                pixels: look.size,
                scale: look.scale,
                image: image.clone(),
            },
            Mesh3d(meshes.add(Rectangle::from_size(look.in_world()))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color_texture: Some(image),
                unlit: true,
                fog_enabled: false,
                // Where nothing is drawn, nothing is there.
                alpha_mode: if see_through {
                    AlphaMode::Mask(0.5)
                } else {
                    AlphaMode::Opaque
                },
                ..default()
            })),
            NotShadowCaster,
            NotShadowReceiver,
            Transform::default(),
            Visibility::Hidden,
        ))
        .id();
    Hung { camera, quad }
}

/// Shows a panel or hides it, its camera drawing only while it is shown.
pub fn show(
    hung: Hung,
    shown: bool,
    cameras: &mut Query<&mut Camera>,
    visibilities: &mut Query<&mut Visibility>,
) {
    if let Ok(mut camera) = cameras.get_mut(hung.camera)
        && camera.is_active != shown
    {
        camera.is_active = shown;
    }
    if let Ok(mut visibility) = visibilities.get_mut(hung.quad) {
        let wanted = if shown {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
}
