/// Longest alpha-weighted mean of a pass's unit directions that has no direction. Two equal
/// dabs 157 degrees apart average to this. A stroke retraced the other way never cancels to
/// zero: the dab at the turn has no partner, so with the default spacing the turn is left with
/// a mean of one dab in nine (0.11), and with dabs a quarter of a diameter apart one in five
const NO_DIRECTION_MEAN: f32 = 0.2;

/// Shortest mean that is a direction for certain (two equal dabs 133 degrees apart). Between
/// the two the pass's coverage fades in, so the place where a stroke turns back on itself has a
/// soft edge instead of speckles
const SURE_DIRECTION_MEAN: f32 = 0.4;

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
}

impl FlowTexel {
    /// Whether the pass has deposited here and its last deposit is more than `pass_window`
    /// dabs before `dab`: the stroke has come back, and what follows is a new pass.
    pub(super) fn is_stale(&self, dab: u32, pass_window: u32) -> bool {
        self.weight > 0.0 && dab.wrapping_sub(self.last_dab) > pass_window
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
        let coverage = if length >= sure {
            self.coverage
        } else {
            let certainty = (length - none) / (sure - none);
            self.coverage * certainty * certainty * (3.0 - 2.0 * certainty)
        };
        let to_unit = 1.0 / length;
        Some(([self.sum[0] * to_unit, self.sum[1] * to_unit], coverage))
    }
}
