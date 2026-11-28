//! The effects the flat cameras and the sun have, and which of them are on: ambient occlusion,
//! anti-aliasing, depth of field, bloom, the sun's shadows and its contact shadows. The escape
//! menu's *Graphics* page changes them (`escape_menu.rs`), `scene.rs` puts them on and takes them
//! off the cameras and the sun at once, and `config.toml` keeps them (`ambient_occlusion`,
//! `anti_aliasing`, `depth_of_field`, `bloom`, `shadows`, `contact_shadows`). The defaults are
//! what the table has always been drawn with.
//!
//! Some do not go together: SSAO and temporal anti-aliasing both need MSAA off, so choosing MSAA
//! turns ambient occlusion off (`Graphics::cycle`, and `Graphics::sanitized` for a config that
//! has both). The shadows' temporal filtering goes with temporal anti-aliasing and is not an
//! option of its own.
//!
//! A run a script limits (`--frames`, `--screenshot`) uses the defaults whatever `config.toml`
//! says, so its pictures are the same everywhere; `--no-bloom` starts without bloom. A headset
//! has its own cameras, which these never touch, and the page is of no use there (`HeadsetRun`).

use bevy::prelude::*;

/// A setting with a few named values, to step through and to keep in `config.toml`.
trait Choice: Copy + PartialEq + 'static {
    const ALL: &'static [Self];

    /// The value as `config.toml` spells it.
    fn name(self) -> &'static str;

    fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        Self::ALL
            .iter()
            .copied()
            .find(|c| c.name().eq_ignore_ascii_case(text))
    }

    /// The value after this one, the first after the last.
    fn next(self) -> Self {
        let at = Self::ALL.iter().position(|c| *c == self).unwrap_or(0);
        Self::ALL[(at + 1) % Self::ALL.len()]
    }
}

/// How much ambient occlusion darkens the creases: under a stone, in a cut line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AmbientOcclusion {
    Off,
    Low,
    #[default]
    High,
}

impl Choice for AmbientOcclusion {
    const ALL: &'static [Self] = &[Self::Off, Self::Low, Self::High];

    fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::High => "high",
        }
    }
}

/// How the picture's jagged edges are smoothed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AntiAliasing {
    Off,
    Fxaa,
    Smaa,
    /// Temporal: smooths over several frames, and the sun's shadows are filtered over time too.
    #[default]
    Taa,
    /// Four samples a pixel. Not with ambient occlusion.
    Msaa,
}

impl Choice for AntiAliasing {
    const ALL: &'static [Self] = &[Self::Off, Self::Fxaa, Self::Smaa, Self::Taa, Self::Msaa];

    fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Fxaa => "fxaa",
            Self::Smaa => "smaa",
            Self::Taa => "taa",
            Self::Msaa => "msaa",
        }
    }
}

/// How sharp the sun's shadows are: the size of the shadow map each cascade draws into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shadows {
    Off,
    Low,
    #[default]
    Medium,
    High,
}

impl Choice for Shadows {
    const ALL: &'static [Self] = &[Self::Off, Self::Low, Self::Medium, Self::High];

    fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

impl Shadows {
    /// The width of each cascade's shadow map, none when there are no shadows.
    pub fn map_size(self) -> Option<usize> {
        match self {
            Self::Off => None,
            Self::Low => Some(1024),
            Self::Medium => Some(2048),
            Self::High => Some(4096),
        }
    }
}

/// One of the page's rows that steps through values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    AmbientOcclusion,
    AntiAliasing,
    DepthOfField,
    Bloom,
    Shadows,
    ContactShadows,
}

/// Which effects are on, and how much.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Graphics {
    pub ambient_occlusion: AmbientOcclusion,
    pub anti_aliasing: AntiAliasing,
    pub depth_of_field: bool,
    pub bloom: bool,
    pub shadows: Shadows,
    pub contact_shadows: bool,
}

impl Default for Graphics {
    /// What the table is drawn with when nothing says otherwise.
    fn default() -> Self {
        Graphics {
            ambient_occlusion: AmbientOcclusion::default(),
            anti_aliasing: AntiAliasing::default(),
            depth_of_field: true,
            bloom: true,
            shadows: Shadows::default(),
            contact_shadows: true,
        }
    }
}

impl Graphics {
    /// Reads what `config.toml` keeps: a value it does not know, or none, is the default, and the
    /// words for each that was not understood come with it.
    pub fn read(
        ambient_occlusion: Option<&str>,
        anti_aliasing: Option<&str>,
        depth_of_field: Option<bool>,
        bloom: Option<bool>,
        shadows: Option<&str>,
        contact_shadows: Option<bool>,
    ) -> (Self, Vec<String>) {
        let mut problems = Vec::new();
        let ao = read_choice("ambient_occlusion", ambient_occlusion, &mut problems);
        let aa = read_choice("anti_aliasing", anti_aliasing, &mut problems);
        let sh = read_choice("shadows", shadows, &mut problems);
        let d = Graphics::default();
        let graphics = Graphics {
            ambient_occlusion: ao.unwrap_or(d.ambient_occlusion),
            anti_aliasing: aa.unwrap_or(d.anti_aliasing),
            depth_of_field: depth_of_field.unwrap_or(d.depth_of_field),
            bloom: bloom.unwrap_or(d.bloom),
            shadows: sh.unwrap_or(d.shadows),
            contact_shadows: contact_shadows.unwrap_or(d.contact_shadows),
        }
        .sanitized();
        (graphics, problems)
    }

    /// The names `config.toml` keeps: ambient occlusion, anti-aliasing and shadows.
    pub fn names(&self) -> [&'static str; 3] {
        [
            self.ambient_occlusion.name(),
            self.anti_aliasing.name(),
            self.shadows.name(),
        ]
    }

    /// These settings with what cannot go together made to: no ambient occlusion with MSAA.
    pub fn sanitized(mut self) -> Self {
        if self.anti_aliasing == AntiAliasing::Msaa {
            self.ambient_occlusion = AmbientOcclusion::Off;
        }
        self
    }

    /// Whether ambient occlusion can be chosen: not with MSAA.
    pub fn can_occlude(&self) -> bool {
        self.anti_aliasing != AntiAliasing::Msaa
    }

    /// Steps a row to its next value, and what that cannot go with to off.
    pub fn cycle(&mut self, setting: Setting) {
        match setting {
            Setting::AmbientOcclusion if self.can_occlude() => {
                self.ambient_occlusion = self.ambient_occlusion.next();
            }
            Setting::AmbientOcclusion => {}
            Setting::AntiAliasing => self.anti_aliasing = self.anti_aliasing.next(),
            Setting::DepthOfField => self.depth_of_field = !self.depth_of_field,
            Setting::Bloom => self.bloom = !self.bloom,
            Setting::Shadows => self.shadows = self.shadows.next(),
            Setting::ContactShadows => self.contact_shadows = !self.contact_shadows,
        }
        *self = self.sanitized();
    }

    /// The settings of a run: the defaults in one a script limits (`--frames`, `--screenshot`),
    /// else what is kept; and no bloom with `--no-bloom`.
    pub fn chosen(limited: bool, no_bloom: bool, kept: Graphics) -> Self {
        let mut graphics = if limited { Graphics::default() } else { kept };
        if no_bloom {
            graphics.bloom = false;
        }
        graphics.sanitized()
    }

    /// Whether temporal anti-aliasing is on, which the camera's prepasses and the shadows'
    /// filtering follow.
    pub fn temporal(&self) -> bool {
        self.anti_aliasing == AntiAliasing::Taa
    }

    /// Whether the camera has a depth prepass: SSAO and TAA read it.
    pub fn wants_depth(&self) -> bool {
        self.ambient_occlusion != AmbientOcclusion::Off || self.temporal()
    }
}

/// A value read from `config.toml`, or what was wrong with it.
fn read_choice<T: Choice>(key: &str, text: Option<&str>, problems: &mut Vec<String>) -> Option<T> {
    let text = text?;
    let value = T::parse(text);
    if value.is_none() {
        let names: Vec<_> = T::ALL.iter().map(|c| c.name()).collect();
        problems.push(format!(
            "config.toml: {key} {text:?} is not one of {}",
            names.join(", ")
        ));
    }
    value
}

/// What `config.toml` keeps, put here by the network's plugin, which reads it.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct Kept(pub Graphics);

/// In a headset's run: the cameras are the headset's, and the settings do nothing.
#[derive(Resource, Debug, Clone, Copy)]
pub struct HeadsetRun;

pub struct GraphicsPlugin {
    /// A run a script limits (`--frames`, `--screenshot`).
    pub limited: bool,
    /// `--no-bloom`.
    pub no_bloom: bool,
    /// Drawing in a headset.
    pub in_headset: bool,
}

impl Plugin for GraphicsPlugin {
    fn build(&self, app: &mut App) {
        let kept = app
            .world()
            .get_resource::<Kept>()
            .copied()
            .unwrap_or_default();
        app.insert_resource(Graphics::chosen(self.limited, self.no_bloom, kept.0));
        if self.in_headset {
            app.insert_resource(HeadsetRun);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_what_the_table_has_always_had() {
        let g = Graphics::default();
        assert_eq!(g.ambient_occlusion, AmbientOcclusion::High);
        assert_eq!(g.anti_aliasing, AntiAliasing::Taa);
        assert!(g.depth_of_field && g.bloom && g.contact_shadows);
        assert_eq!(g.shadows.map_size(), Some(2048), "Bevy's own size");
        assert_eq!(g, g.sanitized());
    }

    #[test]
    fn names_are_read_and_written_the_same() {
        for c in AmbientOcclusion::ALL {
            assert_eq!(AmbientOcclusion::parse(c.name()), Some(*c));
        }
        for c in AntiAliasing::ALL {
            assert_eq!(AntiAliasing::parse(c.name()), Some(*c));
        }
        for c in Shadows::ALL {
            assert_eq!(Shadows::parse(c.name()), Some(*c));
        }
        assert_eq!(AntiAliasing::parse(" TAA "), Some(AntiAliasing::Taa));
        assert_eq!(AntiAliasing::parse("fancy"), None);
    }

    #[test]
    fn a_value_is_stepped_to_the_next_and_round_to_the_first() {
        assert_eq!(AmbientOcclusion::High.next(), AmbientOcclusion::Off);
        assert_eq!(AntiAliasing::Off.next(), AntiAliasing::Fxaa);
        assert_eq!(Shadows::High.next(), Shadows::Off);
    }

    #[test]
    fn what_is_not_understood_is_the_default_and_said_so() {
        let (g, problems) = Graphics::read(
            Some("ultra"),
            Some("smaa"),
            Some(false),
            None,
            Some("Low"),
            Some(false),
        );
        assert_eq!(g.ambient_occlusion, AmbientOcclusion::High);
        assert_eq!(g.anti_aliasing, AntiAliasing::Smaa);
        assert!(!g.depth_of_field && g.bloom && !g.contact_shadows);
        assert_eq!(g.shadows, Shadows::Low);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("ambient_occlusion") && problems[0].contains("ultra"));
        let (none, problems) = Graphics::read(None, None, None, None, None, None);
        assert_eq!(none, Graphics::default());
        assert!(problems.is_empty());
    }

    #[test]
    fn msaa_and_ambient_occlusion_do_not_go_together() {
        let mut g = Graphics::default();
        // TAA to MSAA: ambient occlusion goes off with it.
        g.cycle(Setting::AntiAliasing);
        assert_eq!(g.anti_aliasing, AntiAliasing::Msaa);
        assert_eq!(g.ambient_occlusion, AmbientOcclusion::Off);
        assert!(!g.can_occlude());
        // And is not offered while MSAA is.
        g.cycle(Setting::AmbientOcclusion);
        assert_eq!(g.ambient_occlusion, AmbientOcclusion::Off);
        // Any other anti-aliasing offers it again, from off.
        g.cycle(Setting::AntiAliasing);
        assert_eq!(g.anti_aliasing, AntiAliasing::Off);
        g.cycle(Setting::AmbientOcclusion);
        assert_eq!(g.ambient_occlusion, AmbientOcclusion::Low);

        // A config with both is read as without ambient occlusion.
        let (read, _) = Graphics::read(Some("high"), Some("msaa"), None, None, None, None);
        assert_eq!(read.ambient_occlusion, AmbientOcclusion::Off);
    }

    #[test]
    fn the_other_rows_step_on_their_own() {
        let mut g = Graphics::default();
        g.cycle(Setting::DepthOfField);
        g.cycle(Setting::Bloom);
        g.cycle(Setting::ContactShadows);
        g.cycle(Setting::Shadows);
        g.cycle(Setting::AmbientOcclusion);
        assert!(!g.depth_of_field && !g.bloom && !g.contact_shadows);
        assert_eq!(g.shadows, Shadows::High);
        assert_eq!(g.ambient_occlusion, AmbientOcclusion::Off);
        assert_eq!(g.anti_aliasing, AntiAliasing::Taa, "untouched");
    }

    #[test]
    fn a_script_s_run_uses_the_defaults_whatever_is_kept() {
        let kept = Graphics {
            anti_aliasing: AntiAliasing::Fxaa,
            bloom: false,
            shadows: Shadows::Off,
            ..Graphics::default()
        };
        assert_eq!(Graphics::chosen(false, false, kept), kept);
        assert_eq!(Graphics::chosen(true, false, kept), Graphics::default());
        // --no-bloom is the command line's, kept or not.
        let no_bloom = Graphics::chosen(true, true, kept);
        assert!(!no_bloom.bloom);
        assert_eq!(
            Graphics {
                bloom: true,
                ..no_bloom
            },
            Graphics::default()
        );
        assert!(!Graphics::chosen(false, true, Graphics::default()).bloom);
    }

    #[test]
    fn what_the_camera_needs_follows_the_effects() {
        let mut g = Graphics::default();
        assert!(g.temporal() && g.wants_depth());
        g.anti_aliasing = AntiAliasing::Fxaa;
        assert!(!g.temporal() && g.wants_depth(), "SSAO reads it");
        g.ambient_occlusion = AmbientOcclusion::Off;
        assert!(!g.wants_depth());
    }

    #[test]
    fn the_plugin_makes_the_run_s_settings() {
        let mut app = App::new();
        app.insert_resource(Kept(Graphics {
            depth_of_field: false,
            ..Graphics::default()
        }))
        .add_plugins(GraphicsPlugin {
            limited: false,
            no_bloom: true,
            in_headset: true,
        });
        let g = *app.world().resource::<Graphics>();
        assert!(!g.depth_of_field && !g.bloom);
        assert!(app.world().get_resource::<HeadsetRun>().is_some());
    }
}
