//! The flow map as 8-bit colours for a raw view of the channel: the levels an 8-bit export
//! writes, so the view and the file agree byte for byte.
//!
//! Neither the composite's 16 bits nor a file's 8 have a level at exactly one half. A stored
//! component is read around the level a packed 0.5 is stored as (32768 of 65535) and written
//! around the 8-bit upper middle level (128 of 255), reaching 127 levels each way, so still
//! water is one exact colour.

/// The level a packed 0.5 is stored as in the composite's 16 bits
const STORED_STILL_LEVEL: i32 = 32768;

/// Stored levels a component reaches each way from still water
const STORED_REACH: i32 = 32767;

/// The 8-bit level of a component at rest
const VIEW_STILL_LEVEL: i32 = 128;

/// 8-bit levels a component reaches each way from still water
const VIEW_REACH: i32 = 127;

/// Still water in the raw view: red and green at rest, blue empty, opaque
pub const FLOW_VIEW_STILL: [u8; 4] = [VIEW_STILL_LEVEL as u8, VIEW_STILL_LEVEL as u8, 0, 255];

/// The 8-bit level of one stored flow map component: `128 + 127 × component`, with the
/// component read as `(level − 32768) / 32767`.
pub fn flow_view_byte(level: u16) -> u8 {
    let offset = i32::from(level) - STORED_STILL_LEVEL;

    // To the nearest level; no stored level falls halfway between two. The lowest stored level
    // lies one step past a full reverse flow and rounds onto it
    let levels = (2 * offset * VIEW_REACH + STORED_REACH).div_euclid(2 * STORED_REACH);
    (VIEW_STILL_LEVEL + levels).clamp(0, 255) as u8
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
    use texturecraft_core::{FLOW_STILL, flow_from_polar};
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
        for step in 0..=STORED_REACH {
            let (forward, reverse) = (flow_view_byte((STORED_STILL_LEVEL + step) as u16), flow_view_byte((STORED_STILL_LEVEL - step) as u16));
            assert_eq!(i32::from(forward) - VIEW_STILL_LEVEL, VIEW_STILL_LEVEL - i32::from(reverse), "{step} levels from still");
        }
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
