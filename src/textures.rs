//! Textures with mipmaps. Bevy reads a PNG or a JPEG as a single level, and a board's grain seen
//! from across the table, or at a slant, then shimmers. Each of our textures (an `embedded://`
//! file under `textures/`) gets its smaller levels made here when it has loaded, and a sampler
//! that filters between them and across them at a slant.
//!
//! A colour texture (`…_albedo`) is averaged in linear light, as the GPU reads it; the others
//! (normals, roughness) as stored.
//!
//! Bevy builds a material's GPU side once, from the images it had then, and does not build it
//! again when one of them changes; so each material using a texture given mipmaps here is marked
//! changed too.

use bevy::asset::AssetPath;
use bevy::image::{
    ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor,
};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use crate::assets;

pub struct TexturesPlugin;

impl Plugin for TexturesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreUpdate, add_mipmaps);
    }
}

/// Loads one of our textures: in sRGB if it is a colour, as data if not.
pub fn load(server: &AssetServer, file: &str) -> Handle<Image> {
    let srgb = is_colour(file);
    server
        .load_builder()
        .with_settings(move |s: &mut ImageLoaderSettings| s.is_srgb = srgb)
        .load(assets::path(file))
}

/// Whether a texture holds colours (in sRGB), by our naming, rather than data.
pub fn is_colour(file: &str) -> bool {
    file.rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .is_some_and(|(stem, _)| stem.ends_with("_albedo"))
}

fn add_mipmaps(
    mut events: MessageReader<AssetEvent<Image>>,
    server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut done = Vec::new();
    for event in events.read() {
        let AssetEvent::LoadedWithDependencies { id } = event else {
            continue;
        };
        let Some(path) = server.get_path(*id) else {
            continue;
        };
        if !is_ours(&path) {
            continue;
        }
        let Some(image) = images.get(*id) else {
            continue;
        };
        let format = image.texture_descriptor.format;
        let size = image.texture_descriptor.size;
        if image.texture_descriptor.mip_level_count > 1
            || !matches!(
                format,
                TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb
            )
            || size.depth_or_array_layers != 1
        {
            continue;
        }
        let Some(data) = image.data.as_deref() else {
            continue;
        };
        let (data, levels) = mip_chain(
            data,
            size.width,
            size.height,
            format == TextureFormat::Rgba8UnormSrgb,
        );
        let Some(mut image) = images.get_mut(*id) else {
            continue;
        };
        image.data = Some(data);
        image.texture_descriptor.mip_level_count = levels;
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Linear,
            anisotropy_clamp: 16,
            ..default()
        });
        done.push(*id);
    }
    if done.is_empty() {
        return;
    }
    let stale: Vec<AssetId<StandardMaterial>> = materials
        .iter()
        .filter(|(_, m)| uses_any(m, &done))
        .map(|(id, _)| id)
        .collect();
    for id in stale {
        // Marked changed, its GPU side is built again from the new images.
        if let Some(material) = materials.get_mut(id) {
            material.into_inner();
        }
    }
}

/// Whether a material uses any of these images.
fn uses_any(material: &StandardMaterial, images: &[AssetId<Image>]) -> bool {
    [
        &material.base_color_texture,
        &material.normal_map_texture,
        &material.metallic_roughness_texture,
        &material.occlusion_texture,
        &material.emissive_texture,
    ]
    .into_iter()
    .flatten()
    .any(|h| images.contains(&h.id()))
}

fn is_ours(path: &AssetPath) -> bool {
    path.source().as_str() == Some(bevy::asset::io::embedded::EMBEDDED)
        && path.path().starts_with("textures")
}

/// An RGBA8 image and every smaller level after it, each half the size of the one before (and
/// at least 1) down to 1×1, each texel the average of the two by two above it. With `srgb`, the
/// average is taken in linear light and alpha as it is. Returns the data and the level count.
pub fn mip_chain(rgba: &[u8], width: u32, height: u32, srgb: bool) -> (Vec<u8>, u32) {
    let to_linear: Vec<f32> = (0..=255u8)
        .map(|v| {
            let c = v as f32 / 255.0;
            if srgb {
                if c <= 0.04045 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            } else {
                c
            }
        })
        .collect();
    let from_linear = |c: f32| -> u8 {
        let c = c.clamp(0.0, 1.0);
        let e = if srgb {
            if c <= 0.003_130_8 {
                c * 12.92
            } else {
                1.055 * c.powf(1.0 / 2.4) - 0.055
            }
        } else {
            c
        };
        (e * 255.0).round() as u8
    };

    let mut out = rgba.to_vec();
    let (mut w, mut h) = (width as usize, height as usize);
    let mut start = 0;
    let mut levels = 1;
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = Vec::with_capacity(nw * nh * 4);
        for y in 0..nh {
            for x in 0..nw {
                let (x0, y0) = (x * 2, y * 2);
                let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
                let texel = |xx: usize, yy: usize, c: usize| out[start + (yy * w + xx) * 4 + c];
                for c in 0..4 {
                    let at = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)];
                    if c == 3 || !srgb {
                        let sum: u32 = at.iter().map(|&(a, b)| texel(a, b, c) as u32).sum();
                        next.push(((sum + 2) / 4) as u8);
                    } else {
                        let sum: f32 = at
                            .iter()
                            .map(|&(a, b)| to_linear[texel(a, b, c) as usize])
                            .sum();
                        next.push(from_linear(sum / 4.0));
                    }
                }
            }
        }
        start = out.len();
        out.extend_from_slice(&next);
        w = nw;
        h = nh;
        levels += 1;
    }
    (out, levels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_level_down_to_one_texel() {
        let image: Vec<u8> = (0..8 * 4 * 4).map(|i| i as u8).collect();
        let (data, levels) = mip_chain(&image, 8, 4, false);
        // 8×4, 4×2, 2×1, 1×1.
        assert_eq!(levels, 4);
        assert_eq!(data.len(), (32 + 8 + 2 + 1) * 4);
        assert_eq!(
            &data[..image.len()],
            &image[..],
            "the first level as it was"
        );
    }

    #[test]
    fn a_level_is_the_average_of_the_one_above() {
        // A 2×2 of black and white: grey, as data, and lighter in sRGB, where the eye's
        // half-way is not the number half-way.
        let image = [
            0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255, 255, 255, 255, 255,
        ];
        let (data, _) = mip_chain(&image, 2, 2, false);
        assert_eq!(&data[16..], &[128, 128, 128, 255]);
        let (data, _) = mip_chain(&image, 2, 2, true);
        assert_eq!(&data[16..], &[188, 188, 188, 255]);
    }

    #[test]
    fn an_odd_size_keeps_its_last_row_and_column() {
        let image = vec![200u8; 3 * 3 * 4];
        let (data, levels) = mip_chain(&image, 3, 3, true);
        assert_eq!(levels, 2);
        assert_eq!(&data[36..], &[200, 200, 200, 200]);
    }

    #[test]
    fn a_material_is_known_to_use_its_textures() {
        let mut images = Assets::<Image>::default();
        let a = images.add(Image::default());
        let b = images.add(Image::default());
        let m = StandardMaterial {
            normal_map_texture: Some(a.clone()),
            ..default()
        };
        assert!(uses_any(&m, &[b.id(), a.id()]));
        assert!(!uses_any(&m, &[b.id()]));
        assert!(!uses_any(&StandardMaterial::default(), &[a.id()]));
    }

    mod systems {
        use super::*;

        /// An app that has loaded one of our textures and a material using it, as the asset
        /// server would leave them.
        #[test]
        fn a_loaded_texture_gets_mipmaps_and_its_material_is_built_again() {
            let mut app = App::new();
            app.add_plugins((MinimalPlugins, AssetPlugin::default(), TexturesPlugin))
                .init_asset::<Image>()
                .init_asset::<StandardMaterial>();
            crate::assets::AssetsPlugin.build(&mut app);
            let handle: Handle<Image> = load(
                app.world().resource::<AssetServer>(),
                "textures/stone/slate_albedo.jpg",
            );
            let material = app
                .world_mut()
                .resource_mut::<Assets<StandardMaterial>>()
                .add(StandardMaterial {
                    base_color_texture: Some(handle.clone()),
                    ..default()
                });
            // No image loader here (that is the render plugins'): put in the image the loader
            // would, under the handle the server gave, and say it has loaded.
            let image = Image::new_fill(
                bevy::render::render_resource::Extent3d {
                    width: 4,
                    height: 4,
                    depth_or_array_layers: 1,
                },
                bevy::render::render_resource::TextureDimension::D2,
                &[10, 20, 30, 255],
                TextureFormat::Rgba8UnormSrgb,
                bevy::asset::RenderAssetUsages::default(),
            );
            app.world_mut()
                .resource_mut::<Assets<Image>>()
                .insert(handle.id(), image)
                .unwrap();
            app.world_mut()
                .resource_mut::<Messages<AssetEvent<Image>>>()
                .write(AssetEvent::LoadedWithDependencies { id: handle.id() });
            app.world_mut()
                .resource_mut::<Messages<AssetEvent<StandardMaterial>>>()
                .clear();
            app.update();

            let images = app.world().resource::<Assets<Image>>();
            let image = images.get(&handle).unwrap();
            assert_eq!(image.texture_descriptor.mip_level_count, 3, "4×4, 2×2, 1×1");
            let ImageSampler::Descriptor(sampler) = &image.sampler else {
                panic!("a sampler of its own");
            };
            assert_eq!(sampler.address_mode_u, ImageAddressMode::Repeat);
            let changed: Vec<_> = app
                .world_mut()
                .resource_mut::<Messages<AssetEvent<StandardMaterial>>>()
                .drain()
                .collect();
            assert!(
                changed
                    .iter()
                    .any(|e| *e == AssetEvent::Modified { id: material.id() }),
                "{changed:?}"
            );
        }
    }

    #[test]
    fn colour_textures_are_known_by_name() {
        assert!(is_colour("textures/wood/teak_albedo.jpg"));
        assert!(!is_colour("textures/wood/teak_normal.jpg"));
        assert!(!is_colour("textures/wood/teak_arm.jpg"));
        assert!(!is_colour("albedo"));
    }
}
