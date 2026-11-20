//! Generic defaults for how things look, for any game (Bingle's ADR 0124): the server says what
//! things are, and this client decides how they look. A per-game appearance (`appearance.rs`)
//! may improve on them; no game needs one.
//!
//! Every board is a slab of wood on a felt-covered table, and every piece a stone in its seat's
//! colour.

use bevy::math::Affine2;

use bevy::prelude::*;
use serde::Deserialize;

use crate::{assets, textures};

/// A wood: scanned textures made by the asset pipeline (`assets-src/blender/woods.py`), as
/// `textures/wood/<name>_{albedo,normal,arm}.jpg`, the size in metres one copy of them covers, and
/// its finish: the colour (sRGB) oil gives the raw veneer that was scanned, deeper and warmer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Wood {
    pub name: &'static str,
    pub metres: f32,
    pub finish: [f32; 3],
}

pub const WOODS: [Wood; 6] = [
    // Boards: golden teak, dark walnut, open-grained oak, warm cherry (`BOARD_WOODS`).
    Wood {
        name: "teak",
        metres: 1.0,
        finish: [1.0, 0.93, 0.84],
    },
    Wood {
        name: "walnut",
        metres: 1.0,
        finish: [0.58, 0.44, 0.35],
    },
    Wood {
        name: "oak",
        metres: 1.83,
        finish: [0.97, 0.88, 0.74],
    },
    Wood {
        name: "cherry",
        metres: 1.0,
        finish: [0.78, 0.55, 0.46],
    },
    // Pale.
    Wood {
        name: "maple",
        metres: 1.0,
        finish: [1.0, 1.0, 1.0],
    },
    // Not a wood: the table's felt, made and mapped the same way.
    Wood {
        name: "felt",
        metres: 0.285,
        finish: [1.0, 1.0, 1.0],
    },
];

/// The woods a board may be made of, the first by default.
pub const BOARD_WOODS: [&str; 4] = ["teak", "walnut", "oak", "cherry"];

/// The wood a material names as `board`: whichever the board is made of.
pub const BOARD: &str = "board";

/// How much finer the grain's relief (its normal map) is than its colour: the pores and the
/// small ripples a close look sees, laid over the figure without stretching a single photograph.
/// Not a whole number, so the two never line up.
pub const DETAIL: f32 = 3.17;

pub fn wood(name: &str) -> Option<Wood> {
    WOODS.iter().copied().find(|w| w.name == name)
}

/// A material as data: generic looks are written as these, and appearance files hold them.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialSpec {
    /// A colour texture, by its path under `assets/`.
    pub albedo: Option<String>,
    /// One of `WOODS`, or `board` for the board's own: its colour, normals and roughness,
    /// mapped by position on the table, so a mesh using it needs texture coordinates in metres
    /// (`world_uvs`).
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
    /// How much light passes through it to the other side, as through a shell (0 to 1).
    pub translucency: Option<f32>,
    /// Light of its own, as a lit cell's: linear RGB added to what the camera sees (Bevy does not
    /// scale an emissive colour by the exposure), so 1 is about white.
    pub glow: Option<[f32; 3]>,
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
            && name != BOARD
            && wood(name).is_none()
        {
            out.push(format!("no wood {name}"));
        }
        if self.albedo.is_some() && self.wood.is_some() {
            out.push("both a texture and a wood".into());
        }
        out
    }

    /// The same, with `board` for a wood replaced by the board's wood.
    pub fn on_board(&self, board_wood: &str) -> MaterialSpec {
        let mut out = self.clone();
        if out.wood.as_deref() == Some(BOARD) {
            out.wood = Some(board_wood.to_string());
        }
        out
    }

    /// Whether it is made of the board's wood, whichever that is.
    pub fn is_board_wood(&self) -> bool {
        self.wood.as_deref() == Some(BOARD)
    }

    /// Builds it. A `board` wood here is the first of `BOARD_WOODS`: use `on_board` first to
    /// choose another.
    pub fn build(&self, server: &AssetServer) -> StandardMaterial {
        let tint = self.tint.unwrap_or([1.0; 3]);
        let mut m = StandardMaterial {
            base_color: Color::srgb(tint[0], tint[1], tint[2]),
            perceptual_roughness: self.roughness.unwrap_or(0.5),
            reflectance: self.reflectance.unwrap_or(0.5),
            clearcoat: self.clearcoat.unwrap_or(0.0),
            clearcoat_perceptual_roughness: self.clearcoat_roughness.unwrap_or(0.5),
            diffuse_transmission: self.translucency.unwrap_or(0.0),
            emissive: self
                .glow
                .map_or(LinearRgba::BLACK, |[r, g, b]| LinearRgba::rgb(r, g, b)),
            ..default()
        };
        if let Some(file) = &self.albedo {
            m.base_color_texture = Some(textures::load(server, file));
        }
        let name = match self.wood.as_deref() {
            Some(BOARD) => Some(BOARD_WOODS[0]),
            other => other,
        };
        if let Some(w) = name.and_then(wood) {
            let file = |kind: &str| format!("textures/wood/{}_{kind}.jpg", w.name);
            m.base_color_texture = Some(textures::load(server, &file("albedo")));
            // The relief, finer (`DETAIL`), from the second set of texture coordinates.
            m.normal_map_texture = Some(textures::load(server, &file("normal")));
            m.normal_map_channel = bevy::mesh::UvChannel::Uv1;
            let arm = textures::load(server, &file("arm"));
            m.metallic_roughness_texture = Some(arm.clone());
            m.occlusion_texture = Some(arm);
            m.perceptual_roughness = self.roughness.unwrap_or(1.0);
            m.uv_transform = Affine2::from_scale(Vec2::splat(1.0 / w.metres));
            let [r, g, b] = w.finish;
            m.base_color = Color::srgb(tint[0] * r, tint[1] * g, tint[2] * b);
        }
        m
    }
}

/// Gives a mesh texture coordinates in metres by where each vertex will be on the table (its
/// position plus `at`), for a wood: a face looking up or down is mapped from above, one looking
/// sideways from that side, so the grain runs on across neighbouring meshes and round edges, and
/// is never stretched. A second set, `DETAIL` times finer, is for the grain's relief. Adds the
/// tangents a normal map needs.
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
    let detail: Vec<[f32; 2]> = uvs.iter().map(|[u, v]| [u * DETAIL, v * DETAIL]).collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, detail);
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

/// The board: its wood (whichever is chosen), oiled and polished.
pub fn board() -> MaterialSpec {
    MaterialSpec {
        wood: Some(BOARD.into()),
        clearcoat: Some(0.5),
        clearcoat_roughness: Some(0.18),
        ..default()
    }
}

/// The lines between the cells, cut into the board and darkened.
pub fn groove() -> MaterialSpec {
    MaterialSpec {
        tint: Some([0.30, 0.22, 0.17]),
        ..board()
    }
}

/// A cell a click would be taken on: the board, lit warmly, plainly enough to see at a glance.
pub fn lit_cell() -> MaterialSpec {
    MaterialSpec {
        glow: Some([0.26, 0.22, 0.07]),
        ..board()
    }
}

/// The table the board stands on: felt, which takes the light and gives back little, so the
/// board stands out on it.
pub fn table() -> MaterialSpec {
    MaterialSpec {
        wood: Some("felt".into()),
        // A deep green-blue, darker than the scan, so the board is the brightest thing on it.
        tint: Some([0.35, 0.42, 0.40]),
        reflectance: Some(0.3),
        ..default()
    }
}

/// A goal's edge of the board (its rim and sides), stained in its seat's colour (or a plain dark
/// stain for a goal every seat shares). Satin, not gloss: the sides face the room's window, and a
/// gloss there shows the window rather than the wood.
pub fn goal(colour: Option<Color>) -> MaterialSpec {
    let stain = colour.map_or([0.25, 0.22, 0.20], |c| {
        let c = c.to_srgba();
        [c.red, c.green, c.blue]
    });
    MaterialSpec {
        tint: Some(stain),
        clearcoat: Some(0.2),
        clearcoat_roughness: Some(0.4),
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
        let looks = [
            board(),
            groove(),
            lit_cell(),
            table(),
            goal(None),
            piece(Color::BLACK),
        ];
        for look in looks {
            assert!(
                look.problems().is_empty(),
                "{look:?}: {:?}",
                look.problems()
            );
        }
        assert!(assets::exists(PIECE_MODEL));
        assert!(BOARD_WOODS.iter().all(|w| wood(w).is_some()));
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
    fn the_board_wood_is_whichever_is_chosen() {
        let ebony = MaterialSpec {
            wood: Some(BOARD.into()),
            tint: Some([0.2, 0.1, 0.1]),
            ..default()
        };
        assert!(ebony.problems().is_empty());
        assert!(ebony.is_board_wood());
        let on_oak = ebony.on_board("oak");
        assert_eq!(on_oak.wood.as_deref(), Some("oak"));
        assert_eq!(on_oak.tint, ebony.tint);
        assert!(!on_oak.is_board_wood());
        assert_eq!(
            table().on_board("oak"),
            table(),
            "another wood stays itself"
        );
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
