//! Where a dab was heading along the surface when it was stamped

use texturecraft_geom::glam::{Mat3, Quat, Vec2, Vec3};
use texturecraft_geom::{uv_axes, uv_direction};

/// Facing (cosine) at or below which a triangle looks the opposite way from where the travel
/// was measured: no smallest turn leads there, every axis is as good as another
const OPPOSITE_FACING: f32 = -0.999;

/// The direction a dab moved in, as the stroke gave it: the flow map paints it
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Travel {
    /// Unit direction in world space, from the previous point of the stroke to this dab
    pub direction: Vec3,
    /// The surface normal where the direction was measured
    pub normal: Vec3,
}

impl Travel {
    /// The travel of a symmetry copy whose linear map (a mirror, a turn or both) is `map`
    pub fn mapped(&self, map: Mat3) -> Travel {
        Travel { direction: map * self.direction, normal: map * self.normal }
    }

    /// The direction on a surface facing `normal`: turned with the surface by the smallest
    /// rotation from where it was measured, so a stroke that crosses a crease keeps its angle
    /// to the crease, as on the unfolded surface. None when the surface faces the opposite way.
    pub fn on_surface(&self, normal: Vec3) -> Option<Vec3> {
        let (from, to) = (self.normal.try_normalize()?, normal.try_normalize()?);
        if from.dot(to) <= OPPOSITE_FACING {
            return None;
        }

        let turned = Quat::from_rotation_arc(from, to) * self.direction;
        turned.is_finite().then_some(turned)
    }

    /// The unit direction in texels (x along +U, y along +V as the texture is stored) on the
    /// triangle with corner `positions` and texture coordinates `uvs`, for a texture of `size`
    /// texels. `facing` tells the triangle's front from its back (the sum of its corner
    /// normals does). Texels are square on any atlas, so the direction is measured in them and
    /// not in UV units. None where the triangle gives no direction: no area, numbers that are
    /// not finite, or a travel that does not move along it.
    pub fn in_texels(&self, positions: [Vec3; 3], uvs: [Vec2; 3], facing: Vec3, size: (u32, u32)) -> Option<[f32; 2]> {
        let [position_a, position_b, position_c] = positions;
        let geometric = (position_b - position_a).cross(position_c - position_a).try_normalize()?;
        let front = if geometric.dot(facing) < 0.0 { -geometric } else { geometric };

        let texels = Vec2::new(size.0 as f32, size.1 as f32);
        let (along_u, along_v) = uv_axes(positions, uvs.map(|uv| uv * texels))?;
        let direction = uv_direction(along_u, along_v, self.on_surface(front)?)?.try_normalize()?;
        Some(direction.to_array())
    }
}

/// A unit direction in texels from a segment in UV units (the 2D view's travel), for a texture
/// of `size` texels. None for a segment without length.
pub fn uv_travel_in_texels(segment: Vec2, size: (u32, u32)) -> Option<[f32; 2]> {
    let texels = segment * Vec2::new(size.0 as f32, size.1 as f32);
    texels.try_normalize().filter(|direction| direction.is_finite()).map(|direction| direction.to_array())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const SIZE: (u32, u32) = (256, 256);

    fn close(actual: [f32; 2], expected: [f32; 2]) -> bool {
        (actual[0] - expected[0]).abs() < 1e-4 && (actual[1] - expected[1]).abs() < 1e-4
    }

    /// A triangle of a floor facing +Y whose chart has U along +X and V along +Z
    fn floor() -> ([Vec3; 3], [Vec2; 3]) {
        ([Vec3::ZERO, Vec3::X, Vec3::Z], [Vec2::ZERO, Vec2::X, Vec2::Y])
    }

    #[test]
    fn a_plain_chart_stores_the_travel_as_it_is_seen() {
        let (positions, uvs) = floor();
        let travel = Travel { direction: Vec3::new(0.6, 0.0, 0.8), normal: Vec3::Y };

        assert!(close(travel.in_texels(positions, uvs, Vec3::Y, SIZE).unwrap(), [0.6, 0.8]));
    }

    #[test]
    fn a_mirrored_chart_stores_the_mirrored_direction() {
        // The same floor with U running the other way, as on the far side of a mirrored seam
        let (positions, uvs) = floor();
        let mirrored = uvs.map(|uv| Vec2::new(1.0 - uv.x, uv.y));
        let travel = Travel { direction: Vec3::X, normal: Vec3::Y };

        assert!(close(travel.in_texels(positions, uvs, Vec3::Y, SIZE).unwrap(), [1.0, 0.0]));
        assert!(close(travel.in_texels(positions, mirrored, Vec3::Y, SIZE).unwrap(), [-1.0, 0.0]));
    }

    #[test]
    fn a_stretched_chart_stores_the_coordinates_that_move_along_the_travel() {
        // Twice the texels per surface unit along U as along V
        let (positions, uvs) = floor();
        let stretched = uvs.map(|uv| Vec2::new(uv.x, uv.y * 0.5));
        let diagonal = Travel { direction: Vec3::new(1.0, 0.0, 1.0).normalize(), normal: Vec3::Y };

        let stored = diagonal.in_texels(positions, stretched, Vec3::Y, SIZE).unwrap();
        let expected = Vec2::new(2.0, 1.0).normalize();
        assert!(close(stored, expected.to_array()), "{stored:?}");
    }

    #[test]
    fn texels_are_square_on_an_atlas_that_is_not() {
        // A square chart over the left half of a 2:1 atlas: half the U range, as many texels each way
        let (positions, uvs) = floor();
        let left_half = uvs.map(|uv| Vec2::new(uv.x * 0.5, uv.y));
        let diagonal = Travel { direction: Vec3::new(1.0, 0.0, 1.0).normalize(), normal: Vec3::Y };

        let stored = diagonal.in_texels(positions, left_half, Vec3::Y, (512, 256)).unwrap();
        let half = std::f32::consts::FRAC_1_SQRT_2;
        assert!(close(stored, [half, half]), "{stored:?}");
    }

    #[test]
    fn travel_keeps_its_angle_to_a_crease_it_crosses() {
        // A slope going down 60 degrees from the floor's edge along Z; its chart continues the floor's
        let down = Vec3::new(0.5, -(0.75f32.sqrt()), 0.0);
        let slope = [Vec3::X, Vec3::X + down, Vec3::X + Vec3::Z];
        let uvs = [Vec2::X, Vec2::new(2.0, 0.0), Vec2::ONE].map(|uv| uv * 0.5);
        let slope_normal = (slope[2] - slope[0]).cross(slope[1] - slope[0]).normalize();
        assert!(slope_normal.y > 0.0 && slope_normal.dot(Vec3::Y) < 0.51, "the slope faces up and out: {slope_normal}");

        // 45 degrees to the crease on the floor is 45 degrees to it on the slope
        let oblique = Travel { direction: Vec3::new(1.0, 0.0, 1.0).normalize(), normal: Vec3::Y };
        let stored = oblique.in_texels(slope, uvs, slope_normal, SIZE).unwrap();
        let half = std::f32::consts::FRAC_1_SQRT_2;
        assert!(close(stored, [half, half]), "{stored:?}");

        // Dropping the travel straight onto the slope would lean it towards the crease instead
        let (along_u, along_v) = uv_axes(slope, uvs.map(|uv| uv * 256.0)).unwrap();
        let dropped = uv_direction(along_u, along_v, oblique.direction).unwrap().normalize();
        assert!(dropped.y > 0.85, "{dropped}");
    }

    #[test]
    fn the_corner_normals_tell_the_front_whatever_the_winding() {
        // The floor's corners run clockwise seen from above; the other order runs the other way
        let (positions, uvs) = floor();
        let rewound = ([positions[0], positions[2], positions[1]], [uvs[0], uvs[2], uvs[1]]);
        let wound_down = (positions[1] - positions[0]).cross(positions[2] - positions[0]);
        let wound_up = (rewound.0[1] - rewound.0[0]).cross(rewound.0[2] - rewound.0[0]);
        assert!(wound_down.y < 0.0 && wound_up.y > 0.0);

        let travel = Travel { direction: Vec3::X, normal: Vec3::Y };
        assert!(close(travel.in_texels(positions, uvs, Vec3::Y, SIZE).unwrap(), [1.0, 0.0]));
        assert!(close(travel.in_texels(rewound.0, rewound.1, Vec3::Y, SIZE).unwrap(), [1.0, 0.0]));
    }

    #[test]
    fn triangles_that_give_no_direction_are_told_apart() {
        let (positions, uvs) = floor();
        let travel = Travel { direction: Vec3::X, normal: Vec3::Y };

        // No area on the surface, no area in the texture
        assert_eq!(travel.in_texels([Vec3::ZERO, Vec3::X, Vec3::X * 2.0], uvs, Vec3::Y, SIZE), None);
        assert_eq!(travel.in_texels(positions, [Vec2::ZERO, Vec2::X, Vec2::X * 0.5], Vec3::Y, SIZE), None);

        // Facing away from where the travel was measured, or travel straight off the surface
        assert_eq!(Travel { direction: Vec3::X, normal: Vec3::NEG_Y }.in_texels(positions, uvs, Vec3::Y, SIZE), None);
        assert_eq!(Travel { direction: Vec3::Y, normal: Vec3::Y }.in_texels(positions, uvs, Vec3::Y, SIZE), None);

        // Numbers that are not finite
        assert_eq!(Travel { direction: Vec3::splat(f32::NAN), normal: Vec3::Y }.in_texels(positions, uvs, Vec3::Y, SIZE), None);
        assert_eq!(travel.in_texels([Vec3::ZERO, Vec3::X, Vec3::splat(f32::INFINITY)], uvs, Vec3::Y, SIZE), None);
    }

    #[test]
    fn a_symmetry_copy_travels_the_mapped_way() {
        let travel = Travel { direction: Vec3::new(0.6, 0.0, 0.8), normal: Vec3::Y };

        let mirrored = travel.mapped(Mat3::from_diagonal(Vec3::new(-1.0, 1.0, 1.0)));
        assert_eq!(mirrored, Travel { direction: Vec3::new(-0.6, 0.0, 0.8), normal: Vec3::Y });

        let quarter = travel.mapped(Mat3::from_rotation_y(std::f32::consts::FRAC_PI_2));
        assert!((quarter.direction - Vec3::new(0.8, 0.0, -0.6)).length() < 1e-6 && quarter.normal == Vec3::Y);
    }

    #[test]
    fn a_segment_of_the_2d_view_is_measured_in_texels() {
        // The same UV step is twice as many texels across a 2:1 atlas
        let stored = uv_travel_in_texels(Vec2::new(0.1, 0.1), (512, 256)).unwrap();
        let expected = Vec2::new(2.0, 1.0).normalize();
        assert!(close(stored, expected.to_array()));

        assert_eq!(uv_travel_in_texels(Vec2::ZERO, SIZE), None);
        assert_eq!(uv_travel_in_texels(Vec2::splat(f32::NAN), SIZE), None);
    }
}
