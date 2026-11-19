//! Directions between a triangle's surface and its texture coordinates.

use glam::{Vec2, Vec3};

/// Smallest sine squared of the angle between two axes that still span a plane: below it (axes
/// within 0.06 degrees of each other) a solve only amplifies rounding. Relative to the axes' own
/// lengths, so a tiny triangle and a huge one are judged alike
const MIN_AXES_SINE_SQUARED: f32 = 1e-6;

/// The surface directions of one unit along U and one unit along V on the triangle with corner
/// `positions` and texture coordinates `uvs` (in any unit: UV, or texels for directions per
/// texel). None for a triangle with no area on the surface or in the texture.
pub fn uv_axes(positions: [Vec3; 3], uvs: [Vec2; 3]) -> Option<(Vec3, Vec3)> {
    let [position_a, position_b, position_c] = positions;
    let [uv_a, uv_b, uv_c] = uvs;
    let (edge_b, edge_c, to_b, to_c) = (position_b - position_a, position_c - position_a, uv_b - uv_a, uv_c - uv_a);

    // Twice the texture area against the edges that span it: a sliver has almost none
    let double_area = to_b.perp_dot(to_c);
    let spanned = to_b.length_squared() * to_c.length_squared();
    if !(double_area.is_finite() && spanned.is_finite()) || double_area * double_area <= MIN_AXES_SINE_SQUARED * spanned {
        return None;
    }

    let along_u = (edge_b * to_c.y - edge_c * to_b.y) / double_area;
    let along_v = (edge_c * to_b.x - edge_b * to_c.x) / double_area;
    (along_u.is_finite() && along_v.is_finite()).then_some((along_u, along_v))
}

/// `direction` on the surface as texture coordinates: the `(u, v)` that makes
/// `along_u * u + along_v * v` closest to it (least squares, so the part of `direction` off the
/// surface is dropped). None when the axes do not span a plane or a number is not finite.
pub fn uv_direction(along_u: Vec3, along_v: Vec3, direction: Vec3) -> Option<Vec2> {
    let (uu, uv, vv) = (along_u.dot(along_u), along_u.dot(along_v), along_v.dot(along_v));
    let (u_dot, v_dot) = (along_u.dot(direction), along_v.dot(direction));

    // The determinant is uu * vv * sin² of the angle between the axes
    let det = uu * vv - uv * uv;
    if !(det.is_finite() && det > MIN_AXES_SINE_SQUARED * uu * vv) {
        return None;
    }

    let solved = Vec2::new((u_dot * vv - v_dot * uv) / det, (v_dot * uu - u_dot * uv) / det);
    solved.is_finite().then_some(solved)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn close(actual: Vec2, expected: Vec2) -> bool {
        (actual - expected).length() < 1e-5
    }

    #[test]
    fn a_flat_square_chart_reads_surface_directions_as_they_are() {
        // U along +X, V along +Z, one unit each
        let positions = [Vec3::ZERO, Vec3::X, Vec3::Z];
        let (along_u, along_v) = uv_axes(positions, [Vec2::ZERO, Vec2::X, Vec2::Y]).unwrap();
        assert_eq!((along_u, along_v), (Vec3::X, Vec3::Z));

        assert!(close(uv_direction(along_u, along_v, Vec3::new(0.6, 0.0, 0.8)).unwrap(), Vec2::new(0.6, 0.8)));

        // The part off the surface is dropped
        assert!(close(uv_direction(along_u, along_v, Vec3::new(0.6, 5.0, 0.8)).unwrap(), Vec2::new(0.6, 0.8)));
    }

    #[test]
    fn a_stretched_or_sheared_chart_gives_the_coordinates_that_move_along_the_direction() {
        // Twice as many U units per surface unit as V units, and V leaning 45 degrees
        let positions = [Vec3::ZERO, Vec3::X, Vec3::new(1.0, 0.0, 1.0)];
        let (along_u, along_v) = uv_axes(positions, [Vec2::ZERO, Vec2::new(2.0, 0.0), Vec2::new(0.0, 1.0)]).unwrap();
        assert_eq!((along_u, along_v), (Vec3::new(0.5, 0.0, 0.0), Vec3::new(1.0, 0.0, 1.0)));

        // Moving one unit along +X takes two U units and no V
        assert!(close(uv_direction(along_u, along_v, Vec3::X).unwrap(), Vec2::new(2.0, 0.0)));

        // Moving along +Z takes one V unit, and two U units back to undo the lean
        assert!(close(uv_direction(along_u, along_v, Vec3::Z).unwrap(), Vec2::new(-2.0, 1.0)));
    }

    #[test]
    fn the_same_triangle_at_any_size_is_judged_alike() {
        for scale in [1e-6_f32, 1.0, 1e6] {
            let positions = [Vec3::ZERO, Vec3::X * scale, Vec3::Z * scale];
            let (along_u, along_v) = uv_axes(positions, [Vec2::ZERO, Vec2::X * scale, Vec2::Y * scale]).unwrap();
            assert!(close(uv_direction(along_u, along_v, Vec3::X).unwrap(), Vec2::X), "scale {scale}");
        }
    }

    #[test]
    fn a_sliver_is_refused_at_any_size() {
        // Axes a twentieth of a degree apart: the solve would divide by almost nothing
        let lean = 0.05f32.to_radians();
        for scale in [1e-6_f32, 1.0, 1e6] {
            let (along_u, along_v) = (Vec3::X * scale, Vec3::new(lean.cos(), 0.0, lean.sin()) * scale);
            assert_eq!(uv_direction(along_u, along_v, Vec3::Z), None, "scale {scale}");

            // The same in the texture: a chart squeezed to a hair
            let uvs = [Vec2::ZERO, Vec2::X * scale, Vec2::new(lean.cos(), lean.sin()) * scale];
            assert_eq!(uv_axes([Vec3::ZERO, Vec3::X, Vec3::Z], uvs), None, "scale {scale}");
        }
    }

    #[test]
    fn a_triangle_without_area_has_no_directions() {
        // No area in the texture: its corners are in a line there
        assert_eq!(uv_axes([Vec3::ZERO, Vec3::X, Vec3::Z], [Vec2::ZERO, Vec2::X, Vec2::X * 2.0]), None);
        assert_eq!(uv_axes([Vec3::ZERO, Vec3::X, Vec3::Z], [Vec2::ZERO; 3]), None);

        // No area on the surface: both axes run along the same line
        let (along_u, along_v) = uv_axes([Vec3::ZERO, Vec3::X, Vec3::X * 2.0], [Vec2::ZERO, Vec2::X, Vec2::Y]).unwrap();
        assert_eq!(uv_direction(along_u, along_v, Vec3::X), None);
        assert_eq!(uv_direction(Vec3::ZERO, Vec3::ZERO, Vec3::X), None);
    }

    #[test]
    fn numbers_that_are_not_finite_give_no_direction() {
        assert_eq!(uv_axes([Vec3::ZERO, Vec3::X, Vec3::splat(f32::NAN)], [Vec2::ZERO, Vec2::X, Vec2::Y]), None);
        assert_eq!(uv_axes([Vec3::ZERO, Vec3::X, Vec3::Z], [Vec2::ZERO, Vec2::X, Vec2::splat(f32::INFINITY)]), None);
        assert_eq!(uv_direction(Vec3::X, Vec3::Z, Vec3::splat(f32::NAN)), None);
        assert_eq!(uv_direction(Vec3::X * f32::MAX, Vec3::Z * f32::MAX, Vec3::X), None);
    }
}
