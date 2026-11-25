//! Where what is sent to the graphics card waits on its way there.
//!
//! Everything the renderer uploads (a texture, a frame's uniforms, the interface's vertices)
//! is first written to a staging buffer the processor can reach. wgpu's allocator
//! (gpu-allocator) prefers memory for those that is on the card itself and mapped for the
//! processor. On a card without a resizable BAR that is a window of 256 MB into its memory
//! (an NVIDIA card lists it as a heap of its own of 246 MB), which every program on the
//! desktop shares. wgpu takes it 64 MB at a time, then 128: the textures uploaded as the
//! client starts fill the first block, the second takes the window past what the driver will
//! back properly, and from then on a write there runs at some 18 MB a second in place of
//! gigabytes, for as long as the client runs. A frame uploads a megabyte or so, so it took
//! 100 ms: 10 frames a second on a 2080 Ti, at any resolution, with the card idle. Measured
//! there, a 256 KB buffer took 28 ms to make, against 0.08 ms on a run whose first block had
//! been enough.
//!
//! The allocator falls back to ordinary memory when a block cannot be had from the card's. So
//! the client asks for blocks larger than that window can ever hold: every staging buffer is
//! then in system memory (0.03 ms for the same buffer), whatever else is on the desktop. A
//! card whose whole memory is mapped (a resizable BAR, or memory shared with the processor)
//! has room for such a block and keeps it there, where writes were never slow.
//!
//! The cost is that the blocks are large: the first upload takes 256 MB of system memory in
//! one piece, where wgpu's own choice starts at 64.
//!
//! A headset's device is made by the OpenXR plugin, which does not take this: `--frame-times`
//! shows there whether a frame's time is spent uploading.

use bevy::render::RenderPlugin;
use bevy::render::settings::{MemoryHints, WgpuSettings};

const MB: u64 = 1024 * 1024;

/// The window a card without a resizable BAR maps for the processor.
const SMALL_WINDOW: u64 = 256 * MB;

/// The size asked for a block of the card's own memory. wgpu makes a staging block half of
/// it, and keeps both to 256 MB at most: so twice the window, for a staging block that is all
/// of it and so never fits in it (some of the window is always in use).
const DEVICE_BLOCK: u64 = 2 * SMALL_WINDOW;

/// What the client asks of wgpu's allocator: blocks of one size, large enough that staging
/// buffers stay out of a small window into the card's memory.
pub fn hints() -> MemoryHints {
    MemoryHints::Manual {
        suballocated_device_memory_block_size: DEVICE_BLOCK..DEVICE_BLOCK,
    }
}

/// Bevy's renderer, with the allocator told so.
pub fn render_plugin() -> RenderPlugin {
    RenderPlugin {
        render_creation: WgpuSettings {
            memory_hints: hints(),
            ..Default::default()
        }
        .into(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The size wgpu makes a staging block of, for a hint (wgpu-hal's `AllocationSizes`):
    /// half the device block's, kept between 4 MB and 256 MB.
    fn staging_block(hints: &MemoryHints) -> Option<std::ops::Range<u64>> {
        match hints {
            MemoryHints::Manual {
                suballocated_device_memory_block_size: device,
            } => Some(
                (device.start / 2).clamp(4 * MB, 256 * MB)
                    ..(device.end / 2).clamp(4 * MB, 256 * MB),
            ),
            _ => None,
        }
    }

    #[test]
    fn a_staging_block_never_fits_a_small_window() {
        let staging = staging_block(&hints()).expect("a size of our own");
        // From the first block on: the driver refuses one the size of the whole window, and
        // the allocator then takes system memory.
        assert!(staging.start >= SMALL_WINDOW);
        assert!(staging.end >= staging.start);
    }

    #[test]
    fn the_renderer_is_given_the_hints() {
        use bevy::render::settings::RenderCreation;
        let RenderCreation::Automatic(settings) = render_plugin().render_creation else {
            panic!("the renderer makes its own device");
        };
        assert!(matches!(settings.memory_hints, MemoryHints::Manual { .. }));
    }
}
