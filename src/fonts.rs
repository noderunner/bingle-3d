//! The interface's typeface: Fira Sans (SIL Open Font License, `assets/fonts/OFL.txt`).
//!
//! Bevy's built-in font is ASCII only, and the server's words are not ("Hex (11×11)", designers'
//! names). The two faces are compiled into the binary, so the client finds them wherever it is run
//! from.

use bevy::prelude::*;

pub struct FontsPlugin;

impl Plugin for FontsPlugin {
    fn build(&self, app: &mut App) {
        let mut fonts = app.world_mut().resource_mut::<Assets<Font>>();
        let regular = fonts.add(Font::from_bytes(
            include_bytes!("../assets/fonts/FiraSans-Regular.ttf").to_vec(),
        ));
        let semibold = fonts.add(Font::from_bytes(
            include_bytes!("../assets/fonts/FiraSans-SemiBold.ttf").to_vec(),
        ));
        app.insert_resource(UiFonts { regular, semibold });
    }
}

#[derive(Resource, Clone)]
pub struct UiFonts {
    regular: Handle<Font>,
    semibold: Handle<Font>,
}

impl UiFonts {
    pub fn regular(&self, px: f32) -> TextFont {
        TextFont {
            font: self.regular.clone().into(),
            font_size: FontSize::Px(px),
            ..default()
        }
    }

    pub fn semibold(&self, px: f32) -> TextFont {
        TextFont {
            font: self.semibold.clone().into(),
            font_size: FontSize::Px(px),
            ..default()
        }
    }
}
