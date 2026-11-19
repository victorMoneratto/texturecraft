//! Where a followed stroke's flow begins and how it ends where the stroke turns back: the flow
//! covers the stroke from its first dab, and falls off in steps towards a retraced turn

use serde_json::json;
use texturecraft_engine::core::{Channel, LayerId};
use texturecraft_engine::geom::glam::Vec2;
use texturecraft_engine::texel::from_u16;
use texturecraft_engine::{Session, execute};

use super::flow_follow::{RED, follow_session, look_down, near, screen, stroke_3d, stroke_uv, vector_at};
use super::flow_map::RESOLUTION;

/// Coverage differs by at most this between two channels painted by the same dabs: 16-bit
/// storage of each dab's alpha "over" the ones before
const SAME_COVERAGE: f32 = 4.0 / 65535.0;

/// A plane whose brush paints base colour and, following its strokes, the flow map
fn two_channel_session() -> (Session, LayerId) {
    let mut session = follow_session();
    execute(&mut session, "tool.material", &json!({"channel": "baseColor", "color": RED})).unwrap();
    let layer = session.selected_layer().unwrap();
    (session, layer)
}

/// How much of texel (`x`, `y`) of `layer` the stroke painted in `channel`
fn coverage(session: &Session, layer: LayerId, channel: Channel, x: u32, y: u32) -> f32 {
    session.doc.pixels.layer(layer, channel).map_or(0.0, |image| from_u16(image.get(x, y).last().copied().unwrap_or(0)))
}

/// The texels where the two channels' coverage differs, and how many texels base colour covers
fn footprint_difference(session: &Session, layer: LayerId) -> (Vec<(u32, u32, f32, f32)>, usize) {
    let mut different = Vec::new();
    let mut painted = 0;
    for y in 0..RESOLUTION {
        for x in 0..RESOLUTION {
            let (colour, flow) = (coverage(session, layer, Channel::BaseColor, x, y), coverage(session, layer, Channel::Flow, x, y));
            painted += usize::from(colour > 0.0);
            if (colour - flow).abs() > SAME_COVERAGE {
                different.push((x, y, colour, flow));
            }
        }
    }
    (different, painted)
}

#[test]
fn the_flow_of_a_stroke_starts_where_its_paint_starts() {
    let (mut session, layer) = two_channel_session();

    stroke_uv(&mut session, &[[0.3, 0.5], [0.7, 0.5]]);

    // The first dab had no travel when it was painted: it takes the direction the stroke left it in
    let (different, painted) = footprint_difference(&session, layer);
    assert!(painted > 1000, "the stroke painted: {painted} texels");
    assert!(different.is_empty(), "{} texels of {painted} differ, the first {:?}", different.len(), different.first());

    // Behind the first dab's centre, where only that dab reached
    let behind = 0.3 - 0.75 * session.tool.brush.uv_stamp_radius(1.0);
    assert!(coverage(&session, layer, Channel::BaseColor, (behind * RESOLUTION as f32) as u32, RESOLUTION / 2) > 0.99);
    assert!(near(vector_at(&session, behind, 0.5), Vec2::X), "{}", vector_at(&session, behind, 0.5));
}

#[test]
fn the_flow_of_a_stroke_in_the_3d_view_starts_where_its_paint_starts() {
    let (mut session, layer) = two_channel_session();
    look_down(&mut session);

    let points = [screen(&session, -0.5, 0.0), screen(&session, 0.5, 0.0)];
    stroke_3d(&mut session, &points);

    let (different, painted) = footprint_difference(&session, layer);
    assert!(painted > 1000, "the stroke painted: {painted} texels");
    assert!(different.is_empty(), "{} texels of {painted} differ, the first {:?}", different.len(), different.first());
}

#[test]
fn the_copies_of_a_mirrored_stroke_start_with_flow_too() {
    for in_3d in [false, true] {
        let (mut session, layer) = two_channel_session();
        execute(&mut session, "tool.brush", &json!({"symmetryOn": true, "symmetryX": true})).unwrap();
        if in_3d {
            look_down(&mut session);
            let points = [screen(&session, 0.3, -0.4), screen(&session, 0.6, 0.3)];
            stroke_3d(&mut session, &points);
        } else {
            stroke_uv(&mut session, &[[0.65, 0.3], [0.8, 0.65]]);
        }

        let (different, painted) = footprint_difference(&session, layer);
        assert!(painted > 1000, "3D {in_3d}: the stroke and its copy painted: {painted} texels");
        assert!(different.is_empty(), "3D {in_3d}: {} texels of {painted} differ, the first {:?}", different.len(), different.first());
    }
}

#[test]
fn a_stroke_that_comes_back_onto_the_mesh_has_flow_from_its_first_dab_there() {
    let (mut session, layer) = two_channel_session();
    look_down(&mut session);

    // Off the plane's edge and back on further along
    let off_the_mesh = Vec2::splat(3.0);
    let points = [screen(&session, -0.6, -0.4), screen(&session, -0.2, -0.4), off_the_mesh, screen(&session, 0.2, 0.4), screen(&session, 0.6, 0.4)];
    stroke_3d(&mut session, &points);

    let (different, painted) = footprint_difference(&session, layer);
    assert!(painted > 1000, "{painted}");
    assert!(different.is_empty(), "{} texels of {painted} differ, the first {:?}", different.len(), different.first());
}

#[test]
fn a_click_still_leaves_no_flow() {
    let (mut session, layer) = two_channel_session();

    stroke_uv(&mut session, &[[0.5, 0.5]]);

    let centre = RESOLUTION / 2;
    assert!(coverage(&session, layer, Channel::BaseColor, centre, centre) > 0.99, "the click paints base colour");
    assert_eq!(session.doc.pixels.layer(layer, Channel::Flow).map_or(0, |image| image.allocated_count()), 0, "a dab alone was heading nowhere");
}

/// A stroke along the middle from u = 0.3 to 0.7 and straight back with a brush 0.1 across:
/// the flow map's coverage and the composited flow along U, per texel from the turn backwards
fn retraced_before_the_turn(hardness: u32, spacing: u32) -> Vec<(f32, f32)> {
    let mut session = follow_session();
    execute(&mut session, "tool.brush", &json!({"hardness": hardness, "spacing": spacing})).unwrap();
    let layer = session.selected_layer().unwrap();

    stroke_uv(&mut session, &[[0.3, 0.5], [0.7, 0.5], [0.3, 0.5]]);

    let (turn, row) = ((0.7 * RESOLUTION as f32) as u32, RESOLUTION / 2);
    let texels = (0..turn - (0.4 * RESOLUTION as f32) as u32).map(|back| turn - back);
    texels.map(|x| (coverage(&session, layer, Channel::Flow, x, row), vector_at(&session, (x as f32 + 0.5) / RESOLUTION as f32, 0.5).x)).collect()
}

#[test]
fn the_flow_of_a_retraced_stroke_falls_off_towards_the_turn() {
    // The brush is 25.6 texels across, with a dab every 2.56 texels or every 1.28
    const DIAMETER: usize = 26;
    for (hardness, spacing) in [(100, 10), (50, 10), (100, 5)] {
        let texels = retraced_before_the_turn(hardness, spacing);
        let coverages: Vec<f32> = texels.iter().map(|(coverage, _)| *coverage).collect();
        let shown: Vec<i32> = coverages.iter().map(|coverage| (coverage * 100.0).round() as i32).collect();

        // Within a diameter of the turn the two ways cancel, and a diameter and a half back the way home is all there
        assert!(coverages[2..DIAMETER - 1].iter().all(|coverage| *coverage == 0.0), "hardness {hardness} spacing {spacing}: {shown:?}");
        assert!(coverages[DIAMETER * 3 / 2 + 1..].iter().all(|coverage| *coverage > 0.99), "hardness {hardness} spacing {spacing}: {shown:?}");

        // Between the two it rises a dab at a time, with no edge
        let steps: Vec<f32> = coverages.windows(2).skip(2).map(|pair| pair[1] - pair[0]).collect();
        let largest = steps.iter().copied().fold(0.0, f32::max);
        assert!(steps.iter().all(|step| *step > -1e-3), "hardness {hardness} spacing {spacing}: {shown:?}");
        assert!(largest < 0.4, "hardness {hardness} spacing {spacing}: a step of {largest} in {shown:?}");
        let levels = shown.iter().filter(|percent| **percent > 0 && **percent < 100).collect::<std::collections::BTreeSet<_>>().len();
        assert!(levels >= 4, "hardness {hardness} spacing {spacing}: {shown:?}");

        // Nowhere does the way out show through: the flow is still or heads home
        assert!(texels.iter().skip(2).all(|(_, along_u)| *along_u <= 1e-3), "hardness {hardness} spacing {spacing}: {texels:?}");
    }
}
