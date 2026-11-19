//! Painting the direction a stroke travels into the flow map. The other channels of a stroke
//! share one coverage and paint one value each; the flow map gets its own of both from here,
//! per texel: the mean direction of the dabs that reached it, and coverage only where that
//! mean has a direction.

use std::collections::BTreeMap;

use texturecraft_core::{Channel, flow_level};
use texturecraft_texel::{TILE, TileImage, to_u16};

mod flow_texel;
mod flow_tile;

use flow_tile::FlowTile;

/// What a stroke that follows its own direction paints into the flow map
#[derive(Clone, Debug)]
pub struct FlowProducer {
    /// Length of the painted vectors (0..1): the brush's flow speed
    speed: f32,
    /// Dabs a texel may go without a deposit and still be in the same pass: one brush
    /// diameter of dabs, so a noisy alpha or a tight bend does not split a pass while a stroke
    /// coming back over itself starts a new one
    pass_window: u32,
    /// Per-texel sums, by tile index (`tiles_x` per row); none of them leave this struct
    tiles: Vec<Option<Box<FlowTile>>>,
    tiles_x: u32,
    /// The painted value per texel, straight, under the flow map channel: what a commit or a
    /// live preview reads as the stroke's carried values
    pub values: BTreeMap<Channel, TileImage>,
    /// The flow map's own stroke coverage (1 component), in place of the stroke alpha
    pub coverage: TileImage,
}

/// The levels that store `vector` (each component -1..1, x along +U, y along +V): the one
/// packing every flow map value goes through, so mirrored strokes store mirrored levels
fn packed(vector: [f32; 2]) -> [u16; 3] {
    [flow_level(vector[0]), flow_level(vector[1]), 0]
}

impl FlowProducer {
    /// A producer for a texture of `width` × `height` texels painting vectors of length `speed`
    /// with a brush whose dabs are `spacing` diameters apart.
    pub fn new(width: u32, height: u32, speed: f32, spacing: f32) -> FlowProducer {
        let coverage = TileImage::new(width, height, 1);
        let values = TileImage::new(width, height, Channel::Flow.components());
        let tile_count = (coverage.tiles_x() as usize).saturating_mul(coverage.tiles_y() as usize);

        // Hostile spacings (zero, negative, not a number) count as the densest brush
        let dabs_per_diameter = if spacing > 0.0 { (1.0 / spacing).ceil() } else { f32::MAX };

        FlowProducer {
            speed: if speed.is_finite() { speed.clamp(0.0, 1.0) } else { 0.0 },
            pass_window: (dabs_per_diameter as u32).max(1),
            tiles: vec![None; tile_count],
            tiles_x: coverage.tiles_x(),
            values: [(Channel::Flow, values)].into_iter().collect(),
            coverage,
        }
    }

    /// Deposit dab number `dab` of the stroke: its texels `(x, y, alpha)` and their unit
    /// directions in texels (parallel to `texels`; `[0, 0]` where the dab has none). The copies
    /// of one dab under symmetry share its number, so where they overlap they average, as a
    /// field mirrored about a plane must. Returns whether anything was deposited.
    pub fn deposit(&mut self, texels: &[(u32, u32, f32)], directions: &[[f32; 2]], dab: u32) -> bool {
        let Some(values) = self.values.get_mut(&Channel::Flow) else { return false };
        let (width, height, components) = (self.coverage.width(), self.coverage.height(), values.comps());
        let takes = |&(x, y, alpha): &(u32, u32, f32), direction: &[f32; 2]| {
            alpha > 0.0 && x < width && y < height && *direction != [0.0; 2] && direction[0].is_finite() && direction[1].is_finite()
        };

        // A dab's texels come triangle by triangle, so long runs share a tile: look each tile up once
        let tile_of = |texel: &(u32, u32, f32)| (texel.0 / TILE, texel.1 / TILE);
        let (mut deposited, mut next) = (false, 0);
        for run in texels.chunk_by(|a, b| tile_of(a) == tile_of(b)) {
            let run_directions = directions.get(next..).unwrap_or_default();
            next += run.len();
            let Some((tile_x, tile_y)) = run.first().map(tile_of) else { continue };
            if !run.iter().zip(run_directions).any(|(texel, direction)| takes(texel, direction)) {
                continue;
            }

            let Some(slot) = self.tiles.get_mut(tile_y as usize * self.tiles_x as usize + tile_x as usize) else { continue };
            let sums = slot.get_or_insert_with(|| Box::new(FlowTile::new()));
            let (Some(value_tile), Some(coverage_tile)) = (values.tile_mut(tile_x, tile_y, &[]), self.coverage.tile_mut(tile_x, tile_y, &[])) else { continue };
            for (texel, direction) in run.iter().zip(run_directions) {
                if !takes(texel, direction) {
                    continue;
                }

                let &(x, y, alpha) = texel;
                let index = ((y % TILE) * TILE + x % TILE) as usize;
                let (vector, coverage) = sums.deposit(index, alpha.min(1.0), *direction, dab, self.pass_window);
                if let Some(stored) = value_tile.get_mut(index * components..(index + 1) * components) {
                    stored.copy_from_slice(&packed([vector[0] * self.speed, vector[1] * self.speed]));
                }
                if let Some(stored) = coverage_tile.get_mut(index) {
                    *stored = to_u16(coverage);
                }
                deposited = true;
            }
        }
        deposited
    }

    /// Whether any texel has flow map coverage: a click, or a stroke whose every direction
    /// cancelled, has none.
    pub fn has_coverage(&self) -> bool {
        self.coverage.allocated().filter_map(|(x, y)| self.coverage.tile(x, y)).any(|tile| tile.iter().any(|coverage| *coverage > 0))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use texturecraft_core::{FLOW_STILL_LEVEL, flow_unpack_level};
    use texturecraft_texel::from_u16;

    use super::*;

    const SIZE: u32 = 128;

    /// 16-bit storage keeps a vector component to about this
    const STORED: f32 = 2.0 / 65535.0;

    /// The stored vector and coverage of a texel
    fn read(producer: &FlowProducer, x: u32, y: u32) -> ([f32; 2], f32) {
        let value = producer.values[&Channel::Flow].get(x, y);
        ([flow_unpack_level(value[0]), flow_unpack_level(value[1])], from_u16(producer.coverage.get(x, y)[0]))
    }

    #[test]
    fn opposite_directions_are_stored_the_same_number_of_levels_from_still() {
        // Half speed is the tie of the code: 0.5 × 32767 is 16383.5
        for speed in [1.0, 0.5, 0.3333, 0.001] {
            let mut producer = FlowProducer::new(SIZE, SIZE, speed, 0.1);
            producer.deposit(&[(1, 1, 1.0)], &[[1.0, 0.0]], 0);
            producer.deposit(&[(2, 1, 1.0)], &[[-1.0, 0.0]], 0);
            producer.deposit(&[(3, 1, 1.0)], &[[0.0, 1.0]], 0);
            producer.deposit(&[(4, 1, 1.0)], &[[0.0, -1.0]], 0);

            let levels = |x: u32| producer.values[&Channel::Flow].get(x, 1).to_vec();
            let from_still = |level: u16| i32::from(level) - i32::from(FLOW_STILL_LEVEL);
            let (right, left, down, up) = (levels(1), levels(2), levels(3), levels(4));
            assert_eq!(from_still(right[0]), -from_still(left[0]), "speed {speed}");
            assert_eq!(from_still(down[1]), -from_still(up[1]), "speed {speed}");
            assert_eq!(from_still(right[0]), from_still(down[1]), "speed {speed}");
            assert!(from_still(right[0]) > 0, "speed {speed}");

            // The axis a stroke does not move along is exactly still, and blue is empty
            assert_eq!([right[1], left[1], down[0], up[0]], [FLOW_STILL_LEVEL; 4], "speed {speed}");
            assert_eq!([right[2], left[2], down[2], up[2]], [0; 4]);
        }
    }

    fn close(actual: [f32; 2], expected: [f32; 2]) -> bool {
        near(actual[0], expected[0]) && near(actual[1], expected[1])
    }

    fn near(actual: f32, expected: f32) -> bool {
        (actual - expected).abs() <= STORED
    }

    #[test]
    fn three_overlapping_dabs_average_by_alpha_and_cover_like_paint() {
        let mut producer = FlowProducer::new(SIZE, SIZE, 1.0, 0.1);
        let dabs = [(0.5, [1.0, 0.0]), (0.25, [0.0, 1.0]), (0.5, [0.6, 0.8])];
        for (dab, (alpha, direction)) in dabs.into_iter().enumerate() {
            assert!(producer.deposit(&[(70, 9, alpha)], &[direction], dab as u32));
        }

        // Mean of the directions weighted by alpha, brought back to full length
        let sum = [0.5 + 0.5 * 0.6, 0.25 + 0.5 * 0.8];
        let length = f32::hypot(sum[0], sum[1]);
        let (vector, coverage) = read(&producer, 70, 9);
        assert!(close(vector, [sum[0] / length, sum[1] / length]), "{vector:?}");

        // Coverage as three dabs of any paint: each over the ones before
        let over = 1.0 - (1.0 - 0.5) * (1.0 - 0.25) * (1.0 - 0.5);
        assert!(near(coverage, over), "{coverage}");

        // The texel beside it was never reached
        assert_eq!(read(&producer, 71, 9).1, 0.0);
        assert!(producer.has_coverage());
    }

    #[test]
    fn a_right_angle_crossing_in_one_pass_is_the_diagonal_at_full_speed() {
        let mut producer = FlowProducer::new(SIZE, SIZE, 0.5, 0.1);
        producer.deposit(&[(3, 3, 1.0)], &[[1.0, 0.0]], 0);
        producer.deposit(&[(3, 3, 1.0)], &[[0.0, 1.0]], 1);

        // Full speed is the brush's speed: half here
        let half_diagonal = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
        let (vector, coverage) = read(&producer, 3, 3);
        assert!(close(vector, [half_diagonal, half_diagonal]), "{vector:?}");
        assert!(near(coverage, 1.0), "{coverage}");
    }

    #[test]
    fn a_later_pass_replaces_an_earlier_one_by_its_own_alpha() {
        // Ten dabs to a diameter
        let mut producer = FlowProducer::new(SIZE, SIZE, 1.0, 0.1);
        producer.deposit(&[(8, 8, 1.0), (9, 8, 1.0)], &[[1.0, 0.0]; 2], 0);

        // Ten dabs later is still the same pass: the two directions average
        producer.deposit(&[(8, 8, 1.0)], &[[0.0, 1.0]], 10);
        let half = std::f32::consts::FRAC_1_SQRT_2;
        assert!(close(read(&producer, 8, 8).0, [half, half]));

        // One more and the stroke has come back: at full alpha the new direction is all there is
        producer.deposit(&[(9, 8, 1.0)], &[[0.0, 1.0]], 11);
        let (vector, coverage) = read(&producer, 9, 8);
        assert!(close(vector, [0.0, 1.0]), "{vector:?}");
        assert!(near(coverage, 1.0), "{coverage}");

        // A soft second pass blends over the first like a second stroke over a first
        let mut soft = FlowProducer::new(SIZE, SIZE, 1.0, 0.1);
        soft.deposit(&[(8, 8, 1.0)], &[[1.0, 0.0]], 0);
        soft.deposit(&[(8, 8, 0.25)], &[[0.0, 1.0]], 50);
        let (vector, coverage) = read(&soft, 8, 8);
        assert!(close(vector, [0.75, 0.25]), "{vector:?}");
        assert!(near(coverage, 1.0), "{coverage}");
    }

    #[test]
    fn a_pass_that_turns_back_comes_in_over_a_window() {
        // Ten dabs to a diameter: a texel left more than ten dabs ago starts a new pass
        let returning = |dab: u32| {
            let mut producer = FlowProducer::new(SIZE, SIZE, 1.0, 0.1);
            producer.deposit(&[(8, 8, 1.0)], &[[1.0, 0.0]], 0);
            producer.deposit(&[(8, 8, 1.0)], &[[-1.0, 0.0]], dab);
            read(&producer, 8, 8)
        };

        // Inside the window the two ways cancel, and the first dab past it covers no more than that
        assert_eq!(returning(10).1, 0.0);
        assert_eq!(returning(11).1, 0.0, "no edge where the new pass starts");

        // From there the way back comes in, never flowing the way out, to all there is ten dabs on.
        // A retraced stroke meets every other of these gaps, a dab of distance apart
        let coverages: Vec<f32> = (11..=22).map(|dab| returning(dab).1).collect();
        assert!(coverages.windows(2).all(|pair| pair[1] >= pair[0]), "{coverages:?}");
        assert!(coverages.windows(3).all(|triple| triple[2] - triple[0] < 0.4), "no step is an edge: {coverages:?}");
        for dab in 12..21 {
            let (vector, coverage) = returning(dab);
            assert!(vector[0] < 0.0 && vector[1].abs() <= STORED && coverage > 0.0 && coverage < 1.0, "dab {dab}: {vector:?} {coverage}");
        }
        for dab in [21, 22, 400] {
            let (vector, coverage) = returning(dab);
            assert!(close(vector, [-1.0, 0.0]) && near(coverage, 1.0), "dab {dab}: {vector:?} {coverage}");
        }
    }

    #[test]
    fn a_pass_that_does_not_turn_back_covers_at_once() {
        // Across the earlier pass, and up to 133 degrees from it, a mean of the two is a direction
        for degrees in [0.0f32, 45.0, 90.0, 130.0] {
            let (sin, cos) = degrees.to_radians().sin_cos();
            let mut producer = FlowProducer::new(SIZE, SIZE, 1.0, 0.1);
            producer.deposit(&[(8, 8, 1.0)], &[[1.0, 0.0]], 0);
            producer.deposit(&[(8, 8, 1.0)], &[[cos, sin]], 11);

            let (vector, coverage) = read(&producer, 8, 8);
            assert!(close(vector, [cos, sin]) && near(coverage, 1.0), "{degrees}: {vector:?} {coverage}");
        }

        // A soft dab that turns back blends over the pass before it by what it shows of itself
        let mut soft = FlowProducer::new(SIZE, SIZE, 1.0, 0.1);
        soft.deposit(&[(8, 8, 1.0)], &[[1.0, 0.0]], 0);
        soft.deposit(&[(8, 8, 0.25)], &[[-1.0, 0.0]], 400);
        let (vector, coverage) = read(&soft, 8, 8);
        assert!(close(vector, [0.5, 0.0]) && near(coverage, 1.0), "{vector:?} {coverage}");
    }

    #[test]
    fn directions_that_cancel_leave_no_coverage() {
        let mut producer = FlowProducer::new(SIZE, SIZE, 1.0, 0.1);
        producer.deposit(&[(5, 5, 0.75), (6, 5, 0.75)], &[[1.0, 0.0]; 2], 0);
        assert!(near(read(&producer, 5, 5).1, 0.75));

        // The stroke turns round on the spot
        assert!(producer.deposit(&[(5, 5, 0.75)], &[[-1.0, 0.0]], 1));
        assert_eq!(read(&producer, 5, 5).1, 0.0, "a mean without a direction paints nothing");
        assert!(near(read(&producer, 6, 5).1, 0.75), "the texel the turn did not reach keeps its flow");

        // Going on the new way gives the texel a direction again
        for dab in 2..5 {
            producer.deposit(&[(5, 5, 0.75)], &[[-1.0, 0.0]], dab);
        }
        let (vector, coverage) = read(&producer, 5, 5);
        assert!(close(vector, [-1.0, 0.0]) && coverage > 0.9, "{vector:?} {coverage}");
    }

    #[test]
    fn a_stroke_whose_every_direction_cancelled_has_no_coverage() {
        let mut producer = FlowProducer::new(SIZE, SIZE, 1.0, 0.1);
        assert!(!producer.has_coverage());

        producer.deposit(&[(5, 5, 0.5)], &[[0.0, 1.0]], 0);
        producer.deposit(&[(5, 5, 0.5)], &[[0.0, -1.0]], 1);

        assert!(!producer.has_coverage());
    }

    #[test]
    fn texels_without_a_direction_or_outside_the_texture_take_nothing() {
        let mut producer = FlowProducer::new(SIZE, SIZE, 1.0, 0.1);
        let texels = [(1, 1, 1.0), (2, 1, 0.0), (3, 1, -1.0), (4, 1, f32::NAN), (SIZE, 1, 1.0), (1, SIZE + 900, 1.0), (5, 1, 1.0)];
        let directions = [[0.0, 0.0], [1.0, 0.0], [1.0, 0.0], [1.0, 0.0], [1.0, 0.0], [1.0, 0.0], [f32::NAN, 0.0]];

        assert!(!producer.deposit(&texels, &directions, 0));
        assert!(!producer.has_coverage());

        // Fewer directions than texels: the texels without one are skipped
        assert!(producer.deposit(&[(1, 1, 1.0), (2, 1, 1.0)], &[[1.0, 0.0]], 1));
        assert!(near(read(&producer, 1, 1).1, 1.0));
        assert_eq!(read(&producer, 2, 1).1, 0.0);
    }

    #[test]
    fn coverage_fades_in_as_the_mean_gains_a_direction() {
        // Two equal dabs `apart` degrees from each other average to cos(apart / 2)
        let coverage_of = |apart: f32| {
            let mut producer = FlowProducer::new(SIZE, SIZE, 1.0, 0.1);
            let (sin, cos) = apart.to_radians().sin_cos();
            producer.deposit(&[(4, 4, 1.0)], &[[1.0, 0.0]], 0);
            producer.deposit(&[(4, 4, 1.0)], &[[cos, sin]], 1);
            read(&producer, 4, 4)
        };

        // Up to 133 degrees apart the mean is a direction: full coverage, full length along the bisector
        for apart in [0.0f32, 90.0, 130.0] {
            let (vector, coverage) = coverage_of(apart);
            let (sin, cos) = (apart / 2.0).to_radians().sin_cos();
            assert!(near(coverage, 1.0) && close(vector, [cos, sin]), "{apart}: {vector:?} {coverage}");
        }

        // Then it fades, and from 157 degrees on there is none
        let fading = coverage_of(145.0).1;
        assert!(fading > 0.2 && fading < 0.8, "{fading}");
        assert_eq!(coverage_of(160.0).1, 0.0);
        assert_eq!(coverage_of(180.0).1, 0.0);
    }

    /// `cargo test --release -p texturecraft-paint --lib -- --ignored benchmark_deposit --nocapture`
    #[test]
    #[ignore = "release performance measurement"]
    fn benchmark_deposit() {
        // A dab 256 texels across, as a 12 % brush leaves on a 4K sphere, moved 26 texels per dab
        let block: Vec<(u32, u32, f32)> = (0..256).flat_map(|y| (0..256).map(move |x| (x, y, 0.5))).collect();
        let directions = vec![[0.6, 0.8]; block.len()];
        let mut times = Vec::new();
        for _ in 0..7 {
            let mut producer = FlowProducer::new(4096, 4096, 1.0, 0.1);
            let start = std::time::Instant::now();
            for dab in 0..60u32 {
                let moved: Vec<(u32, u32, f32)> = block.iter().map(|(x, y, alpha)| (x + dab * 26, *y, *alpha)).collect();
                producer.deposit(&moved, &directions, dab);
            }
            times.push(start.elapsed().as_secs_f64() * 1e9 / (60.0 * block.len() as f64));
        }
        times.sort_by(f64::total_cmp);
        eprintln!("deposit: median {:.1} ns per texel of a dab, range {:.1}..{:.1}", times[3], times[0], times[6]);
    }

    #[test]
    fn hostile_speed_and_spacing_are_made_safe() {
        for (speed, spacing) in [(f32::NAN, f32::NAN), (-3.0, 0.0), (f32::INFINITY, -1.0), (9.0, f32::INFINITY)] {
            let mut producer = FlowProducer::new(SIZE, SIZE, speed, spacing);
            producer.deposit(&[(1, 1, 1.0)], &[[1.0, 0.0]], 0);
            let (vector, _) = read(&producer, 1, 1);
            assert!(vector[0].is_finite() && (-STORED..=1.0 + STORED).contains(&vector[0]), "{speed} {spacing}: {vector:?}");
        }
    }
}
