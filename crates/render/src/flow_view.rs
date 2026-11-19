//! The composited flow map as the aux texture shows it in the raw channel view: the whole
//! image, or only the texels that show one composite tile, so a stroke uploads what it changed.
//!
//! The aux texture is square and at most 2048 texels wide ([`crate::aux_size`]) and stretches
//! over the set. Along each axis aux texel `a` of `side` covers the set texels from
//! `a × extent / side` up to `(a + 1) × extent / side`: one texel where the aux texture is as
//! fine as the set, several where the set is larger. Several are averaged as vectors about
//! still water, so a stroke one texel wide still shows and still water stays exactly still.

use rayon::prelude::*;
use texturecraft_compose::{Composite, FLOW_VIEW_STILL, flow_view_mean_byte, flow_view_texel};
use texturecraft_core::{Channel, flow_level_offset};
use texturecraft_texel::{TILE, TileImage};

use crate::aux_region::AuxRegion;
use crate::aux_size;

/// Bytes of an aux texel
const TEXEL_BYTES: usize = 4;

/// Set texels read along an axis for one aux texel at most. An aux texel of a set up to eight
/// times the aux texture's width averages every texel it covers; beyond that (an atlas of many
/// UV tiles) it averages this many, spread evenly, so the image's cost stays bounded
const MAX_FOOTPRINT_SAMPLES: u32 = 8;

/// The set texels aux texel `aux` covers along an axis of `extent` set texels: `first..end`,
/// never empty
fn footprint(aux: u32, side: u32, extent: u32) -> (u32, u32) {
    let (aux, side, extent) = (u64::from(aux), u64::from(side.max(1)), u64::from(extent.max(1)));
    let first = (aux * extent / side).min(extent - 1);
    let end = ((aux + 1) * extent).div_ceil(side).clamp(first + 1, extent);
    (first as u32, end as u32)
}

/// The aux texels whose footprint touches set texels `first..end` along an axis of `extent` set
/// texels: `start..stop`, which is empty when no aux texel does
fn aux_span(first: u32, end: u32, side: u32, extent: u32) -> (u32, u32) {
    let side = side.max(1);
    let scaled = |texel: u32| u64::from(texel) * u64::from(side) / u64::from(extent.max(1));

    // Footprints run in order, so each end is a step or two from where the plain scaling lands
    let mut start = scaled(first).min(u64::from(side - 1)) as u32;
    while start > 0 && footprint(start - 1, side, extent).1 > first {
        start -= 1;
    }
    while start < side && footprint(start, side, extent).1 <= first {
        start += 1;
    }

    let mut stop = scaled(end).min(u64::from(side)) as u32;
    while stop > start && footprint(stop - 1, side, extent).0 >= end {
        stop -= 1;
    }
    while stop < side && footprint(stop, side, extent).0 < end {
        stop += 1;
    }
    (start, stop.max(start))
}

/// The set texels read for a footprint `first..end`: all of them, or [`MAX_FOOTPRINT_SAMPLES`]
/// spread evenly over a longer one
fn samples(first: u32, end: u32) -> impl Iterator<Item = u32> + Clone {
    let length = end.saturating_sub(first).max(1);
    let count = length.min(MAX_FOOTPRINT_SAMPLES);
    (0..count).map(move |index| first + (u64::from(index) * u64::from(length) / u64::from(count)) as u32)
}

/// One row of aux texels from `left` on: `row` is the row's RGBA8, `aux_y` its place in the texture
fn fill_row(composite: &Composite, side: u32, left: u32, aux_y: u32, row: &mut [u8]) {
    let flow = composite.channels.get(&Channel::Flow);
    let (first_y, end_y) = footprint(aux_y, side, composite.height);

    for (aux_x, pixel) in (left..).zip(row.as_chunks_mut::<TEXEL_BYTES>().0) {
        let (first_x, end_x) = footprint(aux_x, side, composite.resolution);
        *pixel = flow.map_or(FLOW_VIEW_STILL, |image| shown_texels(image, samples(first_x, end_x), samples(first_y, end_y)));
    }
}

/// The view's colour of the flow map texels at `columns` × `rows`, shown as one aux texel
fn shown_texels(flow: &TileImage, columns: impl Iterator<Item = u32> + Clone, rows: impl Iterator<Item = u32>) -> [u8; 4] {
    let (mut sums, mut count, mut single) = ([0i64; 2], 0u32, FLOW_VIEW_STILL);
    for y in rows {
        for x in columns.clone() {
            count += 1;

            // A tile nothing was composited into is still water: it adds nothing to the sums
            let Some(tile) = flow.tile(x / TILE, y / TILE) else {
                single = FLOW_VIEW_STILL;
                continue;
            };
            let first = ((y % TILE) * TILE + x % TILE) as usize * flow.comps();
            let stored = tile.get(first..).unwrap_or_default();
            for (sum, level) in sums.iter_mut().zip(stored) {
                *sum += i64::from(flow_level_offset(*level));
            }
            single = flow_view_texel(stored);
        }
    }

    // One texel is shown as the export writes it; more as their mean, blue empty
    if count <= 1 {
        return single;
    }
    [flow_view_mean_byte(sums[0], count), flow_view_mean_byte(sums[1], count), FLOW_VIEW_STILL[2], FLOW_VIEW_STILL[3]]
}

/// RGBA8 of a set's composited flow map for its aux texture: [`crate::aux_size`] of the
/// composite's size a side, stretched over the set, in the colours an 8-bit export writes.
/// Still water where the set has no flow map.
pub fn flow_view_rgba(composite: &Composite) -> Vec<u8> {
    rgba_at(composite, aux_side(composite))
}

/// The aux texels that show composite tile (`tile_x`, `tile_y`), as [`flow_view_rgba`] colours
/// them: every aux texel that covers a texel of the tile. None for a tile outside the set.
pub fn flow_view_tile(composite: &Composite, tile_x: u32, tile_y: u32) -> Option<AuxRegion> {
    tile_at(composite, aux_side(composite), tile_x, tile_y)
}

/// The side of the aux texture of `composite`'s set: bounded, whatever size the composite claims
fn aux_side(composite: &Composite) -> u32 {
    aux_size((composite.resolution, composite.height))
}

/// [`flow_view_rgba`] at a given side (the callers above bound it)
fn rgba_at(composite: &Composite, side: u32) -> Vec<u8> {
    let side = side.max(1);
    let row_bytes = side as usize * TEXEL_BYTES;
    let mut out = vec![0u8; row_bytes * side as usize];
    out.par_chunks_mut(row_bytes).zip(0..side).for_each(|(row, aux_y)| fill_row(composite, side, 0, aux_y, row));
    out
}

/// [`flow_view_tile`] at a given side (the callers above bound it)
fn tile_at(composite: &Composite, side: u32, tile_x: u32, tile_y: u32) -> Option<AuxRegion> {
    let side = side.max(1);
    let span = |tile: u32, extent: u32| {
        let first_texel = tile.checked_mul(TILE).filter(|texel| *texel < extent)?;
        let end_texel = first_texel.saturating_add(TILE).min(extent);
        let (start, stop) = aux_span(first_texel, end_texel, side, extent);
        (start < stop).then_some((start, stop - start))
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
        assert!(rgba_at(&without, 128).as_chunks::<4>().0.iter().all(|pixel| *pixel == FLOW_VIEW_STILL));
        assert!(tile_at(&without, 128, 1, 1).unwrap().rgba.as_chunks::<4>().0.iter().all(|pixel| *pixel == FLOW_VIEW_STILL));

        // A tile nothing was composited into reads zero levels, which would be a full reverse flow
        let partly = composite(128, 128, &[(0, 0)]);
        let rgba = rgba_at(&partly, 128);
        assert_eq!(texel(&rgba, 128, 100, 100), FLOW_VIEW_STILL);
        assert_ne!(texel(&rgba, 128, 10, 10), FLOW_VIEW_STILL, "the painted tile shows its flow");
    }

    #[test]
    fn a_texel_shows_the_exported_colour_of_the_set_texel_under_it() {
        let whole = composite(128, 128, &[(0, 0), (1, 0), (0, 1), (1, 1)]);
        let rgba = rgba_at(&whole, 128);

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
                .into_iter()
                // One and a half, three and four times the aux texture, and past what a texel averages in full
                .chain([(768, 768, 512), (384, 384, 128), (1024, 1024, 256), (1024, 1024, 48), (300, 700, 128)])
        {
            let tiles: Vec<(u32, u32)> = (0..height.div_ceil(TILE)).flat_map(|y| (0..width.div_ceil(TILE)).map(move |x| (x, y))).collect();
            let painted: Vec<(u32, u32)> = tiles.iter().copied().filter(|(x, y)| (x + y) % 3 != 0).collect();
            let composite = composite(width, height, &painted);

            // Start from a colour no flow shows, so a texel no region reaches is caught
            let mut assembled = [9u8, 9, 9, 9].repeat((side * side) as usize);
            for &(tile_x, tile_y) in &tiles {
                let region = tile_at(&composite, side, tile_x, tile_y).unwrap();
                assert_eq!(region.rgba.len(), (region.width * region.height) as usize * TEXEL_BYTES);
                assert!(region.x + region.width <= side && region.y + region.height <= side);
                paste(&mut assembled, side, &region);
            }

            assert!(assembled == rgba_at(&composite, side), "{width}×{height} at {side}");
        }
    }

    #[test]
    fn a_tile_region_is_only_the_texels_that_show_the_tile() {
        // One to one: the tile's own 64×64 texels
        let region = tile_at(&composite(2048, 2048, &[]), 2048, 5, 9).unwrap();
        assert_eq!((region.x, region.y, region.width, region.height), (320, 576, 64, 64));
        assert_eq!(region.rgba.len(), 16384);

        // A 4096 set shows at half size: a quarter of the texels
        let region = tile_at(&composite(4096, 4096, &[]), 2048, 5, 9).unwrap();
        assert_eq!((region.x, region.y, region.width, region.height), (160, 288, 32, 32));

        // A 2×1 atlas keeps its width and stretches down
        let region = tile_at(&composite(512, 256, &[]), 512, 7, 3).unwrap();
        assert_eq!((region.x, region.y, region.width, region.height), (448, 384, 64, 128));
    }

    #[test]
    fn tiles_outside_the_set_have_no_region() {
        let composite = composite(256, 128, &[]);
        assert!(tile_at(&composite, 256, 4, 0).is_none());
        assert!(tile_at(&composite, 256, 0, 2).is_none());
        assert!(tile_at(&composite, 256, u32::MAX, u32::MAX).is_none(), "hostile coordinates do not overflow");
        assert!(tile_at(&composite, 256, 3, 1).is_some());
    }

    #[test]
    fn a_one_texel_stroke_on_a_set_wider_than_the_aux_texture_still_shows() {
        let mut composite = Composite::with_size(&[Channel::Flow], 4096, 4096);
        let flow = composite.channels.get_mut(&Channel::Flow).unwrap();
        for y in 0..TILE {
            for x in 0..2 * TILE {
                flow.set(x, y, &[if x == 101 { u16::MAX } else { 32768 }, 32768, 0]);
            }
        }

        let rgba = flow_view_rgba(&composite);

        // A full flow in one of the two columns the aux texel covers: half the reach
        assert_eq!(rgba.len(), 2048 * 2048 * TEXEL_BYTES);
        assert_eq!(texel(&rgba, 2048, 50, 10), [192, 128, 0, 255], "the stroke at set texel 101 shows in aux texel 50");
        assert_eq!(texel(&rgba, 2048, 49, 10), FLOW_VIEW_STILL);
        assert_eq!(texel(&rgba, 2048, 51, 10), FLOW_VIEW_STILL);

        // The region of the stroke's tile holds the same texels
        let region = flow_view_tile(&composite, 1, 0).unwrap();
        assert_eq!((region.side, region.x, region.y, region.width, region.height), (2048, 32, 0, 32, 32));
        assert_eq!(texel(&region.rgba, region.width, 50 - region.x, 10), [192, 128, 0, 255]);
    }

    #[test]
    fn opposite_flows_average_to_still_water_and_still_water_stays_still() {
        let mut composite = Composite::with_size(&[Channel::Flow], 512, 512);
        let flow = composite.channels.get_mut(&Channel::Flow).unwrap();
        for (x, y) in (0..TILE).flat_map(|y| (0..TILE).map(move |x| (x, y))) {
            // Left half: still water. Right half: columns flowing right and left in turn
            let red = if x < TILE / 2 {
                32768
            } else if x % 2 == 0 {
                60000
            } else {
                5536
            };
            flow.set(x, y, &[red, 32768, 0]);
        }

        let rgba = rgba_at(&composite, 256);

        assert!((0..32).flat_map(|y| (0..32).map(move |x| (x, y))).all(|(x, y)| texel(&rgba, 256, x, y) == FLOW_VIEW_STILL));
    }

    #[test]
    fn the_image_side_is_the_aux_textures_whatever_the_composite_claims() {
        // Sparse composites: no tile is allocated, only the size matters
        for (width, height, side) in [(128u32, 128u32, 128usize), (256, 128, 256), (8192, 64, 2048), (64, 8192, 2048)] {
            let composite = Composite::with_size(&[Channel::Flow], width, height);
            assert_eq!(flow_view_rgba(&composite).len(), side * side * TEXEL_BYTES, "{width}×{height}");

            let region = flow_view_tile(&composite, 0, 0).unwrap();
            assert_eq!(region.side as usize, side);
            assert!(region.rgba.len() <= side * side * TEXEL_BYTES);
        }
    }

    #[test]
    fn a_dirty_tiles_region_reaches_every_aux_texel_that_covers_it() {
        for (extent, side) in [(256u32, 256u32), (512, 256), (768, 512), (384, 128), (300, 128), (128, 512), (1024, 48), (4096, 2048), (8192, 2048)] {
            for tile in 0..extent.div_ceil(TILE) {
                let (first, end) = (tile * TILE, ((tile + 1) * TILE).min(extent));
                let (start, stop) = aux_span(first, end, side, extent);
                for aux in 0..side {
                    let (covered_first, covered_end) = footprint(aux, side, extent);
                    let touches = covered_first < end && covered_end > first;
                    assert_eq!((start..stop).contains(&aux), touches, "aux texel {aux} of {side} and tile {tile} of a {extent} wide set");
                }
            }
        }

        // Every set texel is covered by some aux texel, and footprints never leave the set
        for (extent, side) in [(4096u32, 2048u32), (3000, 2048), (100, 2048), (5000, 7)] {
            let mut covered = vec![false; extent as usize];
            for aux in 0..side {
                let (first, end) = footprint(aux, side, extent);
                assert!(first < end && end <= extent);
                covered[first as usize..end as usize].fill(true);
            }
            assert!(covered.iter().all(|texel| *texel), "{extent} shown by {side}");
        }
    }
}
