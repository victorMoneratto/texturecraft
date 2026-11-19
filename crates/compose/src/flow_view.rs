//! The flow map as 8-bit colours for a raw view of the channel: the levels an 8-bit export
//! writes, so the view and the file agree byte for byte.
//!
//! Neither the composite's 16 bits nor a file's 8 have a level at exactly one half. A stored
//! component is read as `texturecraft_core::flow_code` stores it (still water at level 32768,
//! 32767 levels each way) and written around the 8-bit upper middle level (128 of 255),
//! reaching 127 levels each way, so still water is one exact colour.

use texturecraft_core::{flow_mean_offset_in, flow_offset_in};

/// The 8-bit level of a component at rest
pub const FLOW_VIEW_STILL_LEVEL: u8 = 128;

/// 8-bit levels a component reaches each way from still water
pub const FLOW_VIEW_REACH: u8 = 127;

/// Still water in the raw view: red and green at rest, blue empty, opaque
pub const FLOW_VIEW_STILL: [u8; 4] = [FLOW_VIEW_STILL_LEVEL, FLOW_VIEW_STILL_LEVEL, 0, 255];

/// The 8-bit level of one stored flow map component: `128 + 127 × component`, with the
/// component read as `(level − 32768) / 32767`, to the nearest level. No stored level falls
/// halfway between two; the lowest lies one step past a full reverse flow and reads as it.
pub fn flow_view_byte(level: u16) -> u8 {
    let levels = flow_offset_in(level, u32::from(FLOW_VIEW_REACH), false);
    (i32::from(FLOW_VIEW_STILL_LEVEL) + levels).clamp(0, i32::from(u8::MAX)) as u8
}

/// The 8-bit level of several stored components shown as one texel (a view smaller than the
/// map): their mean as a vector component, from the sum of their
/// [`texturecraft_core::flow_level_offset`]s and their count. One component gives
/// [`flow_view_byte`]; still texels give the still level exactly.
pub fn flow_view_mean_byte(offset_sum: i64, count: u32) -> u8 {
    let levels = flow_mean_offset_in(offset_sum, count, u32::from(FLOW_VIEW_REACH));
    (i32::from(FLOW_VIEW_STILL_LEVEL) + levels).clamp(0, i32::from(u8::MAX)) as u8
}

/// The raw view's RGBA8 of a stored flow map texel (its components in order; a missing one is
/// at rest): the vector in red and green, blue empty.
pub fn flow_view_texel(stored: &[u16]) -> [u8; 4] {
    let component = |k: usize| stored.get(k).map_or(FLOW_VIEW_STILL[0], |level| flow_view_byte(*level));
    [component(0), component(1), FLOW_VIEW_STILL[2], FLOW_VIEW_STILL[3]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use texturecraft_core::{FLOW_REACH_LEVELS, FLOW_STILL, FLOW_STILL_LEVEL, flow_from_polar};
    use texturecraft_texel::to_u16;

    /// The view's colour of a packed flow map value as the composite stores it
    fn colour(value: [f32; 3]) -> [u8; 4] {
        flow_view_texel(&value.map(to_u16))
    }

    #[test]
    fn still_water_is_one_exact_colour() {
        assert_eq!(colour(FLOW_STILL), [128, 128, 0, 255]);
        assert_eq!(colour(FLOW_STILL), FLOW_VIEW_STILL);
        assert_eq!(colour(flow_from_polar(45.0, 0.0)), FLOW_VIEW_STILL, "a direction without speed is still");
    }

    #[test]
    fn known_vectors_show_the_colours_they_export() {
        // Right is +U in red; up the image is towards smaller V, so green falls
        assert_eq!(colour(flow_from_polar(0.0, 1.0)), [255, 128, 0, 255]);
        assert_eq!(colour(flow_from_polar(180.0, 1.0)), [1, 128, 0, 255]);
        assert_eq!(colour(flow_from_polar(90.0, 0.5)), [128, 64, 0, 255]);
        assert_eq!(colour(flow_from_polar(270.0, 1.0)), [128, 255, 0, 255]);
    }

    #[test]
    fn both_directions_reach_the_same_number_of_levels() {
        assert_eq!((flow_view_byte(0), flow_view_byte(1), flow_view_byte(u16::MAX)), (1, 1, 255));
        for step in 0..=FLOW_REACH_LEVELS {
            let (forward, reverse) = (flow_view_byte(FLOW_STILL_LEVEL + step), flow_view_byte(FLOW_STILL_LEVEL - step));
            let still = i32::from(FLOW_VIEW_STILL_LEVEL);
            assert_eq!(i32::from(forward) - still, still - i32::from(reverse), "{step} levels from still");
        }
    }

    #[test]
    fn opposite_authored_flows_show_mirrored_colours() {
        for (angle, speed) in [(0.0, 0.25), (0.0, 0.5), (90.0, 0.5), (45.0, 0.75), (135.0, 1.0), (20.0, 0.6)] {
            let (forward, reverse) = (colour(flow_from_polar(angle, speed)), colour(flow_from_polar(angle + 180.0, speed)));
            for k in 0..2 {
                assert_eq!(i32::from(forward[k]) + i32::from(reverse[k]), 2 * i32::from(FLOW_VIEW_STILL_LEVEL), "{angle} at {speed}");
            }
        }
    }

    #[test]
    fn the_packed_value_is_stored_on_the_level_the_core_names() {
        // The core cannot see the tile storage: its rounding is repeated there and checked here
        assert_eq!(to_u16(FLOW_STILL[0]), FLOW_STILL_LEVEL);
        assert_eq!(texturecraft_core::flow_unpack(texturecraft_texel::from_u16(to_u16(FLOW_STILL[0]))), 0.0);
        for level in 0..=u16::MAX {
            let stored = texturecraft_texel::from_u16(level);
            assert_eq!(texturecraft_core::flow_stored_level(stored), level);
            assert_eq!(to_u16(stored), level);
        }
        for step in 0..=2000 {
            let component = step as f32 / 1000.0 - 1.0;
            assert_eq!(to_u16(texturecraft_core::flow_pack(component)), texturecraft_core::flow_level(component), "{component}");
        }
        for packed in [0.0, 0.1, 0.25, 0.49999, 0.5, 0.50001, 0.75, 0.9, 1.0, -3.0, 4.0] {
            assert_eq!(texturecraft_core::flow_stored_level(packed), to_u16(packed), "{packed}");
        }
    }

    #[test]
    fn a_mean_of_one_component_is_that_components_level() {
        for level in (0..=u16::MAX).step_by(7).chain([u16::MAX]) {
            assert_eq!(flow_view_mean_byte(i64::from(texturecraft_core::flow_level_offset(level)), 1), flow_view_byte(level), "{level}");
        }
        assert_eq!(flow_view_mean_byte(0, 16), FLOW_VIEW_STILL_LEVEL);
        assert_eq!((flow_view_mean_byte(32767 * 2, 4), flow_view_mean_byte(-32767 * 2, 4)), (192, 64), "half the texels at full speed");
    }

    #[test]
    fn faster_never_shows_a_lower_level() {
        let levels: Vec<u8> = (0..=u16::MAX).map(flow_view_byte).collect();
        assert!(levels.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    #[test]
    fn a_texel_with_missing_components_is_at_rest_there() {
        assert_eq!(flow_view_texel(&[]), FLOW_VIEW_STILL);
        assert_eq!(flow_view_texel(&[u16::MAX]), [255, 128, 0, 255]);

        // The third stored component is never shown: blue stays empty
        assert_eq!(flow_view_texel(&[32768, 32768, u16::MAX]), FLOW_VIEW_STILL);
    }
}
