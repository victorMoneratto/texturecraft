//! Keeping each set's aux texture in step with its composited flow map while the raw view of
//! the channel is shown. The whole image goes up once; after that only the texels of the tiles
//! that were composited again, so the view follows a stroke without uploading the image again.

use std::collections::BTreeMap;

use rayon::prelude::*;
use texturecraft_engine::Session;
use texturecraft_engine::compose::FLOW_VIEW_STILL;
use texturecraft_engine::core::Channel;
use texturecraft_render::flow_view::{flow_view_rgba, flow_view_tile};

use crate::gpu::SceneSync;

/// What a set's aux texture holds, as [`crate::gpu::GpuLink`] records it: a name and a revision
pub(crate) type AuxContent = (String, u64);

/// The name recorded for a set whose aux texture holds its flow map
const FLOW_CONTENT: &str = "flow";

/// The record of an aux texture that shows its set's flow map. Whether the set has the channel
/// is part of it: a set that gains or loses the channel needs its whole image again
fn content(has_channel: bool) -> AuxContent {
    (FLOW_CONTENT.to_owned(), u64::from(has_channel))
}

/// Queue what the flow view needs in `out`. A set whose aux texture holds something else gets
/// its whole image when it has the channel, and one still texel to stretch over it when it has
/// not: no image is built for a set with nothing to show. Afterwards only the texels showing
/// the tiles in `composited` (texture set, tiles) go up, the ones this frame sent to the
/// display textures.
pub(crate) fn sync(held: &mut BTreeMap<usize, AuxContent>, session: &Session, composited: &[(usize, Vec<(u32, u32)>)], out: &mut SceneSync) {
    for (texture_set, cache) in session.caches.iter().enumerate() {
        let has_channel = cache.composite.channels.contains_key(&Channel::Flow);
        let shown = content(has_channel);
        if held.get(&texture_set) != Some(&shown) {
            if has_channel {
                out.aux.push((texture_set, texturecraft_render::aux_size(cache.size), flow_view_rgba(&cache.composite)));
            } else {
                out.aux_fills.push((texture_set, FLOW_VIEW_STILL));
            }
            held.insert(texture_set, shown);
            continue;
        }

        // Without the channel the texture is still water everywhere and stays so
        let Some((_, tiles)) = composited.iter().find(|(set, _)| *set == texture_set && has_channel) else { continue };
        let regions: Vec<_> = tiles.par_iter().filter_map(|&(tile_x, tile_y)| flow_view_tile(&cache.composite, tile_x, tile_y)).collect();
        out.aux_regions.extend(regions.into_iter().map(|region| (texture_set, region)));
    }
}

/// The flow view is not shown this frame: the tiles composited meanwhile are not followed, so
/// the aux textures no longer count as holding the flow map and the next showing sends them whole.
pub(crate) fn leave(held: &mut BTreeMap<usize, AuxContent>) {
    held.retain(|_, (name, _)| name != FLOW_CONTENT);
}
