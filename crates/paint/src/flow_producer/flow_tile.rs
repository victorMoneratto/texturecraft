use texturecraft_texel::TILE_AREA;

use super::flow_texel::FlowTexel;

/// What a stroke has deposited on the texels of one tile, kept as sums so that the dabs of one
/// pass average and a later pass over the same place replaces them
#[derive(Clone, Debug)]
pub(super) struct FlowTile {
    /// The pass in progress on each texel
    passes: Vec<FlowTexel>,
    /// What earlier passes left under it: a vector no longer than 1 and its coverage. Only a
    /// tile where a stroke came back over itself has it
    base: Option<Vec<[f32; 3]>>,
}

impl FlowTile {
    pub(super) fn new() -> FlowTile {
        FlowTile { passes: vec![FlowTexel::default(); TILE_AREA], base: None }
    }

    /// Add one dab's `alpha` and unit `direction` to texel `index`. A texel whose last deposit
    /// is more than `pass_window` dabs old starts a new pass, which paints over the earlier
    /// one as a new stroke would. Returns the vector (no longer than 1) and coverage the texel
    /// shows now.
    pub(super) fn deposit(&mut self, index: usize, alpha: f32, direction: [f32; 2], dab: u32, pass_window: u32) -> ([f32; 2], f32) {
        let Some(pass) = self.passes.get_mut(index) else { return ([0.0; 2], 0.0) };
        let mut under = self.base.as_ref().and_then(|base| base.get(index)).copied().unwrap_or_default();
        if pass.is_stale(dab, pass_window) {
            // Fold the finished pass into what lies under the new one. A new pass that turns
            // back on it soon after holds part of itself back, and keeps as much of the
            // finished pass from showing under it: towards the turn both ways fade out
            let held_back = pass.holds_back_next(direction, dab, pass_window);
            let (vector, coverage) = shown(pass, under, 1.0 - held_back);
            under = [vector[0], vector[1], coverage];
            if let Some(texel) = self.base.get_or_insert_with(|| vec![[0.0; 3]; TILE_AREA]).get_mut(index) {
                *texel = under;
            }
            *pass = FlowTexel::holding_back(held_back);
        }

        pass.add(alpha, direction, dab);
        shown(pass, under, 1.0)
    }
}

/// The vector and coverage a texel shows: `pass` at `strength` (0..1) of its coverage over
/// `under` (what earlier passes left, as vector x, y and coverage), "over" on the vectors
/// themselves as the compositor blends two committed strokes. A pass whose directions
/// cancelled covers nothing.
fn shown(pass: &FlowTexel, under: [f32; 3], strength: f32) -> ([f32; 2], f32) {
    let [under_x, under_y, under_coverage] = under;
    let Some((direction, pass_coverage)) = pass.pass() else { return ([under_x, under_y], under_coverage) };
    let pass_coverage = pass_coverage * strength;
    if under_coverage <= 0.0 {
        return (direction, pass_coverage);
    }

    let kept = under_coverage * (1.0 - pass_coverage);
    let coverage = pass_coverage + kept;
    if coverage <= 0.0 {
        return ([0.0; 2], 0.0);
    }

    ([(direction[0] * pass_coverage + under_x * kept) / coverage, (direction[1] * pass_coverage + under_y * kept) / coverage], coverage)
}
