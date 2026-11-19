//! The direction a stamp's travel takes in the texture: through mirrored and stretched charts,
//! over a crease, on a triangle without area, and under the quick mask

use std::collections::BTreeSet;

use texturecraft_core::{Channel, flow_unpack_level};
use texturecraft_geom::glam::{Vec2, Vec3};
use texturecraft_geom::{Bvh, Mesh, UvGBuffer};
use texturecraft_paint::{BrushAlpha, FlowProducer, RasterCtx, Shape, Stamp, StrokeCanvas, Travel, fill_triangles, rasterize};
use texturecraft_texel::{TileImage, from_u16, to_u16};

const SIZE: u32 = 256;

/// Stored directions are 16-bit and the charts are exact: a stored unit vector is this close
const STORED: f32 = 1e-3;

/// Triangle pairs wound to face up for corners given as (x, z) low-low, high-low, high-high, low-high
const QUAD: [u32; 6] = [0, 2, 1, 0, 3, 2];

/// A mesh of quads, each with its own corners (so each keeps its own normal) and chart
fn quads(parts: &[([Vec3; 4], [Vec2; 4])]) -> Mesh {
    let positions = parts.iter().flat_map(|(corners, _)| *corners).collect();
    let uvs = parts.iter().flat_map(|(_, chart)| *chart).collect();
    let indices = (0..parts.len() as u32).flat_map(|part| QUAD.map(|corner| part * 4 + corner)).collect();
    Mesh::from_parts(positions, None, uvs, indices, "charts").unwrap()
}

/// A flat rectangle of floor from `low` to `high` (x, z)
fn floor(low: Vec2, high: Vec2) -> [Vec3; 4] {
    [Vec3::new(low.x, 0.0, low.y), Vec3::new(high.x, 0.0, low.y), Vec3::new(high.x, 0.0, high.y), Vec3::new(low.x, 0.0, high.y)]
}

/// A chart whose corners follow [`floor`]'s
fn chart(low: Vec2, high: Vec2) -> [Vec2; 4] {
    [low, Vec2::new(high.x, low.y), high, Vec2::new(low.x, high.y)]
}

/// One dab on a floor at `center`, heading along `direction`
fn dab(center: Vec3, direction: Vec3, radius: f32) -> Stamp {
    Stamp {
        center,
        normal: Vec3::Y,
        tangent: Vec3::X,
        bitangent: Vec3::Z,
        radius,
        flow: 1.0,
        hardness: 0.9,
        depth: 1.0,
        seed: 1,
        source: None,
        travel: Some(Travel { direction: direction.normalize(), normal: Vec3::Y }),
    }
}

/// A canvas whose stroke follows its direction at full speed
fn flow_canvas() -> StrokeCanvas {
    let mut canvas = StrokeCanvas::new(SIZE, SIZE);
    canvas.flow = Some(FlowProducer::new(SIZE, SIZE, 1.0, 0.1));
    canvas
}

/// Rasterize `stamps` on `mesh` without a camera, as a scripted stroke does
fn paint(mesh: &Mesh, mut canvas: StrokeCanvas, stamps: &[Stamp]) -> StrokeCanvas {
    let bvh = Bvh::new(mesh);
    let uv_buffer = UvGBuffer::new(mesh, 0, SIZE, SIZE);
    let context = RasterCtx {
        mesh,
        bvh: &bvh,
        gbuf: &uv_buffer,
        material: 0,
        depth: None,
        eye: Vec3::new(0.0, 5.0, 0.0),
        backface_culling: false,
        angle_fade: (89.0, 90.0),
        shape: Shape { alpha: BrushAlpha::Round, image: None, library: None, params: Default::default(), level: 0 },
    };

    rasterize(&mut canvas, &context, stamps);
    canvas
}

/// The stored direction at texture coordinate (`u`, `v`), None where the flow map has no coverage
fn stored(canvas: &StrokeCanvas, u: f32, v: f32) -> Option<Vec2> {
    let producer = canvas.flow.as_ref().unwrap();
    let (x, y) = ((u * SIZE as f32) as u32, (v * SIZE as f32) as u32);
    let covered = producer.coverage.get(x, y)[0] > 0;
    let value = producer.values[&Channel::Flow].get(x, y);
    covered.then(|| Vec2::new(flow_unpack_level(value[0]), flow_unpack_level(value[1])))
}

fn stroke_alpha(canvas: &StrokeCanvas, u: f32, v: f32) -> f32 {
    from_u16(canvas.alpha.get((u * SIZE as f32) as u32, (v * SIZE as f32) as u32)[0])
}

fn close(actual: Option<Vec2>, expected: Vec2) -> bool {
    actual.is_some_and(|actual| (actual - expected).length() < STORED)
}

#[test]
fn a_dab_stores_its_travel_and_a_dab_without_travel_stores_nothing() {
    let mesh = quads(&[(floor(Vec2::splat(-1.0), Vec2::ONE), chart(Vec2::ZERO, Vec2::ONE))]);

    let heading = paint(&mesh, flow_canvas(), &[dab(Vec3::ZERO, Vec3::new(0.6, 0.0, 0.8), 0.3)]);
    assert!(close(stored(&heading, 0.5, 0.5), Vec2::new(0.6, 0.8)), "{:?}", stored(&heading, 0.5, 0.5));
    assert_eq!(stored(&heading, 0.9, 0.9), None, "outside the dab");

    // A click: the other channels get their paint, the flow map none
    let click = Stamp { travel: None, ..dab(Vec3::ZERO, Vec3::X, 0.3) };
    let clicked = paint(&mesh, flow_canvas(), &[click]);
    assert!(stroke_alpha(&clicked, 0.5, 0.5) > 0.99);
    assert_eq!(stored(&clicked, 0.5, 0.5), None);
    assert!(!clicked.flow.as_ref().unwrap().has_coverage());
    assert!(!clicked.dirty.is_empty(), "the stroke still touched the tile");
}

#[test]
fn a_coplanar_mirrored_seam_stores_the_same_surface_direction_in_both_charts() {
    // Two halves of one floor: the left chart reads left to right, the right one right to left
    let left = (floor(Vec2::new(-1.0, -1.0), Vec2::new(0.0, 1.0)), chart(Vec2::ZERO, Vec2::new(0.5, 1.0)));
    let right = (floor(Vec2::new(0.0, -1.0), Vec2::new(1.0, 1.0)), chart(Vec2::new(1.0, 0.0), Vec2::new(0.5, 1.0)));
    let mesh = quads(&[left, right]);

    // One dab across the seam, heading +X with a little +Z
    let canvas = paint(&mesh, flow_canvas(), &[dab(Vec3::ZERO, Vec3::new(0.8, 0.0, 0.6), 0.3)]);

    // World x = -0.1 is u = 0.45 on the left chart; x = 0.1 is u = 0.95 on the mirrored one
    assert!(close(stored(&canvas, 0.45, 0.5), Vec2::new(0.8, 0.6)), "{:?}", stored(&canvas, 0.45, 0.5));
    assert!(close(stored(&canvas, 0.95, 0.5), Vec2::new(-0.8, 0.6)), "{:?}", stored(&canvas, 0.95, 0.5));
}

#[test]
fn a_stretched_chart_stores_the_direction_that_moves_the_texture_along_the_stroke() {
    // The floor's Z takes half the texture's height: half as many texels per unit as X
    let mesh = quads(&[(floor(Vec2::splat(-1.0), Vec2::ONE), chart(Vec2::ZERO, Vec2::new(1.0, 0.5)))]);

    let canvas = paint(&mesh, flow_canvas(), &[dab(Vec3::ZERO, Vec3::new(1.0, 0.0, 1.0), 0.3)]);

    // A diagonal on the surface is two texels along U for each along V, at full length
    let direction = stored(&canvas, 0.5, 0.25);
    assert!(close(direction, Vec2::new(2.0, 1.0).normalize()), "{direction:?}");
}

#[test]
fn a_stroke_over_a_crease_keeps_its_angle_to_the_crease() {
    // A floor up to x = 0, then a slope going down 60 degrees; the slope's chart continues the floor's
    let down = Vec3::new(0.5, -(0.75f32.sqrt()), 0.0);
    let slope = [Vec3::new(0.0, 0.0, -1.0), down - Vec3::Z, down + Vec3::Z, Vec3::new(0.0, 0.0, 1.0)];
    let mesh =
        quads(&[(floor(Vec2::new(-1.0, -1.0), Vec2::new(0.0, 1.0)), chart(Vec2::ZERO, Vec2::new(0.5, 1.0))), (slope, chart(Vec2::new(0.5, 0.0), Vec2::ONE))]);

    // A dab on the floor beside the crease, heading across it at 45 degrees
    let canvas = paint(&mesh, flow_canvas(), &[dab(Vec3::new(-0.05, 0.0, 0.0), Vec3::new(1.0, 0.0, 1.0), 0.3)]);

    // The unfolded surface is one flat chart: 45 degrees on the floor and on the slope
    let diagonal = Vec2::ONE.normalize();
    let (on_floor, on_slope) = (stored(&canvas, 0.45, 0.5), stored(&canvas, 0.55, 0.5));
    assert!(close(on_floor, diagonal), "{on_floor:?}");
    assert!(close(on_slope, diagonal), "{on_slope:?}");
}

#[test]
fn a_triangle_without_area_takes_the_stroke_but_no_direction() {
    // A floor, and a triangle squeezed into a line on it that still owns a corner of the texture
    let positions = floor(Vec2::splat(-1.0), Vec2::ONE).into_iter().chain([Vec3::ZERO, Vec3::new(0.1, 0.0, 0.0), Vec3::new(0.2, 0.0, 0.0)]).collect();
    let uvs = chart(Vec2::ZERO, Vec2::splat(0.5)).into_iter().chain([Vec2::new(0.6, 0.6), Vec2::new(0.9, 0.6), Vec2::new(0.6, 0.9)]).collect();
    let indices = QUAD.into_iter().chain([4, 5, 6]).collect();
    let mesh = Mesh::from_parts(positions, None, uvs, indices, "line").unwrap();

    let canvas = paint(&mesh, flow_canvas(), &[dab(Vec3::ZERO, Vec3::X, 0.3)]);

    assert!(stroke_alpha(&canvas, 0.65, 0.65) > 0.5, "the stroke reaches the squeezed triangle");
    assert_eq!(stored(&canvas, 0.65, 0.65), None, "which has no direction to store");
    assert!(close(stored(&canvas, 0.25, 0.25), Vec2::X), "the floor beside it has: {:?}", stored(&canvas, 0.25, 0.25));
}

#[test]
fn the_quick_mask_limits_what_the_flow_map_takes() {
    let mesh = quads(&[(floor(Vec2::splat(-1.0), Vec2::ONE), chart(Vec2::ZERO, Vec2::ONE))]);

    // Open on the left half, half open in a band right of the middle, closed beyond
    let mut mask = TileImage::new(SIZE, SIZE, 1);
    for y in 0..SIZE {
        for x in 0..SIZE * 9 / 16 {
            mask.set(x, y, &[to_u16(if x < SIZE / 2 { 1.0 } else { 0.5 })]);
        }
    }
    let mut canvas = flow_canvas();
    canvas.limit = Some((mask, false));

    let canvas = paint(&mesh, canvas, &[dab(Vec3::ZERO, Vec3::X, 0.3)]);

    let coverage = |u: f32| from_u16(canvas.flow.as_ref().unwrap().coverage.get((u * SIZE as f32) as u32, SIZE / 2)[0]);
    assert!(coverage(0.45) > 0.99, "open: {}", coverage(0.45));
    assert!((coverage(0.53) - 0.5).abs() < 0.01, "half open: {}", coverage(0.53));
    assert_eq!(coverage(0.6), 0.0, "closed");
    assert_eq!(stored(&canvas, 0.6, 0.5), None);
}

#[test]
fn a_fill_stores_no_direction() {
    let mesh = quads(&[(floor(Vec2::splat(-1.0), Vec2::ONE), chart(Vec2::ZERO, Vec2::ONE))]);
    let uv_buffer = UvGBuffer::new(&mesh, 0, SIZE, SIZE);
    let mut canvas = flow_canvas();

    fill_triangles(&mut canvas, &uv_buffer, &BTreeSet::from([0, 1]));

    assert!(stroke_alpha(&canvas, 0.5, 0.5) > 0.99, "the fill covers the floor");
    assert!(!canvas.flow.as_ref().unwrap().has_coverage());
}
