//! Environments: where the table stands. The plain one is a felt-covered table in a dim room; the
//! others are places, modelled by the asset pipeline. Which one is this client's own choice, as
//! the board's wood is: the server knows nothing of it, the game is the same in any of them, and
//! each player at a table may be sitting somewhere else.
//!
//! An environment is a file, `assets/environments/<id>.toml` (data, never code), which may give:
//! - `model`: a glTF file, every node of which is drawn where it stands, each part in the
//!   material of `materials` its own is named for;
//! - `ground`: instead of a model or with one, a flat top at the table's height in a material;
//! - `sun`: the light that casts the shadows; `sky`: the light from all round, as the two
//!   cubemaps Bevy's `EnvironmentMapLight` reads, and what is seen behind everything (a cubemap,
//!   or the plain `background`); `fog`; `exposure`;
//! - `camera`: how far back the orbit camera may go there;
//! - `seating`: the node of the model that is a cushion, laid out once for each seat of the game;
//! - `shadows.none`: materials whose parts cast no shadow; `drift`: materials whose texture
//!   moves, as water's does.
//!
//! `scene.rs` builds the one chosen. Whatever the environment, **the table top is at y = 0 with
//! its middle at the origin**, and a unit is a metre: boards and pieces never know where they are.

use std::collections::BTreeMap;
use std::f32::consts::TAU;

use bevy::prelude::*;
use serde::Deserialize;

use crate::assets;
use crate::look::MaterialSpec;

/// The environment the client starts in, by id: the plain table.
pub const DEFAULT: &str = "room";

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    /// The file's name, which `config.toml` and `--environment` know it by. Not in the file.
    #[serde(skip)]
    pub id: String,
    /// What the player reads when choosing.
    pub name: String,
    pub model: Option<String>,
    pub ground: Option<Ground>,
    pub sun: Sun,
    pub sky: Sky,
    /// What is behind everything where the sky gives no picture (sRGB).
    #[serde(default)]
    pub background: [f32; 3],
    pub fog: Fog,
    /// The camera's exposure (EV100): 9.7, Bevy's own, suits a room; daylight needs more.
    pub exposure: Option<f32>,
    #[serde(default)]
    pub camera: CameraLimits,
    pub seating: Option<Seating>,
    #[serde(default)]
    pub shadows: Shadows,
    /// How far a material's texture moves a second, in its texture coordinates.
    #[serde(default)]
    pub drift: BTreeMap<String, [f32; 2]>,
    #[serde(default)]
    pub materials: BTreeMap<String, MaterialSpec>,
}

/// A flat top at the table's height, `size` metres square.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ground {
    pub material: String,
    pub size: f32,
}

/// The light that casts the shadows.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sun {
    /// Where it shines from: x to the right, y up, z towards the player.
    pub from: [f32; 3],
    /// In lux.
    pub illuminance: f32,
    /// sRGB; white when not given. Files written before 2026-11-27 call it `colour`.
    #[serde(alias = "colour")]
    pub color: Option<[f32; 3]>,
    /// How far from the camera shadows are drawn, in metres.
    pub shadows_to: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sky {
    pub diffuse: String,
    pub specular: String,
    /// cd/m² for a value of 1 in the cubemaps.
    pub intensity: f32,
    /// Where most of the cubemaps' light comes from, when that is not where the sun is: they are
    /// turned about the vertical to bring it to the sun's side.
    pub window: Option<[f32; 3]>,
    /// A cubemap seen behind everything, and its brightness (cd/m² for a value of 1).
    pub backdrop: Option<String>,
    #[serde(default)]
    pub backdrop_brightness: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fog {
    /// sRGB. Files written before 2026-11-27 call it `colour`.
    #[serde(alias = "colour")]
    pub color: [f32; 3],
    /// Where it starts and where nothing is seen through it, in metres from the eye.
    pub start: f32,
    pub end: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraLimits {
    /// How far from what it looks at the orbit camera may go, in metres.
    #[serde(default = "CameraLimits::default_radius")]
    pub max_radius: f32,
    /// The lens's aperture: a larger number keeps more than the board sharp.
    pub f_stops: Option<f32>,
}

impl CameraLimits {
    fn default_radius() -> f32 {
        crate::camera::OrbitCamera::MAX_RADIUS
    }
}

impl Default for CameraLimits {
    fn default() -> Self {
        CameraLimits {
            max_radius: Self::default_radius(),
            f_stops: None,
        }
    }
}

/// A cushion for each seat of the game, on the floor round the table.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seating {
    /// The node of the model that is one, standing on y = 0 about the origin.
    pub node: String,
    /// How far from the table's middle each lies, and the floor's height (the table top is 0).
    pub distance: f32,
    pub floor: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shadows {
    #[serde(default)]
    pub none: Vec<String>,
}

impl Environment {
    pub fn parse(id: &str, text: &str) -> Result<Self, String> {
        let mut parsed: Environment = toml::from_str(text).map_err(|e| e.to_string())?;
        parsed.id = id.to_string();
        Ok(parsed)
    }

    /// What is wrong with it: files the client does not have, and names it does not define.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        let files = [
            self.model.as_ref(),
            Some(&self.sky.diffuse),
            Some(&self.sky.specular),
            self.sky.backdrop.as_ref(),
        ];
        for file in files.into_iter().flatten() {
            if !assets::exists(file) {
                out.push(format!("no file {file}"));
            }
        }
        for (name, material) in &self.materials {
            out.extend(
                material
                    .problems()
                    .into_iter()
                    .map(|p| format!("{name}: {p}")),
            );
        }
        if let Some(ground) = &self.ground
            && !self.materials.contains_key(&ground.material)
        {
            out.push(format!("the ground: no material {}", ground.material));
        }
        for name in self.shadows.none.iter().chain(self.drift.keys()) {
            if !self.materials.contains_key(name) {
                out.push(format!("no material {name}"));
            }
        }
        if self.model.is_none() && self.ground.is_none() {
            out.push("neither a model nor a ground".into());
        }
        if self.seating.is_some() && self.model.is_none() {
            out.push("seating, with no model to take a cushion from".into());
        }
        if Vec3::from_array(self.sun.from).length() < 1e-3 {
            out.push("the sun shines from nowhere".into());
        }
        out
    }

    /// Where the sun shines from, as a direction.
    pub fn sun_from(&self) -> Vec3 {
        Vec3::from_array(self.sun.from).normalize_or(Vec3::Y)
    }

    /// How the sky's cubemaps are turned: about the vertical, so that their `window` is on the
    /// sun's side. Bevy looks a cubemap up in the direction this gives a world direction.
    pub fn sky_rotation(&self) -> Quat {
        let Some(window) = self.sky.window else {
            return Quat::IDENTITY;
        };
        let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z).normalize_or(Vec3::X);
        Quat::from_rotation_arc(flat(self.sun_from()), flat(Vec3::from_array(window)))
    }
}

/// Where each seat's cushion lies, and which way it is turned (about the vertical): the first
/// seat on the player's side of the table (+z), the others evenly round it, so two face each
/// other and four have a side each.
pub fn seat_places(seats: usize, distance: f32) -> Vec<(Vec2, f32)> {
    (0..seats)
        .map(|i| {
            let turn = TAU * i as f32 / seats as f32;
            (Vec2::new(turn.sin(), turn.cos()) * distance, turn)
        })
        .collect()
}

/// Every environment the client has, the plain table first.
#[derive(Resource, Debug)]
pub struct Environments(pub Vec<Environment>);

impl Default for Environments {
    fn default() -> Self {
        Self::built_in()
    }
}

impl Environments {
    /// The files compiled into the client. One that cannot be read is left out, and logged.
    pub fn built_in() -> Self {
        let mut out = Vec::new();
        for (file, _) in assets::FILES {
            let Some(id) = file
                .strip_prefix("environments/")
                .and_then(|f| f.strip_suffix(".toml"))
            else {
                continue;
            };
            let parsed = assets::text(file)
                .ok_or_else(|| "not UTF-8".to_string())
                .and_then(|text| Environment::parse(id, text));
            match parsed {
                Ok(e) => {
                    for problem in e.problems() {
                        warn!("{file}: {problem}");
                    }
                    out.push(e);
                }
                Err(e) => error!("{file}: {e}"),
            }
        }
        out.sort_by_key(|e| (e.id != DEFAULT, e.name.clone()));
        Environments(out)
    }

    pub fn get(&self, id: &str) -> Option<&Environment> {
        self.0.iter().find(|e| e.id == id)
    }

    /// The one chosen, or the plain table if there is none of that name.
    pub fn chosen(&self, chosen: &Chosen) -> Option<&Environment> {
        self.get(&chosen.0).or_else(|| self.get(DEFAULT))
    }
}

/// The environment the player has chosen, by id.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct Chosen(pub String);

impl Default for Chosen {
    fn default() -> Self {
        Chosen(DEFAULT.into())
    }
}

/// The environment `config.toml` remembers, read before the first frame.
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct Kept(pub Option<String>);

pub struct EnvironmentPlugin {
    /// One for this run (`--environment`), in place of the one remembered.
    pub chosen: Option<String>,
}

impl Plugin for EnvironmentPlugin {
    fn build(&self, app: &mut App) {
        let all = Environments::built_in();
        let kept = app.world().get_resource::<Kept>().and_then(|k| k.0.clone());
        let wanted = self.chosen.clone().or(kept);
        let chosen = match wanted {
            Some(id) if all.get(&id).is_some() => Chosen(id),
            Some(id) => {
                warn!("no environment {id:?}: the table it is");
                Chosen::default()
            }
            None => Chosen::default(),
        };
        app.insert_resource(all).insert_resource(chosen);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_environment_reads_and_names_only_what_it_has() {
        let mut count = 0;
        for (file, _) in assets::FILES {
            let Some(id) = file
                .strip_prefix("environments/")
                .and_then(|f| f.strip_suffix(".toml"))
            else {
                continue;
            };
            let e = Environment::parse(id, assets::text(file).unwrap())
                .unwrap_or_else(|e| panic!("{file}: {e}"));
            assert!(e.problems().is_empty(), "{file}: {:?}", e.problems());
            assert_eq!(e.id, id);
            count += 1;
        }
        let all = Environments::built_in();
        assert_eq!(all.0.len(), count);
        assert!(count >= 2, "the table, and somewhere else");
        assert_eq!(all.0[0].id, DEFAULT, "the plain table is first");
    }

    #[test]
    fn an_unknown_choice_is_the_plain_table() {
        let all = Environments::built_in();
        assert_eq!(all.chosen(&Chosen("garden".into())).unwrap().id, "garden");
        assert_eq!(all.chosen(&Chosen("the moon".into())).unwrap().id, DEFAULT);
    }

    #[test]
    fn a_rooms_windows_are_turned_to_the_suns_side() {
        let all = Environments::built_in();
        let room = all.get(DEFAULT).unwrap();
        let r = room.sky_rotation();
        let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z).normalize();
        let sun = flat(room.sun_from());
        let window = flat(Vec3::from_array(room.sky.window.unwrap()));
        // Looking towards the sun, Bevy reads the room where its windows are.
        assert!((r * sun).distance(window) < 1e-5);
        assert!((r * Vec3::Y).distance(Vec3::Y) < 1e-5, "up stays up");
        // A sky made for its sun is not turned.
        assert_eq!(all.get("garden").unwrap().sky_rotation(), Quat::IDENTITY);
    }

    #[test]
    fn seats_go_round_the_table_from_the_players_side() {
        let two = seat_places(2, 0.8);
        assert!(
            two[0].0.distance(Vec2::new(0.0, 0.8)) < 1e-5,
            "{:?}",
            two[0]
        );
        assert!(two[1].0.distance(Vec2::new(0.0, -0.8)) < 1e-5, "facing");
        let four = seat_places(4, 1.0);
        assert!(four[1].0.distance(Vec2::new(1.0, 0.0)) < 1e-5);
        assert!(four[3].0.distance(Vec2::new(-1.0, 0.0)) < 1e-5);
        assert!(four.iter().all(|(p, _)| (p.length() - 1.0).abs() < 1e-5));
        assert!(seat_places(0, 1.0).is_empty());
    }

    #[test]
    fn mistakes_are_found() {
        let e = Environment::parse(
            "x",
            r#"
            name = "X"
            model = "environments/none.glb"
            [ground]
            material = "missing"
            size = 1.0
            [sun]
            from = [0.0, 0.0, 0.0]
            illuminance = 1.0
            shadows_to = 1.0
            [sky]
            diffuse = "environment/room_diffuse.ktx2"
            specular = "environment/none.ktx2"
            intensity = 1.0
            [fog]
            color = [0.0, 0.0, 0.0]
            start = 1.0
            end = 2.0
            [shadows]
            none = ["nothing"]
            "#,
        )
        .unwrap();
        assert_eq!(e.problems().len(), 5, "{:?}", e.problems());
        assert!(
            Environment::parse("x", "name = \"X\"\nweather = 1").is_err(),
            "unknown keys are refused, so a misspelling is not ignored"
        );
    }
    /// An environment file written before the word was spelled `color` still says `colour` for
    /// the sun's and the fog's, and is read the same; both spellings at once are refused.
    #[test]
    fn the_suns_and_fogs_color_is_read_from_the_old_spelling_too() {
        let text = |sun: &str, fog: &str| {
            format!(
                "name = \"X\"\n[sun]\nfrom = [0, 1, 0]\nilluminance = 1\nshadows_to = 1\n{sun}\n\
                 [sky]\ndiffuse = \"d\"\nspecular = \"s\"\nintensity = 1\n\
                 [fog]\n{fog}\nstart = 1\nend = 2\n"
            )
        };
        let new = Environment::parse("x", &text("color = [1, 0.5, 0.25]", "color = [0, 0.5, 1]"));
        let old = Environment::parse(
            "x",
            &text("colour = [1, 0.5, 0.25]", "colour = [0, 0.5, 1]"),
        );
        let (new, old) = (new.unwrap(), old.unwrap());
        assert_eq!(old, new);
        assert_eq!(new.sun.color, Some([1.0, 0.5, 0.25]));
        assert_eq!(new.fog.color, [0.0, 0.5, 1.0]);
        assert!(
            Environment::parse(
                "x",
                &text("color = [1, 0, 0]\ncolour = [0, 1, 0]", "color = [0, 0, 0]")
            )
            .is_err()
        );
    }
}
