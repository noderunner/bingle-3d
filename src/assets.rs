//! The files in `assets/` the scene is drawn with, compiled into the binary (plan, decision 1) and
//! loaded through Bevy's asset server as `embedded://<path>`. They are made by the asset pipeline
//! (`assets-src/README.md`); the fonts are compiled in by `fonts.rs`.
//!
//! Beside them, the private folder (plan, decision 2): the looks of games this repository may
//! not hold (Bingle's ADR 0127), read once at start-up and found by the same paths, so a private
//! game's appearance file, its models and its textures are used as ours are.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

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
    "appearance/blooms.toml",
    "appearance/bowling-solitaire.toml",
    "appearance/chessmen.toml",
    "appearance/courtfall.toml",
    "appearance/draughts.toml",
    "appearance/fendo.toml",
    "appearance/fist.toml",
    "appearance/hex.toml",
    "appearance/points.toml",
    "appearance/reversi.toml",
    "appearance/stones.toml",
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
    "models/bowl_fences.glb",
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
    "models/disc.glb",
    "models/disc_large.glb",
    "models/fence.glb",
    "models/ring.glb",
    "models/stone.glb",
    "models/tak/flat.glb",
    "models/tak/wall.glb",
    "models/tile_hex.glb",
    "pictures/cards/points.jpg",
    "sounds/cheer.ogg",
    "sounds/groan.ogg",
    "sounds/key.ogg",
    "sounds/thump.ogg",
    "sounds/tick.ogg",
    "sounds/tock.ogg",
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
    "textures/stone/black_slate_albedo.jpg",
    "textures/stone/jasper_albedo.jpg",
    "textures/stone/lapis_albedo.jpg",
    "textures/stone/shell_albedo.jpg",
    "textures/stone/slate_albedo.jpg",
    "textures/stone/tiger_eye_albedo.jpg",
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

/// The variable that names the private folder, when `--private-dir` does not.
pub const PRIVATE_DIR_VAR: &str = "BINGLE3D_PRIVATE_DIR";

/// The private folder's files, by their paths in it, once read (`use_private`).
static PRIVATE: OnceLock<Vec<(String, &'static [u8])>> = OnceLock::new();

/// Files read from the private folder: each by its path in it, and its bytes.
pub type PrivateFiles = Vec<(String, Vec<u8>)>;

/// The private folder's files, by their paths in it (with `/` between the folders, as ours), and
/// what was wrong: a file in place of one of ours is left out, since a private look adds to the
/// client and never changes what every game draws; so is a hidden file and a text of notes (`.md`).
pub fn read_private(dir: &Path) -> Result<(PrivateFiles, Vec<String>), String> {
    fn walk(dir: &Path, root: &Path, out: &mut PrivateFiles, problems: &mut Vec<String>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => return problems.push(format!("{}: {e}", dir.display())),
        };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let name = relative.to_string_lossy().replace('\\', "/");
            let hidden = relative
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'));
            if hidden {
                continue;
            }
            if path.is_dir() {
                walk(&path, root, out, problems);
            } else if name.ends_with(".md") {
                continue;
            } else if FILES.iter().any(|(f, _)| *f == name) {
                problems.push(format!(
                    "{name}: the client has its own, so this one is left out"
                ));
            } else {
                match std::fs::read(&path) {
                    Ok(bytes) => out.push((name, bytes)),
                    Err(e) => problems.push(format!("{name}: {e}")),
                }
            }
        }
    }
    if !dir.is_dir() {
        return Err(format!(
            "the private folder {} is not a folder",
            dir.display()
        ));
    }
    let (mut files, mut problems) = (Vec::new(), Vec::new());
    walk(dir, dir, &mut files, &mut problems);
    Ok((files, problems))
}

/// Takes the private folder's files for the rest of the run. Only the first call counts.
pub fn use_private(files: PrivateFiles) {
    // Kept for as long as the client runs, as ours are, so they are lent out as ours are.
    let files = files
        .into_iter()
        .map(|(f, bytes)| (f, &*Box::leak(bytes.into_boxed_slice())))
        .collect();
    let _ = PRIVATE.set(files);
}

/// Every file: ours, then the private folder's.
pub fn all() -> impl Iterator<Item = (&'static str, &'static [u8])> {
    let private = PRIVATE.get().map(Vec::as_slice).unwrap_or_default();
    FILES
        .iter()
        .copied()
        .chain(private.iter().map(|(f, bytes)| (f.as_str(), *bytes)))
}

/// Whether this file is the private folder's.
pub fn is_private(file: &str) -> bool {
    PRIVATE
        .get()
        .is_some_and(|files| files.iter().any(|(f, _)| f == file))
}

/// The path the asset server loads a file by.
pub fn path(file: &str) -> String {
    format!("embedded://{file}")
}

/// Whether the client has this file.
pub fn exists(file: &str) -> bool {
    all().any(|(f, _)| f == file)
}

/// A file's contents as text, if it is one of ours and is UTF-8.
pub fn text(file: &str) -> Option<&'static str> {
    all()
        .find(|(f, _)| *f == file)
        .and_then(|(_, bytes)| std::str::from_utf8(bytes).ok())
}

/// The JSON part of one of our models: a binary glTF file is a 12-byte header, then chunks,
/// each its length, its kind and its bytes, the JSON first.
fn model_json(file: &str) -> Option<serde_json::Value> {
    let bytes = all().find(|(f, _)| *f == file)?.1;
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

/// The least and greatest corners of a model's first mesh's box, in metres.
pub fn model_bounds(file: &str) -> Option<(Vec3, Vec3)> {
    let doc = model_json(file)?;
    let corner = |v: &serde_json::Value| -> Option<Vec3> {
        Some(Vec3::new(
            v[0].as_f64()? as f32,
            v[1].as_f64()? as f32,
            v[2].as_f64()? as f32,
        ))
    };
    let mut bounds: Option<(Vec3, Vec3)> = None;
    for part in doc["meshes"][0]["primitives"].as_array()? {
        let positions = &doc["accessors"][part["attributes"]["POSITION"].as_u64()? as usize];
        let (low, high) = (corner(&positions["min"])?, corner(&positions["max"])?);
        bounds = Some(bounds.map_or((low, high), |(a, b)| (a.min(low), b.max(high))));
    }
    bounds
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

pub struct AssetsPlugin {
    /// The private folder, if this run has one: its files are added to ours.
    pub private_dir: Option<PathBuf>,
}

impl Plugin for AssetsPlugin {
    fn build(&self, app: &mut App) {
        if let Some(dir) = &self.private_dir {
            match read_private(dir) {
                Ok((files, problems)) => {
                    for problem in problems {
                        warn!("private folder: {problem}");
                    }
                    info!("private folder: {} files", files.len());
                    use_private(files);
                }
                Err(e) => error!("{e}"),
            }
        }
        let registry = app.world_mut().resource_mut::<EmbeddedAssetRegistry>();
        for (file, bytes) in all() {
            registry.insert_asset(PathBuf::new(), Path::new(file), bytes);
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
        // A Tak flat stone: a square block, 18.6 mm across and 6.6 mm thick.
        let (low, high) = model_bounds("models/tak/flat.glb").unwrap();
        let size = high - low;
        assert!(
            (size.x - 0.0186).abs() < 2e-4 && (size.z - size.x).abs() < 1e-5,
            "{size}"
        );
        assert!((size.y - 0.0066).abs() < 2e-4, "{size}");
        assert_eq!(model_bounds("models/nothing.glb"), None);
        // Every model can be stood on the table.
        for (file, _) in FILES.iter().filter(|(f, _)| f.starts_with("models/")) {
            let (low, high) = model_span(file).unwrap_or_else(|| panic!("{file}"));
            assert!(high > low, "{file}");
        }
    }

    #[test]
    fn the_private_folder_adds_to_ours_and_never_replaces_them() {
        let dir = std::env::temp_dir().join(format!("bingle-3d-private-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for folder in ["appearance", "models", ".git"] {
            std::fs::create_dir_all(dir.join(folder)).unwrap();
        }
        let write = |file: &str| std::fs::write(dir.join(file), b"x").unwrap();
        write("appearance/made-up.toml");
        write("models/made-up.glb");
        write("models/ring.glb"); // one of ours
        write(".git/config");
        write("README.md");
        let (files, problems) = read_private(&dir).unwrap();
        let names: Vec<&str> = files.iter().map(|(f, _)| f.as_str()).collect();
        assert_eq!(names, ["appearance/made-up.toml", "models/made-up.glb"]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("models/ring.glb"));
        assert!(read_private(&dir.join("not-there")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_file_is_found_by_its_path() {
        assert!(exists("models/stone.glb"));
        assert!(!exists("models/nothing.glb"));
        assert!(text("appearance/hex.toml").unwrap().contains("games"));
        assert_eq!(path("models/stone.glb"), "embedded://models/stone.glb");
    }
}
