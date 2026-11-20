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
    "appearance/hex.toml",
    "appearance/points.toml",
    "environment/room_diffuse.ktx2",
    "environment/room_specular.ktx2",
    "models/stone.glb",
    "pictures/cards/points.jpg",
    "textures/engraving/glyphs_albedo.png",
    "textures/engraving/glyphs_normal.png",
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
    fn a_file_is_found_by_its_path() {
        assert!(exists("models/stone.glb"));
        assert!(!exists("models/nothing.glb"));
        assert!(text("appearance/hex.toml").unwrap().contains("games"));
        assert_eq!(path("models/stone.glb"), "embedded://models/stone.glb");
    }
}
