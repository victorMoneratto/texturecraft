//! The composited flow map as the aux texture shows it in the raw channel view: the whole
//! image, or only the texels that show one composite tile, so a stroke uploads what it changed.
//!
//! The aux texture is square and at most 2048 texels wide ([`crate::aux_size`]) and stretches
//! over the set: aux texel `a` of `side` shows set texel `a × extent / side` along each axis.

use rayon::prelude::*;
use texturecraft_compose::{Composite, FLOW_VIEW_STILL, flow_view_texel};
use texturecraft_core::Channel;
use texturecraft_texel::{TILE, TileImage};

use crate::aux_region::AuxRegion;

/// Bytes of an aux texel
const TEXEL_BYTES: usize = 4;

/// The set texel that aux texel `aux` shows, along an axis of `extent` set texels
fn set_texel(aux: u32, side: u32, extent: u32) -> u32 {
    (u64::from(aux) * u64::from(extent) / u64::from(side.max(1))) as u32
}

/// The first aux texel that shows set texel `texel` or one after it, along an axis of `extent` set texels
fn first_aux_texel(texel: u32, side: u32, extent: u32) -> u32 {
    (u64::from(texel) * u64::from(side)).div_ceil(u64::from(extent.max(1))) as u32
}

/// One row of aux texels from `left` on: `row` is the row's RGBA8, `aux_y` its place in the texture
fn fill_row(composite: &Composite, side: u32, left: u32, aux_y: u32, row: &mut [u8]) {
    let flow = composite.channels.get(&Channel::Flow);
    let set_y = set_texel(aux_y, side, composite.height);

    for (aux_x, pixel) in (left..).zip(row.as_chunks_mut::<TEXEL_BYTES>().0) {
        let set_x = set_texel(aux_x, side, composite.resolution);
        *pixel = flow.map_or(FLOW_VIEW_STILL, |image| shown_texel(image, set_x, set_y));
    }
}

/// The view's colour of flow map texel (`x`, `y`); a tile nothing was composited into is still water
fn shown_texel(flow: &TileImage, x: u32, y: u32) -> [u8; 4] {
    let Some(tile) = flow.tile(x / TILE, y / TILE) else { return FLOW_VIEW_STILL };
    let first = ((y % TILE) * TILE + x % TILE) as usize * flow.comps();
    flow_view_texel(tile.get(first..).unwrap_or_default())
}

/// RGBA8 of a set's composited flow map for its aux texture, `side × side` stretched over the
/// set: the colours an 8-bit export writes. Still water where the set has no flow map.
pub fn flow_view_rgba(composite: &Composite, side: u32) -> Vec<u8> {
    let side = side.max(1);
    let row_bytes = side as usize * TEXEL_BYTES;
    let mut out = vec![0u8; row_bytes * side as usize];
    out.par_chunks_mut(row_bytes).zip(0..side).for_each(|(row, aux_y)| fill_row(composite, side, 0, aux_y, row));
    out
}

/// The aux texels that show composite tile (`tile_x`, `tile_y`), as [`flow_view_rgba`] colours
/// them. None for a tile outside the set, or one no aux texel shows.
pub fn flow_view_tile(composite: &Composite, side: u32, tile_x: u32, tile_y: u32) -> Option<AuxRegion> {
    let side = side.max(1);
    let span = |tile: u32, extent: u32| {
        let first_texel = tile.checked_mul(TILE).filter(|texel| *texel < extent)?;
        let end_texel = first_texel.saturating_add(TILE).min(extent);
        let (first, end) = (first_aux_texel(first_texel, side, extent), first_aux_texel(end_texel, side, extent).min(side));
        (first < end).then_some((first, end - first))
    };
    let (x, width) = span(tile_x, composite.resolution)?;
    let (y, height) = span(tile_y, composite.height)?;

    let row_bytes = width as usize * TEXEL_BYTES;
    let mut rgba = vec![0u8; row_bytes * height as usize];
    for (row, aux_y) in rgba.chunks_mut(row_bytes).zip(y..) {
        fill_row(composite, side, x, aux_y, row);
    }
    Some(AuxRegion { side, x, y, width, height, rgba })
}

#[cfg(test)]
mod tests {
    use super::*;
    use texturecraft_texel::to_u16;

    /// A composite with a flow map whose texel (x, y) stores red level `x` and green level `y`
    /// in the tiles of `painted`; the other tiles were never composited
    fn composite(width: u32, height: u32, painted: &[(u32, u32)]) -> Composite {
        let mut composite = Composite::with_size(&[Channel::Flow], width, height);
        let flow = composite.channels.get_mut(&Channel::Flow).unwrap();
        for &(tile_x, tile_y) in painted {
            for (x, y) in (0..TILE).flat_map(|y| (0..TILE).map(move |x| (tile_x * TILE + x, tile_y * TILE + y))) {
                flow.set(x, y, &[to_u16(x as f32 / width as f32), to_u16(y as f32 / height as f32), 0]);
            }
        }
        composite
    }

    fn texel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let start = ((y * width + x) as usize) * TEXEL_BYTES;
        [rgba[start], rgba[start + 1], rgba[start + 2], rgba[start + 3]]
    }

    /// Paste `region` into a `side × side` image
    fn paste(image: &mut [u8], side: u32, region: &AuxRegion) {
        for row in 0..region.height {
            let from = (row * region.width) as usize * TEXEL_BYTES;
            let to = ((region.y + row) * side + region.x) as usize * TEXEL_BYTES;
            let bytes = region.width as usize * TEXEL_BYTES;
            image[to..to + bytes].copy_from_slice(&region.rgba[from..from + bytes]);
        }
    }

    #[test]
    fn a_set_without_the_channel_and_unpainted_tiles_are_still_water() {
        let without = Composite::new(&[Channel::BaseColor], 128);
        assert!(flow_view_rgba(&without, 128).as_chunks::<4>().0.iter().all(|pixel| *pixel == FLOW_VIEW_STILL));
        assert!(flow_view_tile(&without, 128, 1, 1).unwrap().rgba.as_chunks::<4>().0.iter().all(|pixel| *pixel == FLOW_VIEW_STILL));

        // A tile nothing was composited into reads zero levels, which would be a full reverse flow
        let partly = composite(128, 128, &[(0, 0)]);
        let rgba = flow_view_rgba(&partly, 128);
        assert_eq!(texel(&rgba, 128, 100, 100), FLOW_VIEW_STILL);
        assert_ne!(texel(&rgba, 128, 10, 10), FLOW_VIEW_STILL, "the painted tile shows its flow");
    }

    #[test]
    fn a_texel_shows_the_exported_colour_of_the_set_texel_under_it() {
        let whole = composite(128, 128, &[(0, 0), (1, 0), (0, 1), (1, 1)]);
        let rgba = flow_view_rgba(&whole, 128);

        // Texel (x, y) flows x/128 of the way from full left to full right, and y/128 from up to down
        assert_eq!(texel(&rgba, 128, 64, 64), [128, 128, 0, 255], "the middle is stored as still water");
        assert_eq!(texel(&rgba, 128, 0, 127), [1, 253, 0, 255]);
        assert_eq!(texel(&rgba, 128, 96, 32), [191, 64, 0, 255], "half speed right and up");
    }

    #[test]
    fn the_tiles_regions_together_are_the_whole_image() {
        // Square, wide and tall (2×1 atlases), twice and one and a half times the aux texture, and sizes no tile divides
        for (width, height, side) in
            [(256u32, 256u32, 256u32), (512, 256, 512), (256, 512, 512), (512, 512, 256), (192, 192, 192), (200, 100, 200), (384, 128, 256)]
        {
            let tiles: Vec<(u32, u32)> = (0..height.div_ceil(TILE)).flat_map(|y| (0..width.div_ceil(TILE)).map(move |x| (x, y))).collect();
            let painted: Vec<(u32, u32)> = tiles.iter().copied().filter(|(x, y)| (x + y) % 3 != 0).collect();
            let composite = composite(width, height, &painted);

            // Start from a colour no flow shows, so a texel no region reaches is caught
            let mut assembled = [9u8, 9, 9, 9].repeat((side * side) as usize);
            for &(tile_x, tile_y) in &tiles {
                let region = flow_view_tile(&composite, side, tile_x, tile_y).unwrap();
                assert_eq!(region.rgba.len(), (region.width * region.height) as usize * TEXEL_BYTES);
                assert!(region.x + region.width <= side && region.y + region.height <= side);
                paste(&mut assembled, side, &region);
            }

            assert!(assembled == flow_view_rgba(&composite, side), "{width}×{height} at {side}");
        }
    }

    #[test]
    fn a_tile_region_is_only_the_texels_that_show_the_tile() {
        // One to one: the tile's own 64×64 texels
        let region = flow_view_tile(&composite(2048, 2048, &[]), 2048, 5, 9).unwrap();
        assert_eq!((region.x, region.y, region.width, region.height), (320, 576, 64, 64));
        assert_eq!(region.rgba.len(), 16384);

        // A 4096 set shows at half size: a quarter of the texels
        let region = flow_view_tile(&composite(4096, 4096, &[]), 2048, 5, 9).unwrap();
        assert_eq!((region.x, region.y, region.width, region.height), (160, 288, 32, 32));

        // A 2×1 atlas keeps its width and stretches down
        let region = flow_view_tile(&composite(512, 256, &[]), 512, 7, 3).unwrap();
        assert_eq!((region.x, region.y, region.width, region.height), (448, 384, 64, 128));
    }

    #[test]
    fn tiles_outside_the_set_have_no_region() {
        let composite = composite(256, 128, &[]);
        assert!(flow_view_tile(&composite, 256, 4, 0).is_none());
        assert!(flow_view_tile(&composite, 256, 0, 2).is_none());
        assert!(flow_view_tile(&composite, 256, u32::MAX, u32::MAX).is_none(), "hostile coordinates do not overflow");
        assert!(flow_view_tile(&composite, 256, 3, 1).is_some());
    }
}
