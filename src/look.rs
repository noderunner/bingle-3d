//! Generic defaults for how things look, for any game (Bingle's ADR 0124): the server says what
//! things are, and this client decides how they look. A per-game appearance (`appearance.rs`)
//! may improve on them; no game needs one.
//!
//! Every board is wood on a darker wooden table, and every piece a stone in its seat's colour.

use bevy::math::Affine2;
use bevy::prelude::*;
use serde::Deserialize;

use crate::{assets, textures};

/// A wood: scanned textures made by the asset pipeline (`assets-src/blender/woods.py`), as
/// `textures/wood/<name>_{albedo,normal,arm}.jpg`, and the size in metres one copy of them covers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Wood {
    pub name: &'static str,
    pub metres: f32,
}

pub const WOODS: [Wood; 3] = [
    // The board: golden, fine and even.
    Wood {
        name: "teak",
        metres: 1.0,
    },
    // Pale.
    Wood {
        name: "maple",
        metres: 1.0,
    },
    // The table: dark and reddish.
    Wood {
        name: "dark",
        metres: 2.0,
    },
];

pub fn wood(name: &str) -> Option<Wood> {
    WOODS.iter().copied().find(|w| w.name == name)
}

/// A material as data: generic looks are written as these, and appearance files hold them.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialSpec {
    /// A colour texture, by its path under `assets/`.
    pub albedo: Option<String>,
    /// One of `WOODS`: its colour, normals and roughness, mapped by position on the table, so
    /// a mesh using it needs texture coordinates in metres (`world_uvs`).
    pub wood: Option<String>,
    /// Multiplies the colour (sRGB), as a stain does; the whole colour when there is no texture.
    pub tint: Option<[f32; 3]>,
    /// Perceptual roughness; with a wood, it scales the wood's own.
    pub roughness: Option<f32>,
    /// Bevy's reflectance: 0.5 is most things; less for a surface that reflects little.
    pub reflectance: Option<f32>,
    /// A clear coat over it: how much, and how rough. Polish.
    pub clearcoat: Option<f32>,
    pub clearcoat_roughness: Option<f32>,
}

impl MaterialSpec {
    /// What is wrong with it, if anything: a texture or wood the client does not have.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(file) = &self.albedo
            && !assets::exists(file)
        {
            out.push(format!("no texture {file}"));
        }
        if let Some(name) = &self.wood
            && wood(name).is_none()
        {
            out.push(format!("no wood {name}"));
        }
        if self.albedo.is_some() && self.wood.is_some() {
            out.push("both a texture and a wood".into());
        }
        out
    }

    pub fn build(&self, server: &AssetServer) -> StandardMaterial {
        let tint = self
            .tint
            .map_or(Color::WHITE, |[r, g, b]| Color::srgb(r, g, b));
        let mut m = StandardMaterial {
            base_color: tint,
            perceptual_roughness: self.roughness.unwrap_or(0.5),
            reflectance: self.reflectance.unwrap_or(0.5),
            clearcoat: self.clearcoat.unwrap_or(0.0),
            clearcoat_perceptual_roughness: self.clearcoat_roughness.unwrap_or(0.5),
            ..default()
        };
        if let Some(file) = &self.albedo {
            m.base_color_texture = Some(textures::load(server, file));
        }
        if let Some(w) = self.wood.as_deref().and_then(wood) {
            let file = |kind: &str| format!("textures/wood/{}_{kind}.jpg", w.name);
            m.base_color_texture = Some(textures::load(server, &file("albedo")));
            m.normal_map_texture = Some(textures::load(server, &file("normal")));
            let arm = textures::load(server, &file("arm"));
            m.metallic_roughness_texture = Some(arm.clone());
            m.occlusion_texture = Some(arm);
            m.perceptual_roughness = self.roughness.unwrap_or(1.0);
            m.uv_transform = Affine2::from_scale(Vec2::splat(1.0 / w.metres));
        }
        m
    }
}

/// Gives a mesh texture coordinates in metres by where each vertex will be on the table (its
/// position plus `at`), for a wood: a face looking up or down is mapped from above, one looking
/// sideways from that side, so the grain runs on across neighbouring meshes and round edges, and
/// is never stretched. Adds the tangents a normal map needs.
pub fn world_uvs(mut mesh: Mesh, at: Vec3) -> Mesh {
    use bevy::mesh::VertexAttributeValues;
    let (
        Some(VertexAttributeValues::Float32x3(positions)),
        Some(VertexAttributeValues::Float32x3(normals)),
    ) = (
        mesh.attribute(Mesh::ATTRIBUTE_POSITION),
        mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
    )
    else {
        return mesh;
    };
    let uvs: Vec<[f32; 2]> = positions
        .iter()
        .zip(normals)
        .map(|(p, n)| planar_uv(Vec3::from_array(*p) + at, Vec3::from_array(*n)))
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    if let Err(e) = mesh.generate_tangents() {
        warn!("no tangents for a wooden mesh: {e}");
    }
    mesh
}

/// A point's texture coordinates in metres, seen along the axis its normal is closest to.
pub fn planar_uv(p: Vec3, n: Vec3) -> [f32; 2] {
    let a = n.abs();
    if a.y >= a.x && a.y >= a.z {
        [p.x, p.z]
    } else if a.x >= a.z {
        [p.z, p.y]
    } else {
        [p.x, p.y]
    }
}

/// The model every piece is drawn as unless an appearance says otherwise: a stone, lens-shaped,
/// made to fill most of a 28 mm cell (`assets-src/blender/stones.py`).
pub const PIECE_MODEL: &str = "models/stone.glb";

/// The board's cells: oiled teak.
pub fn board() -> MaterialSpec {
    MaterialSpec {
        wood: Some("teak".into()),
        clearcoat: Some(0.4),
        clearcoat_roughness: Some(0.25),
        ..default()
    }
}

/// The board beneath its cells, seen in the grooves between them: the same wood, darker.
pub fn groove() -> MaterialSpec {
    MaterialSpec {
        tint: Some([0.45, 0.36, 0.30]),
        ..board()
    }
}

/// The table the board stands on: worn rather than polished, so it does not outshine the board.
pub fn table() -> MaterialSpec {
    MaterialSpec {
        wood: Some("dark".into()),
        roughness: Some(1.6),
        reflectance: Some(0.35),
        ..default()
    }
}

/// A goal's edge of the board, stained in its seat's colour (or a plain dark stain for a goal
/// every seat shares).
pub fn goal(colour: Option<Color>) -> MaterialSpec {
    let stain = colour.map_or([0.25, 0.22, 0.20], |c| {
        let c = c.to_srgba();
        [c.red, c.green, c.blue]
    });
    MaterialSpec {
        tint: Some(stain),
        ..board()
    }
}

/// A piece in its seat's colour, polished.
pub fn piece(colour: Color) -> MaterialSpec {
    let c = colour.to_srgba();
    MaterialSpec {
        tint: Some([c.red, c.green, c.blue]),
        roughness: Some(0.35),
        clearcoat: Some(0.3),
        clearcoat_roughness: Some(0.15),
        ..default()
    }
}

/// The colour a seat is drawn in. A seat's colour word is an identifier, not a colour, but most
/// games name their seats after the colour players know them by, so a word this client knows is
/// drawn as that colour. Any other word gets a colour from a palette by seat, so that every seat
/// still looks different.
pub fn seat_colour(word: &str, seat: u32) -> Color {
    named(word).unwrap_or_else(|| {
        let i = (seat.max(1) - 1) as usize % PALETTE.len();
        PALETTE[i]
    })
}

/// Distinct, fairly saturated colours that read well on a light board and on each other.
const PALETTE: [Color; 8] = [
    Color::srgb(0.80, 0.18, 0.16),
    Color::srgb(0.15, 0.33, 0.78),
    Color::srgb(0.95, 0.75, 0.15),
    Color::srgb(0.20, 0.60, 0.30),
    Color::srgb(0.55, 0.30, 0.70),
    Color::srgb(0.95, 0.50, 0.15),
    Color::srgb(0.15, 0.65, 0.70),
    Color::srgb(0.85, 0.40, 0.60),
];

fn named(word: &str) -> Option<Color> {
    Some(match word.to_ascii_lowercase().as_str() {
        "red" => PALETTE[0],
        "blue" => PALETTE[1],
        "yellow" | "gold" => PALETTE[2],
        "green" => PALETTE[3],
        "purple" | "violet" => PALETTE[4],
        "orange" => PALETTE[5],
        "cyan" | "teal" => PALETTE[6],
        "pink" => PALETTE[7],
        "white" => Color::srgb(0.92, 0.91, 0.87),
        "black" => Color::srgb(0.08, 0.08, 0.09),
        "grey" | "gray" | "silver" => Color::srgb(0.55, 0.56, 0.58),
        "brown" => Color::srgb(0.45, 0.28, 0.15),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_generic_looks_use_only_what_the_client_has() {
        let looks = [board(), groove(), table(), goal(None), piece(Color::BLACK)];
        for look in looks {
            assert!(
                look.problems().is_empty(),
                "{look:?}: {:?}",
                look.problems()
            );
        }
        assert!(assets::exists(PIECE_MODEL));
        for w in WOODS {
            for kind in ["albedo", "normal", "arm"] {
                assert!(assets::exists(&format!(
                    "textures/wood/{}_{kind}.jpg",
                    w.name
                )));
            }
        }
    }

    #[test]
    fn wood_is_mapped_from_the_side_a_face_looks_to() {
        let p = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(planar_uv(p, Vec3::Y), [1.0, 3.0]);
        assert_eq!(planar_uv(p, Vec3::NEG_Y), [1.0, 3.0]);
        assert_eq!(planar_uv(p, Vec3::X), [3.0, 2.0]);
        assert_eq!(planar_uv(p, Vec3::NEG_Z), [1.0, 2.0]);
        let mesh = world_uvs(
            Cuboid::new(1.0, 1.0, 1.0).mesh().build(),
            Vec3::new(5.0, 0.0, 0.0),
        );
        let Some(bevy::mesh::VertexAttributeValues::Float32x2(uvs)) =
            mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            panic!("the mesh has texture coordinates");
        };
        assert!(uvs.iter().all(|[u, _]| *u >= -0.5 && *u <= 5.5));
        assert!(mesh.attribute(Mesh::ATTRIBUTE_TANGENT).is_some());
    }

    #[test]
    fn a_material_naming_what_is_not_there_says_so() {
        let spec = MaterialSpec {
            albedo: Some("textures/none.jpg".into()),
            wood: Some("balsa".into()),
            ..default()
        };
        assert_eq!(spec.problems().len(), 3);
    }

    #[test]
    fn known_words_are_their_colour_and_others_differ_by_seat() {
        assert_eq!(seat_colour("Red", 2), seat_colour("red", 1));
        assert_ne!(seat_colour("red", 1), seat_colour("blue", 2));
        assert_eq!(seat_colour("cthonic", 1), PALETTE[0]);
        assert_ne!(seat_colour("cthonic", 1), seat_colour("abyssal", 2));
        assert_eq!(seat_colour("x", 9), PALETTE[0], "the palette goes round");
        assert_eq!(seat_colour("x", 0), PALETTE[0]);
    }
}
