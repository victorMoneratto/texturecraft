//! How a flow map component (-1..1) is packed into the 0..1 value the tools, fills and the
//! compositor carry, and which 16-bit level that value is stored as.
//!
//! 16-bit storage has an even number of levels, so none sits at one half. Still water is level
//! 32768 and a component reaches 32767 levels each way from it: opposite flows at one speed are
//! then stored the same number of levels from still, and every reader that turns a stored
//! component into something else (a file, a view colour, an angle) goes through this module so
//! that still water stays exactly still.

/// The stored level of a component at rest
pub const FLOW_STILL_LEVEL: u16 = 32768;

/// Stored levels a component reaches each way from still water
pub const FLOW_REACH_LEVELS: u16 = 32767;

/// The highest stored level: a packed value is its level over this
const TOP_LEVEL: f32 = u16::MAX as f32;

/// The packed value of a component at rest: the one stored as [`FLOW_STILL_LEVEL`]
pub const FLOW_STILL_PACKED: f32 = FLOW_STILL_LEVEL as f32 / TOP_LEVEL;

/// The level a vector component (-1..1) is stored as: `32768 + round(component × 32767)`,
/// rounded away from zero at a tie so that `component` and `-component` land the same number
/// of levels from still water. A number that is not finite is still.
pub fn flow_level(component: f32) -> u16 {
    if !component.is_finite() {
        return FLOW_STILL_LEVEL;
    }

    let offset = (component.clamp(-1.0, 1.0) * f32::from(FLOW_REACH_LEVELS)).round() as i32;
    (i32::from(FLOW_STILL_LEVEL) + offset).clamp(0, i32::from(u16::MAX)) as u16
}

/// A vector component (-1..1) as the packed value tools and fills carry, quantised to the level
/// it is stored as ([`flow_level`]).
pub fn flow_pack(component: f32) -> f32 {
    f32::from(flow_level(component)) / TOP_LEVEL
}

/// The level 16-bit storage keeps for a packed value: 0..1 to the nearest of 65536 levels, as
/// the tile storage rounds it. A number that is not finite is still.
pub fn flow_stored_level(packed: f32) -> u16 {
    if !packed.is_finite() {
        return FLOW_STILL_LEVEL;
    }

    (packed.clamp(0.0, 1.0) * TOP_LEVEL + 0.5) as u16
}

/// How many levels a stored component lies from still water, -32767..=32767. The lowest level
/// lies one step past a full reverse flow and counts as that flow.
pub fn flow_level_offset(level: u16) -> i32 {
    (i32::from(level) - i32::from(FLOW_STILL_LEVEL)).max(-i32::from(FLOW_REACH_LEVELS))
}

/// The vector component (-1..1) of a stored level: `(level − 32768) / 32767`.
pub fn flow_unpack_level(level: u16) -> f32 {
    flow_level_offset(level) as f32 / f32::from(FLOW_REACH_LEVELS)
}

/// The vector component (-1..1) of a packed value, read at the level it is stored as, so the
/// packed still value is exactly 0.
pub fn flow_unpack(packed: f32) -> f32 {
    flow_unpack_level(flow_stored_level(packed))
}

/// A stored level in a code that reaches `reach` levels each way from its own still level (127
/// for an 8-bit file): the signed number of levels from still, to the nearest, a tie rounded
/// away from zero so opposite flows stay mirrored. Negated when `reverse`.
pub fn flow_offset_in(level: u16, reach: u32, reverse: bool) -> i32 {
    let offset = i64::from(flow_level_offset(level));
    flow_mean_offset_in(if reverse { -offset } else { offset }, 1, reach)
}

/// [`flow_offset_in`] of the mean of `count` stored components, given the sum of their
/// [`flow_level_offset`]s: several texels shown as one. The mean of still texels is still, and
/// the means of mirrored flows are mirrored.
pub fn flow_mean_offset_in(offset_sum: i64, count: u32, reach: u32) -> i32 {
    let divisor = 2 * i64::from(FLOW_REACH_LEVELS) * i64::from(count.max(1));
    let levels = offset_sum.saturating_abs().saturating_mul(2).saturating_mul(i64::from(reach)).saturating_add(divisor / 2) / divisor;
    let levels = levels.min(i64::from(reach)) as i32;
    if offset_sum < 0 { -levels } else { levels }
}

/// A packed flow map value flowing the other way at the same speed. Still water stays still;
/// the third component is not part of the vector and stays empty.
pub fn flow_reversed(packed: [f32; 3]) -> [f32; 3] {
    [flow_pack(-flow_unpack(packed[0])), flow_pack(-flow_unpack(packed[1])), 0.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn still_water_is_level_32768_and_unpacks_to_exactly_zero() {
        assert_eq!(flow_level(0.0), FLOW_STILL_LEVEL);
        assert_eq!(flow_level(-0.0), FLOW_STILL_LEVEL);
        assert_eq!(flow_pack(0.0), FLOW_STILL_PACKED);
        assert_eq!(flow_stored_level(FLOW_STILL_PACKED), FLOW_STILL_LEVEL);
        assert_eq!(flow_unpack(FLOW_STILL_PACKED), 0.0);
        assert_eq!(flow_unpack_level(FLOW_STILL_LEVEL), 0.0);

        // A plain half is stored on the same level, so it reads as still too
        assert_eq!(flow_stored_level(0.5), FLOW_STILL_LEVEL);
        assert_eq!(flow_unpack(0.5), 0.0);
    }

    #[test]
    fn opposite_components_are_stored_the_same_number_of_levels_from_still() {
        // Every multiple of 1/1000, which holds the ties (0.5 × 32767 is 16383.5)
        for step in 0..=1000 {
            let component = step as f32 / 1000.0;
            let (forward, reverse) = (i32::from(flow_level(component)), i32::from(flow_level(-component)));
            assert_eq!(forward - i32::from(FLOW_STILL_LEVEL), i32::from(FLOW_STILL_LEVEL) - reverse, "{component}");
            assert_eq!(flow_unpack(flow_pack(component)), -flow_unpack(flow_pack(-component)), "{component}");
        }
        assert_eq!((flow_level(1.0), flow_level(-1.0)), (u16::MAX, 1));
        assert_eq!((flow_level(0.5), flow_level(-0.5)), (49152, 16384));
    }

    #[test]
    fn every_level_survives_packing_and_storage() {
        for level in 1..=u16::MAX {
            let component = flow_unpack_level(level);
            assert_eq!(flow_level(component), level);
            assert_eq!(flow_stored_level(flow_pack(component)), level);
            assert_eq!(flow_stored_level(f32::from(level) / TOP_LEVEL), level);
        }

        // One step past full reverse reads as full reverse
        assert_eq!(flow_unpack_level(0), -1.0);
        assert_eq!((flow_unpack_level(1), flow_unpack_level(u16::MAX)), (-1.0, 1.0));
    }

    #[test]
    fn hostile_numbers_are_still_and_out_of_range_ones_are_clamped() {
        for hostile in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(flow_level(hostile), FLOW_STILL_LEVEL);
            assert_eq!(flow_pack(hostile), FLOW_STILL_PACKED);
            assert_eq!(flow_unpack(hostile), 0.0);
        }
        assert_eq!((flow_level(7.0), flow_level(-7.0)), (u16::MAX, 1));
        assert_eq!((flow_unpack(7.0), flow_unpack(-7.0)), (1.0, -1.0));
    }

    #[test]
    fn a_coarser_code_keeps_still_exact_and_opposite_flows_mirrored() {
        const BYTE_REACH: u32 = 127;

        assert_eq!(flow_offset_in(FLOW_STILL_LEVEL, BYTE_REACH, false), 0);
        assert_eq!(flow_offset_in(FLOW_STILL_LEVEL, BYTE_REACH, true), 0);
        assert_eq!(
            (flow_offset_in(u16::MAX, BYTE_REACH, false), flow_offset_in(1, BYTE_REACH, false), flow_offset_in(0, BYTE_REACH, false)),
            (127, -127, -127)
        );
        for step in 0..=FLOW_REACH_LEVELS {
            let (above, below) = (FLOW_STILL_LEVEL + step, FLOW_STILL_LEVEL - step);
            assert_eq!(flow_offset_in(above, BYTE_REACH, false), -flow_offset_in(below, BYTE_REACH, false), "{step}");
            assert_eq!(flow_offset_in(above, BYTE_REACH, true), flow_offset_in(below, BYTE_REACH, false), "{step}");

            // The stored code itself is kept level for level
            assert_eq!(flow_offset_in(above, u32::from(FLOW_REACH_LEVELS), false), i32::from(step));
        }
    }

    #[test]
    fn a_mean_of_stored_components_keeps_still_exact_and_mirrors() {
        const BYTE_REACH: u32 = 127;

        // One texel is the texel itself
        for level in [0u16, 1, 100, 16384, 32767, 32768, 32769, 49152, u16::MAX] {
            assert_eq!(flow_mean_offset_in(i64::from(flow_level_offset(level)), 1, BYTE_REACH), flow_offset_in(level, BYTE_REACH, false));
        }

        // Still texels, and a flow with its opposite, average to still; a full flow among three still ones is a quarter
        assert_eq!(flow_mean_offset_in(0, 4, BYTE_REACH), 0);
        assert_eq!(flow_mean_offset_in(i64::from(flow_level_offset(60000)) + i64::from(flow_level_offset(5536)), 2, BYTE_REACH), 0);
        assert_eq!(flow_mean_offset_in(32767, 4, BYTE_REACH), 32);
        assert_eq!(flow_mean_offset_in(-32767, 4, BYTE_REACH), -32);
        for sum in [1i64, 77, 12345, 65534, 131068] {
            assert_eq!(flow_mean_offset_in(sum, 4, BYTE_REACH), -flow_mean_offset_in(-sum, 4, BYTE_REACH), "{sum}");
        }

        // Hostile sums and counts stay within the reach
        assert_eq!(flow_mean_offset_in(i64::MAX, 0, BYTE_REACH), 127);
        assert_eq!(flow_mean_offset_in(i64::MIN, 1, BYTE_REACH), -127);
    }

    #[test]
    fn reversing_a_flow_mirrors_it_and_keeps_still_water() {
        assert_eq!(flow_reversed([FLOW_STILL_PACKED, FLOW_STILL_PACKED, 0.0]), [FLOW_STILL_PACKED, FLOW_STILL_PACKED, 0.0]);
        assert_eq!(flow_reversed([0.5, 0.5, 0.9]), [FLOW_STILL_PACKED, FLOW_STILL_PACKED, 0.0]);
        assert_eq!(flow_reversed([flow_pack(0.25), flow_pack(-1.0), 0.0]), [flow_pack(-0.25), flow_pack(1.0), 0.0]);

        let twice = flow_reversed(flow_reversed([flow_pack(0.3), flow_pack(-0.7), 0.0]));
        assert_eq!(twice, [flow_pack(0.3), flow_pack(-0.7), 0.0]);
    }
}
