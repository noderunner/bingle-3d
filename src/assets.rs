//! The files in `assets/` the scene is drawn with, compiled into the binary (plan, decision 1) and
//! loaded through Bevy's asset server as `embedded://<path>`. They are made by the asset pipeline
//! (`assets-src/README.md`); the fonts are compiled in by `fonts.rs`.

use std::path::{Path, PathBuf};

use bevy::asset::io::embedded::EmbeddedAssetRegistry;
use bevy::prelude::*;

macro_rules! files {
    ($($path:literal),* $(,)?) => {
        /// Every file, by its path under `assets/`.
        pub const FILES: &[(&str, &[u8])] = &[
            $(($path, include_bytes!(concat!("../assets/", $path)))),*
        ];
    };
}

files![
    "appearance/amazons.toml",
    "appearance/chessmen.toml",
    "appearance/draughts.toml",
    "appearance/fist.toml",
    "appearance/hex.toml",
    "appearance/points.toml",
    "appearance/tak.toml",
    "environment/garden_diffuse.ktx2",
    "environment/garden_sky.ktx2",
    "environment/garden_specular.ktx2",
    "environment/room_diffuse.ktx2",
    "environment/room_specular.ktx2",
    "environments/garden.glb",
    "environments/garden.toml",
    "environments/room.toml",
    "models/bowl.glb",
    "models/bowl_stones.glb",
    "models/card.glb",
    "models/checkers/king.glb",
    "models/checkers/man.glb",
    "models/chess/bishop.glb",
    "models/chess/king.glb",
    "models/chess/knight.glb",
    "models/chess/pawn.glb",
    "models/chess/queen.glb",
    "models/chess/rook.glb",
    "models/die.glb",
    "models/stone.glb",
    "models/tak/flat.glb",
    "models/tak/wall.glb",
    "pictures/cards/points.jpg",
    "sounds/cheer.ogg",
    "sounds/groan.ogg",
    "sounds/key.ogg",
    "sounds/thump.ogg",
    "sounds/tick.ogg",
    "textures/cards/back_albedo.jpg",
    "textures/cards/faces/clubs_10_albedo.png",
    "textures/cards/faces/clubs_2_albedo.png",
    "textures/cards/faces/clubs_3_albedo.png",
    "textures/cards/faces/clubs_4_albedo.png",
    "textures/cards/faces/clubs_5_albedo.png",
    "textures/cards/faces/clubs_6_albedo.png",
    "textures/cards/faces/clubs_7_albedo.png",
    "textures/cards/faces/clubs_8_albedo.png",
    "textures/cards/faces/clubs_9_albedo.png",
    "textures/cards/faces/clubs_a_albedo.png",
    "textures/cards/faces/clubs_j_albedo.png",
    "textures/cards/faces/clubs_k_albedo.png",
    "textures/cards/faces/clubs_q_albedo.png",
    "textures/cards/faces/diamonds_10_albedo.png",
    "textures/cards/faces/diamonds_2_albedo.png",
    "textures/cards/faces/diamonds_3_albedo.png",
    "textures/cards/faces/diamonds_4_albedo.png",
    "textures/cards/faces/diamonds_5_albedo.png",
    "textures/cards/faces/diamonds_6_albedo.png",
    "textures/cards/faces/diamonds_7_albedo.png",
    "textures/cards/faces/diamonds_8_albedo.png",
    "textures/cards/faces/diamonds_9_albedo.png",
    "textures/cards/faces/diamonds_a_albedo.png",
    "textures/cards/faces/diamonds_j_albedo.png",
    "textures/cards/faces/diamonds_k_albedo.png",
    "textures/cards/faces/diamonds_q_albedo.png",
    "textures/cards/faces/hearts_10_albedo.png",
    "textures/cards/faces/hearts_2_albedo.png",
    "textures/cards/faces/hearts_3_albedo.png",
    "textures/cards/faces/hearts_4_albedo.png",
    "textures/cards/faces/hearts_5_albedo.png",
    "textures/cards/faces/hearts_6_albedo.png",
    "textures/cards/faces/hearts_7_albedo.png",
    "textures/cards/faces/hearts_8_albedo.png",
    "textures/cards/faces/hearts_9_albedo.png",
    "textures/cards/faces/hearts_a_albedo.png",
    "textures/cards/faces/hearts_j_albedo.png",
    "textures/cards/faces/hearts_k_albedo.png",
    "textures/cards/faces/hearts_q_albedo.png",
    "textures/cards/faces/spades_10_albedo.png",
    "textures/cards/faces/spades_2_albedo.png",
    "textures/cards/faces/spades_3_albedo.png",
    "textures/cards/faces/spades_4_albedo.png",
    "textures/cards/faces/spades_5_albedo.png",
    "textures/cards/faces/spades_6_albedo.png",
    "textures/cards/faces/spades_7_albedo.png",
    "textures/cards/faces/spades_8_albedo.png",
    "textures/cards/faces/spades_9_albedo.png",
    "textures/cards/faces/spades_a_albedo.png",
    "textures/cards/faces/spades_j_albedo.png",
    "textures/cards/faces/spades_k_albedo.png",
    "textures/cards/faces/spades_q_albedo.png",
    "textures/engraving/glyphs_albedo.png",
    "textures/engraving/glyphs_normal.png",
    "textures/garden/bamboo_albedo.png",
    "textures/garden/bark_albedo.jpg",
    "textures/garden/bark_arm.jpg",
    "textures/garden/bark_normal.jpg",
    "textures/garden/cane_albedo.jpg",
    "textures/garden/fence_albedo.jpg",
    "textures/garden/fence_arm.jpg",
    "textures/garden/fence_normal.jpg",
    "textures/garden/gravel_albedo.jpg",
    "textures/garden/gravel_arm.jpg",
    "textures/garden/gravel_normal.jpg",
    "textures/garden/hedge_albedo.jpg",
    "textures/garden/hedge_normal.jpg",
    "textures/garden/hills_albedo.png",
    "textures/garden/hinoki_albedo.jpg",
    "textures/garden/hinoki_arm.jpg",
    "textures/garden/hinoki_normal.jpg",
    "textures/garden/linen_albedo.jpg",
    "textures/garden/linen_arm.jpg",
    "textures/garden/linen_normal.jpg",
    "textures/garden/maple_albedo.png",
    "textures/garden/moss_albedo.jpg",
    "textures/garden/moss_arm.jpg",
    "textures/garden/moss_normal.jpg",
    "textures/garden/rock_albedo.jpg",
    "textures/garden/rock_arm.jpg",
    "textures/garden/rock_normal.jpg",
    "textures/garden/stone_albedo.jpg",
    "textures/garden/stone_arm.jpg",
    "textures/garden/stone_normal.jpg",
    "textures/garden/tatami_albedo.jpg",
    "textures/garden/tatami_arm.jpg",
    "textures/garden/tatami_normal.jpg",
    "textures/garden/treeline_albedo.png",
    "textures/garden/water_normal.jpg",
    "textures/stone/shell_albedo.jpg",
    "textures/stone/slate_albedo.jpg",
    "textures/wood/cherry_albedo.jpg",
    "textures/wood/cherry_arm.jpg",
    "textures/wood/cherry_normal.jpg",
    "textures/wood/felt_albedo.jpg",
    "textures/wood/felt_arm.jpg",
    "textures/wood/felt_normal.jpg",
    "textures/wood/maple_albedo.jpg",
    "textures/wood/maple_arm.jpg",
    "textures/wood/maple_normal.jpg",
    "textures/wood/teak_albedo.jpg",
    "textures/wood/teak_arm.jpg",
    "textures/wood/teak_normal.jpg",
    "textures/wood/walnut_albedo.jpg",
    "textures/wood/walnut_arm.jpg",
    "textures/wood/walnut_normal.jpg",
];

/// The path the asset server loads a file by.
pub fn path(file: &str) -> String {
    format!("embedded://{file}")
}

/// Whether the client has this file.
pub fn exists(file: &str) -> bool {
    FILES.iter().any(|(f, _)| *f == file)
}

/// A file's contents as text, if it is one of ours and is UTF-8.
pub fn text(file: &str) -> Option<&'static str> {
    FILES
        .iter()
        .find(|(f, _)| *f == file)
        .and_then(|(_, bytes)| std::str::from_utf8(bytes).ok())
}

/// The JSON part of one of our models: a binary glTF file is a 12-byte header, then chunks,
/// each its length, its kind and its bytes, the JSON first.
fn model_json(file: &str) -> Option<serde_json::Value> {
    let bytes = FILES.iter().find(|(f, _)| *f == file)?.1;
    if bytes.get(..4)? != b"glTF" || bytes.get(16..20)? != b"JSON" {
        return None;
    }
    let length = u32::from_le_bytes(bytes.get(12..16)?.try_into().ok()?) as usize;
    serde_json::from_slice(bytes.get(20..20 + length)?).ok()
}

/// How low and how high a model's first mesh reaches (y), in metres: where its foot is, to
/// stand it on something, and how much it raises what stands on it.
pub fn model_span(file: &str) -> Option<(f32, f32)> {
    let doc = model_json(file)?;
    let mut span: Option<(f32, f32)> = None;
    for part in doc["meshes"][0]["primitives"].as_array()? {
        // glTF gives every accessor of positions its least and greatest values.
        let positions = &doc["accessors"][part["attributes"]["POSITION"].as_u64()? as usize];
        let low = positions["min"][1].as_f64()? as f32;
        let high = positions["max"][1].as_f64()? as f32;
        span = Some(span.map_or((low, high), |(a, b)| (a.min(low), b.max(high))));
    }
    span
}

/// The parts of a model's first mesh, in order, each by the name of its material: a card's
/// face, back and edge. A model with no materials has one part, with no name.
pub fn model_parts(file: &str) -> Vec<String> {
    let Some(doc) = model_json(file) else {
        return Vec::new();
    };
    let name = |part: &serde_json::Value| {
        part["material"]
            .as_u64()
            .and_then(|m| doc["materials"][m as usize]["name"].as_str())
            .unwrap_or_default()
            .to_string()
    };
    doc["meshes"][0]["primitives"]
        .as_array()
        .map(|parts| parts.iter().map(name).collect())
        .unwrap_or_default()
}

pub struct AssetsPlugin;

impl Plugin for AssetsPlugin {
    fn build(&self, app: &mut App) {
        let registry = app.world_mut().resource_mut::<EmbeddedAssetRegistry>();
        for (file, bytes) in FILES {
            registry.insert_asset(PathBuf::new(), Path::new(file), *bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every file under `assets/`, but the fonts (compiled in by `fonts.rs`) and the credits.
    fn on_disk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if path.is_dir() {
                if name != "fonts" {
                    on_disk(&path, root, out);
                }
            } else if name != "CREDITS.md" {
                out.push(name);
            }
        }
    }

    #[test]
    fn every_file_in_assets_is_compiled_in_and_no_other() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        let mut found = Vec::new();
        on_disk(&root, &root, &mut found);
        found.sort();
        let mut listed: Vec<String> = FILES.iter().map(|(f, _)| f.to_string()).collect();
        listed.sort();
        assert_eq!(found, listed, "list each file of assets/ in FILES");
    }

    #[test]
    fn a_model_says_how_tall_it_is_and_what_its_parts_are() {
        // The stone is centred on its middle, 8.8 mm thick.
        let (low, high) = model_span("models/stone.glb").unwrap();
        assert!((low + 0.0044).abs() < 1e-5 && (high - 0.0044).abs() < 1e-5);
        assert_eq!(model_parts("models/stone.glb"), [""]);
        assert_eq!(model_parts("models/card.glb"), ["face", "back", "edge"]);
        assert_eq!(model_span("appearance/hex.toml"), None, "not a model");
        assert_eq!(model_span("models/nothing.glb"), None);
        assert!(model_parts("models/nothing.glb").is_empty());
        // Every model can be stood on the table.
        for (file, _) in FILES.iter().filter(|(f, _)| f.starts_with("models/")) {
            let (low, high) = model_span(file).unwrap_or_else(|| panic!("{file}"));
            assert!(high > low, "{file}");
        }
    }

    #[test]
    fn a_file_is_found_by_its_path() {
        assert!(exists("models/stone.glb"));
        assert!(!exists("models/nothing.glb"));
        assert!(text("appearance/hex.toml").unwrap().contains("games"));
        assert_eq!(path("models/stone.glb"), "embedded://models/stone.glb");
    }
}
