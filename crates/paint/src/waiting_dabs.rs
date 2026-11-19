//! Dabs that were painted before the stroke had a direction to give them

use std::collections::VecDeque;

use crate::stamp_source::StampSource;

/// Most dabs kept waiting: the copies of two dabs under the fullest symmetry (8 mirror images
/// of 32 radial copies each). A particle brush gives no dab a direction, and a long stroke of
/// it must not pile them up
const MAX_WAITING: usize = 512;

/// The dabs of a stroke that had no travel when they were painted: its first dab, and the first
/// after the pointer came back onto the mesh. The other channels take such a dab at once; the
/// flow map takes it when the next dab arrives and tells which way the stroke left it, so the
/// flow starts where the paint starts. A dab the stroke never leaves (a click) waits for good.
#[derive(Clone, Debug)]
pub(crate) struct WaitingDabs<Dab> {
    /// Oldest first, each with its number in the stroke and the symmetry copy it is (None: the
    /// stroke itself)
    held: VecDeque<(u32, Option<StampSource>, Dab)>,
}

impl<Dab> Default for WaitingDabs<Dab> {
    fn default() -> Self {
        WaitingDabs { held: VecDeque::new() }
    }
}

impl<Dab> WaitingDabs<Dab> {
    /// Keep `dab`, number `number` of the stroke and symmetry copy `copy`, until the next dab
    /// of that copy arrives. Past [`MAX_WAITING`] the dab that waited longest is let go.
    pub(crate) fn hold(&mut self, number: u32, copy: Option<StampSource>, dab: Dab) {
        if self.held.len() >= MAX_WAITING {
            self.held.pop_front();
        }
        self.held.push_back((number, copy, dab));
    }

    /// The waiting dab that dab `number` of symmetry copy `copy` follows directly, if there is
    /// one: the dab before it of the same copy. A stroke numbers each dab of a copy once, so no
    /// other dab can take it, in whatever order the copies of several dabs arrive.
    pub(crate) fn take_before(&mut self, number: u32, copy: Option<StampSource>) -> Option<Dab> {
        let at = self.held.iter().position(|(waiting, of_copy, _)| waiting.wrapping_add(1) == number && *of_copy == copy)?;
        self.held.remove(at).map(|(_, _, dab)| dab)
    }
}

#[cfg(test)]
mod tests {
    use texturecraft_geom::glam::{Mat3, Vec3};

    use super::*;

    fn mirrored() -> Option<StampSource> {
        Some(StampSource { origin: Vec3::ZERO, to_source: Mat3::from_diagonal(Vec3::new(-1.0, 1.0, 1.0)) })
    }

    #[test]
    fn a_dab_is_taken_by_the_next_dab_of_its_own_copy() {
        let mut waiting = WaitingDabs::default();
        waiting.hold(0, None, "first");
        waiting.hold(0, mirrored(), "first, mirrored");

        // The 2D view sends the dabs of a sample before any of their copies
        assert_eq!(waiting.take_before(1, None), Some("first"));
        assert_eq!(waiting.take_before(2, None), None);
        assert_eq!(waiting.take_before(1, mirrored()), Some("first, mirrored"));
        assert_eq!(waiting.take_before(1, mirrored()), None, "taken once");
    }

    #[test]
    fn a_dab_is_not_taken_by_itself_or_by_a_dab_further_on() {
        let mut waiting = WaitingDabs::default();
        waiting.hold(9, None, "ninth");

        assert_eq!(waiting.take_before(9, None), None);
        assert_eq!(waiting.take_before(11, None), None);
        assert_eq!(waiting.take_before(10, mirrored()), None, "another copy's dab");
        assert_eq!(waiting.take_before(10, None), Some("ninth"));

        // Dab numbers wrap: the dab after the last number is number 0
        waiting.hold(u32::MAX, mirrored(), "last");
        assert_eq!(waiting.take_before(0, mirrored()), Some("last"));
    }

    #[test]
    fn dabs_that_never_get_a_direction_do_not_pile_up() {
        let mut waiting = WaitingDabs::default();
        for particle in 0..5000u32 {
            waiting.hold(particle.wrapping_mul(17), None, particle);
        }

        assert_eq!(waiting.held.len(), MAX_WAITING);
        assert_eq!(waiting.held.front().map(|(_, _, particle)| *particle), Some(5000 - MAX_WAITING as u32), "the longest waiting went first");
    }
}
