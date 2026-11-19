/// Longest alpha-weighted mean of a pass's unit directions that has no direction. Two equal
/// dabs 157 degrees apart average to this. A stroke retraced the other way never cancels to
/// zero: the dab at the turn has no partner, so with the default spacing the turn is left with
/// a mean of one dab in nine (0.11), and with dabs a quarter of a diameter apart one in five
const NO_DIRECTION_MEAN: f32 = 0.2;

/// Shortest mean that is a direction for certain (two equal dabs 133 degrees apart). Between
/// the two the pass's coverage fades in, so the place where a stroke turns back on itself has a
/// soft edge instead of speckles
const SURE_DIRECTION_MEAN: f32 = 0.4;

/// Dabs, as a share of the pass window, over which a pass that turns back on the one before it
/// comes to its full coverage. A texel the stroke left less than a window ago still belongs to
/// the pass on its way out: the two ways cancel there and nothing is painted. A texel left a
/// dab longer ago starts a new pass, which at once would cover it fully: a hard edge across the
/// stroke one brush diameter before the turn. Coming in over a whole window lets the flow of the
/// way back fall off towards the turn instead: along a retraced stroke the gap grows by two
/// dabs per dab of distance, so that is half a diameter, in as many steps as it holds dabs
const RETURN_FADE_WINDOWS: f32 = 1.0;

/// How sure a mean of unit directions of length `mean` (0..1) is a direction: 0 up to
/// [`NO_DIRECTION_MEAN`], 1 from [`SURE_DIRECTION_MEAN`], smooth between
fn direction_certainty(mean: f32) -> f32 {
    let certainty = ((mean - NO_DIRECTION_MEAN) / (SURE_DIRECTION_MEAN - NO_DIRECTION_MEAN)).clamp(0.0, 1.0);
    certainty * certainty * (3.0 - 2.0 * certainty)
}

/// What the dabs of one pass of a stroke have deposited on one texel
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct FlowTexel {
    /// Sum of alpha × unit direction over the pass's dabs
    sum: [f32; 2],
    /// Sum of alpha over the same dabs: zero where the pass has not deposited
    weight: f32,
    /// The dabs' alpha accumulated "over"
    coverage: f32,
    /// The dab that deposited last
    last_dab: u32,
    /// Share of its coverage the pass holds back (0: none): a pass that turned back on the one
    /// before it and came too soon after it to cover the texel fully
    held_back: f32,
}

impl FlowTexel {
    /// Whether the pass has deposited here and its last deposit is more than `pass_window`
    /// dabs before `dab`: the stroke has come back, and what follows is a new pass.
    pub(super) fn is_stale(&self, dab: u32, pass_window: u32) -> bool {
        self.weight > 0.0 && dab.wrapping_sub(self.last_dab) > pass_window
    }

    /// A pass that has not deposited yet and will show all but `held_back` of its coverage
    pub(super) fn holding_back(held_back: f32) -> FlowTexel {
        FlowTexel { held_back: if held_back.is_finite() { held_back.clamp(0.0, 1.0) } else { 0.0 }, ..FlowTexel::default() }
    }

    /// How much of its coverage the pass that starts on this texel with dab `dab`, heading the
    /// unit `direction`, holds back, when this pass is the one before it (see [`Self::is_stale`]).
    /// A dab still inside the window would have averaged with this pass, and two opposite ways
    /// average to no direction and no coverage. The first dab past the window starts as sure as
    /// that average of the two ways would have been, so the coverage does not jump there, and
    /// later ones start surer, up to full coverage [`RETURN_FADE_WINDOWS`] windows on. A pass
    /// that does not turn back holds nothing back.
    pub(super) fn holds_back_next(&self, direction: [f32; 2], dab: u32, pass_window: u32) -> f32 {
        let Some((earlier, _)) = self.pass() else { return 0.0 };

        // Two unit directions average to a vector of length sqrt((1 + their dot product) / 2)
        let agreement = earlier[0] * direction[0] + earlier[1] * direction[1];
        let mean_of_both = ((1.0 + agreement) * 0.5).max(0.0).sqrt();
        let dabs_past_the_window = dab.wrapping_sub(self.last_dab).saturating_sub(pass_window).saturating_sub(1);
        let fade_dabs = (pass_window as f32 * RETURN_FADE_WINDOWS).max(1.0);
        let arrived = (dabs_past_the_window as f32 / fade_dabs).clamp(0.0, 1.0);
        let arrived = arrived * arrived * (3.0 - 2.0 * arrived);

        let held_back = (1.0 - direction_certainty(mean_of_both)) * (1.0 - arrived);
        if held_back.is_finite() { held_back } else { 0.0 }
    }

    /// Add dab number `dab`: its `alpha` and unit `direction`
    pub(super) fn add(&mut self, alpha: f32, direction: [f32; 2], dab: u32) {
        self.sum = [self.sum[0] + alpha * direction[0], self.sum[1] + alpha * direction[1]];
        self.weight += alpha;
        self.coverage = alpha + self.coverage * (1.0 - alpha);
        self.last_dab = dab;
    }

    /// The pass's unit direction and the coverage it has there: the mean of its dabs at full
    /// length, covering less as the mean gets too short to be a direction. None where the dabs
    /// cancelled, or where there were none.
    pub(super) fn pass(&self) -> Option<([f32; 2], f32)> {
        // Lengths are compared against the weight instead of dividing by it: this runs for
        // every texel of every dab, and most texels have a plain direction
        let length_squared = self.sum[0] * self.sum[0] + self.sum[1] * self.sum[1];
        let none = NO_DIRECTION_MEAN * self.weight;
        if !(self.weight > 0.0 && length_squared.is_finite() && length_squared > none * none) {
            return None;
        }

        let length = length_squared.sqrt();
        let sure = SURE_DIRECTION_MEAN * self.weight;
        let coverage = if length >= sure { self.coverage } else { self.coverage * direction_certainty(length / self.weight) };
        let to_unit = 1.0 / length;
        Some(([self.sum[0] * to_unit, self.sum[1] * to_unit], coverage * (1.0 - self.held_back)))
    }
}
