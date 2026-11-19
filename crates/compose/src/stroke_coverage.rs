//! Stroke coverage that belongs to one channel instead of the whole stroke.

use texturecraft_core::Channel;
use texturecraft_texel::from_u16;

use crate::eval::StrokePreview;

impl StrokePreview {
    /// The live stroke's coverage at texel (`x`, `y`) for the channel at `channel_index` of
    /// [`Channel::ALL`], when that channel does not share the stroke alpha: a stroke that paints
    /// its own direction covers the flow map only where it has one. None for every other
    /// channel.
    pub(crate) fn channel_stroke(&self, channel_index: usize, x: u32, y: u32) -> Option<f32> {
        let coverage = self.coverage.get(Channel::ALL.get(channel_index)?)?;
        Some(from_u16(coverage.get(x, y)[0]))
    }
}
