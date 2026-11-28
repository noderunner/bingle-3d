//! Things too small to be seen, kept in the scene so that the pipelines a game is drawn with are
//! compiled before it is. Bevy compiles a pipeline on a thread the first time something is drawn
//! with it and draws nothing with it meanwhile, but the depth prepass, whose pipeline is quick,
//! has already written the thing's depth: what lies behind it fails the depth test and its place
//! shows the sky's clear color, black, for the frames the pipeline takes (a tenth of a second to
//! a third, each). A pipeline is not made again for the next thing drawn with it, so what is laid
//! later draws at once.
//!
//! What a pipeline is made for is the mesh's vertex attributes (a wood's second set of texture
//! coordinates and its tangents, say) as much as the material, so a warm thing is made of the
//! very meshes and materials the real thing is: `card_table` warms its cards, `board` its boards
//! and pieces.

use bevy::prelude::*;

/// What a thing kept in the scene for its pipelines is for.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Warm {
    Cards,
    Boards,
}

/// The size of a warm thing: under a pixel from any distance the camera goes.
pub const SIZE: f32 = 1.0e-4;

/// The whole of a warm thing, which its parts are put under.
pub fn whole(commands: &mut Commands, kind: Warm) -> Entity {
    commands
        .spawn((
            Name::new(format!("Warm {kind:?}")),
            kind,
            Transform::from_scale(Vec3::splat(SIZE)),
            Visibility::default(),
        ))
        .id()
}

/// Whether things of this kind are in the scene already.
pub fn has(warm: &Query<&Warm>, kind: Warm) -> bool {
    warm.iter().any(|w| *w == kind)
}
