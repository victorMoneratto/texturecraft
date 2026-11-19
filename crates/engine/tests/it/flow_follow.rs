//! Strokes that paint the direction they travel into the flow map, as the brush does by default:
//! what a stroke stores, where it stores nothing, and that the live stroke shows what is stored

use serde_json::json;
use texturecraft_engine::core::{Channel, FLOW_STILL, flow_to_polar};
use texturecraft_engine::geom::glam::{Vec2, Vec3};
use texturecraft_engine::geom::primitives;
use texturecraft_engine::texel::TILE;
use texturecraft_engine::{Session, execute};

use super::flow_map::{RESOLUTION, STORAGE_STEP, close, flow_at, flow_session};

pub(super) const VIEWPORT: f32 = 400.0;
pub(super) const RED: [f32; 3] = [1.0, 0.0, 0.0];

/// How far a stored vector may be from the one painted where every dab agrees: 16-bit storage
/// and the rounding of the dabs' own arithmetic
const SAME_VECTOR: f32 = 2e-3;

/// A viewport pixel beside the plane, over nothing
const OFF_THE_MESH: Vec2 = Vec2::splat(3.0);

/// A plane with the flow map whose brush paints the flow map alone, following its strokes
pub(super) fn follow_session() -> Session {
    let mut session = flow_session();
    execute(&mut session, "tool.material", &json!({"channel": "flow", "solo": true})).unwrap();
    execute(&mut session, "tool.brush", &json!({"size": 10, "hardness": 100})).unwrap();
    assert!(session.tool.flow_follow, "strokes follow their direction unless told otherwise");
    session
}

/// The flow vector a packed value stores: x along +U, y along +V
pub(super) fn vector_of(value: [f32; 3]) -> Vec2 {
    let (angle, speed) = flow_to_polar(value);
    let (sin, cos) = angle.to_radians().sin_cos();
    Vec2::new(cos, -sin) * speed
}

/// The composited flow vector at (`u`, `v`)
pub(super) fn vector_at(session: &Session, u: f32, v: f32) -> Vec2 {
    vector_of(flow_at(session, u, v))
}

pub(super) fn near(actual: Vec2, expected: Vec2) -> bool {
    (actual - expected).length() < SAME_VECTOR
}

pub(super) fn base_color_at(session: &Session, u: f32, v: f32) -> [f32; 3] {
    session.caches[0].composite.value(Channel::BaseColor, (u * RESOLUTION as f32) as u32, (v * RESOLUTION as f32) as u32)
}

pub(super) fn stroke_uv(session: &mut Session, points: &[[f32; 2]]) {
    execute(session, "tool.strokeUv", &json!({"points": points})).unwrap();
}

/// Look at the plane from above, with room around it in the view
pub(super) fn look_down(session: &mut Session) {
    session.viewport = Vec2::splat(VIEWPORT);
    session.camera.yaw = 0.0;
    session.camera.pitch = 1.3;
    session.camera.distance *= 1.5;
    session.update();
}

/// The viewport pixel of the plane's point (`x`, `z`); its texture coordinate is ((x + 1) / 2, (z + 1) / 2)
pub(super) fn screen(session: &Session, x: f32, z: f32) -> Vec2 {
    let ndc = session.camera.project(Vec3::new(x, 0.0, z), 1.0).unwrap();
    Vec2::new((ndc.x * 0.5 + 0.5) * session.viewport.x, (0.5 - ndc.y * 0.5) * session.viewport.y)
}

pub(super) fn stroke_3d(session: &mut Session, points: &[Vec2]) {
    let points: Vec<[f32; 2]> = points.iter().map(|point| point.to_array()).collect();
    execute(session, "tool.stroke", &json!({"points": points})).unwrap();
}

/// Texture coordinates along a circle of `radius` around `center`, every two degrees; the
/// stroke through them heads along (-sin, cos) at each angle
fn arc(center: Vec2, radius: f32, degrees: std::ops::RangeInclusive<u32>) -> Vec<[f32; 2]> {
    degrees.step_by(2).map(|angle| (center + radius * Vec2::from_angle((angle as f32).to_radians())).to_array()).collect()
}

fn degrees_between(a: Vec2, b: Vec2) -> f32 {
    a.normalize_or_zero().dot(b.normalize_or_zero()).clamp(-1.0, 1.0).acos().to_degrees()
}

#[test]
fn a_stroke_along_u_stores_one_zero() {
    let mut session = follow_session();

    stroke_uv(&mut session, &[[0.3, 0.5], [0.7, 0.5]]);

    assert!(near(vector_at(&session, 0.5, 0.5), Vec2::X), "{}", vector_at(&session, 0.5, 0.5));
    assert!(close(flow_at(&session, 0.5, 0.5), [1.0, 0.5, 0.0]), "{:?}", flow_at(&session, 0.5, 0.5));
    assert!(close(flow_at(&session, 0.5, 0.9), FLOW_STILL), "away from the stroke nothing moves");

    // The speed control sets the length; the stroke still gives the direction: towards smaller V here
    execute(&mut session, "tool.material", &json!({"channel": "flow", "speed": 0.5})).unwrap();
    assert!(session.tool.flow_follow, "a speed alone leaves strokes following");
    stroke_uv(&mut session, &[[0.2, 0.9], [0.2, 0.7]]);

    assert!(near(vector_at(&session, 0.2, 0.8), Vec2::new(0.0, -0.5)), "{}", vector_at(&session, 0.2, 0.8));
}

#[test]
fn a_stroke_in_the_3d_view_stores_the_way_it_went_across_the_texture() {
    let mut session = follow_session();
    look_down(&mut session);
    let (from, to) = (screen(&session, -0.5, 0.3), screen(&session, 0.5, -0.2));
    let uv = |session: &Session, pixel: Vec2| session.pick(pixel, session.viewport).unwrap().uv;
    let (uv_from, uv_to) = (uv(&session, from), uv(&session, to));

    stroke_3d(&mut session, &[from, to]);

    // The direction between the two ends as the texture has them, measured apart from the stroke
    let expected = ((uv_to - uv_from) * RESOLUTION as f32).normalize();
    assert!((expected - Vec2::new(1.0, -0.5).normalize()).length() < 1e-3, "the plane's chart is its top view: {expected}");
    let middle = (uv_from + uv_to) / 2.0;
    assert!(near(vector_at(&session, middle.x, middle.y), expected), "{}", vector_at(&session, middle.x, middle.y));
    assert!(close(flow_at(&session, 0.1, 0.9), FLOW_STILL));
}

#[test]
fn a_slow_drag_of_texel_steps_flows_along_the_drag() {
    let mut session = follow_session();

    // A pointer moved slowly down and to the right: one texel right, one texel down, and so on
    let texel = 1.0 / RESOLUTION as f32;
    let mut pointer = Vec2::new(0.25, 0.25);
    session.begin_stroke_uv(pointer, 1.0).unwrap();
    for step in 0..256 {
        pointer += if step % 2 == 0 { Vec2::X * texel } else { Vec2::Y * texel };
        session.continue_stroke_uv(pointer, 1.0);
    }
    session.end_stroke().unwrap();
    session.update();

    // All along it the flow is the drag's diagonal, not a mix of the steps that made it
    for along in [0.3, 0.35, 0.4, 0.45, 0.5, 0.55, 0.6, 0.65, 0.7] {
        let stored = vector_at(&session, along, along);
        assert!(degrees_between(stored, Vec2::ONE) < 1.0 && (stored.length() - 1.0).abs() < 0.01, "at {along}: {stored}");
    }
}

#[test]
fn turning_follow_off_paints_the_fixed_direction_again() {
    let mut session = follow_session();

    // A fixed direction given by angle is what the next stroke paints
    execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": 90.0})).unwrap();
    assert!(!session.tool.flow_follow);
    stroke_uv(&mut session, &[[0.3, 0.3], [0.7, 0.3]]);
    assert!(near(vector_at(&session, 0.5, 0.3), Vec2::NEG_Y), "{}", vector_at(&session, 0.5, 0.3));

    // Following again, the same drag stores where it went
    execute(&mut session, "tool.material", &json!({"channel": "flow", "follow": true})).unwrap();
    stroke_uv(&mut session, &[[0.3, 0.7], [0.7, 0.7]]);
    assert!(near(vector_at(&session, 0.5, 0.7), Vec2::X), "{}", vector_at(&session, 0.5, 0.7));

    // And off by name, the fixed direction is still the one given before
    execute(&mut session, "tool.material", &json!({"channel": "flow", "follow": false})).unwrap();
    stroke_uv(&mut session, &[[0.3, 0.9], [0.7, 0.9]]);
    assert!(near(vector_at(&session, 0.5, 0.9), Vec2::NEG_Y), "{}", vector_at(&session, 0.5, 0.9));
}

#[test]
fn a_circular_stroke_is_tangential_at_its_centreline() {
    let mut session = follow_session();
    let (center, radius) = (Vec2::splat(0.5), 0.25);

    stroke_uv(&mut session, &arc(center, radius, 0..=300));

    for degrees in [40.0f32, 130.0, 215.0, 280.0] {
        let direction = Vec2::from_angle(degrees.to_radians());
        let on_the_circle = center + radius * direction;
        let stored = vector_at(&session, on_the_circle.x, on_the_circle.y);

        // A quarter turn on from the radius, at full speed
        assert!(degrees_between(stored, direction.perp()) < 3.0, "{degrees}: {stored}");
        assert!((stored.length() - 1.0).abs() < 0.01, "{degrees}: {stored}");
    }
}

#[test]
fn a_soft_edged_curve_fades_out_without_turning() {
    let mut session = follow_session();
    execute(&mut session, "tool.brush", &json!({"size": 12, "hardness": 0})).unwrap();
    let (center, radius, brush_radius) = (Vec2::splat(0.5), 0.25, 0.06);

    stroke_uv(&mut session, &arc(center, radius, 0..=300));

    let direction = Vec2::from_angle(150.0f32.to_radians());
    let at = |from_center: f32| {
        let point = center + from_center * direction;
        vector_at(&session, point.x, point.y)
    };
    let (middle, edge) = (at(radius), at(radius + 0.8 * brush_radius));

    // Over still water a soft edge is slower flow the same way, not flow some other way
    assert!(middle.length() > 0.95, "{middle}");
    assert!(edge.length() > 0.05 && edge.length() < 0.8, "{edge}");
    assert!(degrees_between(edge, direction.perp()) < 5.0 && degrees_between(middle, direction.perp()) < 3.0, "{middle} {edge}");
    assert_eq!(at(radius + 1.2 * brush_radius), Vec2::ZERO, "beyond the brush");
}

#[test]
fn a_second_pass_over_the_same_place_replaces_the_first() {
    // One stroke: to the right, round the top, then down across where it began
    let mut session = follow_session();
    stroke_uv(&mut session, &[[0.2, 0.5], [0.8, 0.5], [0.8, 0.2], [0.5, 0.2], [0.5, 0.8]]);

    assert!(near(vector_at(&session, 0.5, 0.5), Vec2::Y), "the crossing flows the later way: {}", vector_at(&session, 0.5, 0.5));
    assert!(near(vector_at(&session, 0.35, 0.5), Vec2::X), "beside it the first pass stays");
    assert!(near(vector_at(&session, 0.5, 0.65), Vec2::Y));

    // Two strokes: the later one wins the same way
    let mut session = follow_session();
    stroke_uv(&mut session, &[[0.2, 0.5], [0.8, 0.5]]);
    stroke_uv(&mut session, &[[0.5, 0.2], [0.5, 0.8]]);

    assert!(near(vector_at(&session, 0.5, 0.5), Vec2::Y), "{}", vector_at(&session, 0.5, 0.5));
    assert!(near(vector_at(&session, 0.35, 0.5), Vec2::X));
}

#[test]
fn a_tight_reversal_leaves_no_flow_at_the_turn() {
    let mut session = follow_session();
    execute(&mut session, "tool.material", &json!({"channel": "baseColor", "color": RED})).unwrap();

    // Out to the right and straight back; the brush is 0.1 across
    stroke_uv(&mut session, &[[0.3, 0.5], [0.7, 0.5], [0.3, 0.5]]);

    // Within a brush width before the turn the two ways cancel: no flow is painted, the colour is
    for u in [0.62, 0.66, 0.7] {
        assert!(close(flow_at(&session, u, 0.5), FLOW_STILL), "{u}: {:?}", flow_at(&session, u, 0.5));
        assert!(close(base_color_at(&session, u, 0.5), RED), "{u}: the other channel takes the whole stroke");
    }

    // Further back the way home painted over the way out
    assert!(near(vector_at(&session, 0.45, 0.5), Vec2::NEG_X), "{}", vector_at(&session, 0.45, 0.5));
}

/// The composited flow map and base color of every texel
pub(super) fn snapshot(session: &Session) -> Vec<[[f32; 3]; 2]> {
    let composite = &session.caches[0].composite;
    let texels = (0..RESOLUTION).flat_map(|y| (0..RESOLUTION).map(move |x| (x, y)));
    texels.map(|(x, y)| [composite.value(Channel::Flow, x, y), composite.value(Channel::BaseColor, x, y)]).collect()
}

/// The largest difference of any component between two snapshots
pub(super) fn largest_difference(a: &[[[f32; 3]; 2]], b: &[[[f32; 3]; 2]]) -> f32 {
    let components = |snapshot: &[[[f32; 3]; 2]]| snapshot.iter().flatten().flatten().copied().collect::<Vec<f32>>();
    components(a).iter().zip(components(b)).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max)
}

/// What a commit may round differently from the live stroke: it stores 16-bit premultiplied values
pub(super) const COMMIT_ROUNDING: f32 = 4.0 * STORAGE_STEP;

#[test]
fn the_live_stroke_shows_what_the_commit_stores() {
    let mut session = follow_session();
    execute(&mut session, "tool.material", &json!({"channel": "baseColor", "color": RED})).unwrap();
    execute(&mut session, "tool.brush", &json!({"size": 12, "hardness": 30})).unwrap();
    let untouched = snapshot(&session);
    let steps = session.history_labels().len();

    // A soft curve: its texels differ in direction and in coverage
    let points = arc(Vec2::splat(0.5), 0.2, 20..=250);
    session.begin_stroke_uv(Vec2::from(points[0]), 1.0).unwrap();
    for point in &points[1..] {
        session.continue_stroke_uv(Vec2::from(*point), 1.0);
    }
    session.update();
    let live = snapshot(&session);
    assert_eq!(session.history_labels().len(), steps, "still being drawn");

    session.end_stroke().unwrap();
    session.update();
    let committed = snapshot(&session);

    assert_eq!(session.history_labels().len(), steps + 1);
    assert!(largest_difference(&untouched, &committed) > 0.4, "the stroke painted");
    let difference = largest_difference(&live, &committed);
    assert!(difference <= COMMIT_ROUNDING, "the live stroke differs from the stored one by {difference}");

    // Neighbours in one tile hold different directions, and the live stroke had each of them
    let (x, y) = ((0.5 * RESOLUTION as f32) as u32, (0.7 * RESOLUTION as f32) as u32);
    let texel = |snapshot: &[[[f32; 3]; 2]], x: u32, y: u32| snapshot[(y * RESOLUTION + x) as usize][0];
    assert_eq!((x / TILE, y / TILE), ((x + 3) / TILE, y / TILE), "one tile");
    assert!(vector_of(texel(&committed, x, y)).length() > 0.9 && vector_of(texel(&committed, x + 3, y)).length() > 0.9);
    assert!(degrees_between(vector_of(texel(&committed, x, y)), vector_of(texel(&committed, x + 3, y))) > 1.0);
}

#[test]
fn the_live_stroke_paints_no_flow_on_a_tile_it_only_touched() {
    let mut session = follow_session();
    execute(&mut session, "tool.material", &json!({"channel": "baseColor", "color": RED})).unwrap();
    look_down(&mut session);
    assert!(session.pick(OFF_THE_MESH, session.viewport).is_none());

    // A drag in one corner, off the mesh, and back onto it in the opposite corner for one dab
    let (from, to, back) = (screen(&session, -0.6, -0.5), screen(&session, -0.2, -0.5), screen(&session, 0.6, 0.6));
    assert!(session.begin_stroke(from, session.viewport, 1.0).unwrap());
    assert!(session.continue_stroke(to, 1.0));
    assert!(!session.continue_stroke(OFF_THE_MESH, 1.0));
    assert!(session.continue_stroke(back, 1.0));
    session.update();
    let live = snapshot(&session);

    // The dab that came back has no direction: colour, and no flow, on a tile of its own
    let (dragged, dabbed) = (Vec2::new(0.3, 0.25), Vec2::new(0.8, 0.8));
    assert_ne!(((dragged.x * RESOLUTION as f32) as u32 / TILE, (dragged.y * RESOLUTION as f32) as u32 / TILE), (3, 3));
    assert_eq!(((dabbed.x * RESOLUTION as f32) as u32 / TILE, (dabbed.y * RESOLUTION as f32) as u32 / TILE), (3, 3));
    assert!(near(vector_at(&session, dragged.x, dragged.y), Vec2::X), "{}", vector_at(&session, dragged.x, dragged.y));
    assert!(close(base_color_at(&session, dabbed.x, dabbed.y), RED));
    assert!(close(flow_at(&session, dabbed.x, dabbed.y), FLOW_STILL), "{:?}", flow_at(&session, dabbed.x, dabbed.y));

    session.end_stroke().unwrap();
    session.update();

    let difference = largest_difference(&live, &snapshot(&session));
    assert!(difference <= COMMIT_ROUNDING, "the live stroke differs from the stored one by {difference}");
    assert!(close(flow_at(&session, dabbed.x, dabbed.y), FLOW_STILL));
}

#[test]
fn a_click_on_the_flow_map_alone_changes_nothing() {
    let mut session = follow_session();
    look_down(&mut session);
    session.modified = false;
    let (steps, pixels) = (session.history_labels(), session.doc.pixels.clone());

    // A click in the 2D view and one in the 3D view: neither went anywhere
    stroke_uv(&mut session, &[[0.5, 0.5]]);
    let on_the_plane = screen(&session, 0.3, 0.3);
    stroke_3d(&mut session, &[on_the_plane]);

    assert_eq!(session.history_labels(), steps, "nothing to undo");
    assert!(!session.modified, "nothing to save");
    assert!(session.doc.pixels.layers == pixels.layers && session.doc.pixels.masks == pixels.masks, "no pixel changed");
    assert!(close(flow_at(&session, 0.5, 0.5), FLOW_STILL));

    // With base colour on as well the same click paints base colour, and still no flow
    execute(&mut session, "tool.material", &json!({"channel": "baseColor", "color": RED})).unwrap();
    stroke_uv(&mut session, &[[0.5, 0.5]]);

    assert_eq!(session.history_labels().len(), steps.len() + 1);
    assert!(session.modified);
    assert!(close(base_color_at(&session, 0.5, 0.5), RED));
    assert!(close(flow_at(&session, 0.5, 0.5), FLOW_STILL));

    // One undo step takes a whole followed stroke away
    execute(&mut session, "tool.material", &json!({"channel": "flow", "solo": true})).unwrap();
    stroke_uv(&mut session, &[[0.2, 0.8], [0.5, 0.8], [0.8, 0.8]]);
    assert_eq!(session.history_labels().len(), steps.len() + 2, "one stroke, one step");
    assert!(near(vector_at(&session, 0.5, 0.8), Vec2::X));
    execute(&mut session, "edit.undo", &serde_json::Value::Null).unwrap();
    session.update();
    assert!(close(flow_at(&session, 0.5, 0.8), FLOW_STILL));
}

#[test]
fn coming_back_onto_the_mesh_paints_no_stale_direction() {
    let mut session = follow_session();
    look_down(&mut session);
    assert!(session.pick(OFF_THE_MESH, session.viewport).is_none());

    // To the right, off the mesh, back on it elsewhere and then towards smaller V
    let points = [screen(&session, -0.6, -0.4), screen(&session, -0.1, -0.4), OFF_THE_MESH, screen(&session, 0.4, 0.5), screen(&session, 0.4, 0.0)];
    stroke_3d(&mut session, &points);

    assert!(near(vector_at(&session, 0.3, 0.3), Vec2::X), "{}", vector_at(&session, 0.3, 0.3));

    // Where it came back the flow is the way it went on, with nothing of the way it had left
    assert!(near(vector_at(&session, 0.7, 0.75), Vec2::NEG_Y), "{}", vector_at(&session, 0.7, 0.75));
    assert!(near(vector_at(&session, 0.7, 0.6), Vec2::NEG_Y));

    // Ending where it came back, that last dab went nowhere: no flow
    let mut session = follow_session();
    look_down(&mut session);
    stroke_3d(&mut session, &points[..4]);

    assert!(near(vector_at(&session, 0.3, 0.3), Vec2::X));
    assert!(close(flow_at(&session, 0.7, 0.75), FLOW_STILL), "{:?}", flow_at(&session, 0.7, 0.75));
}

#[test]
fn a_diagonal_on_a_udim_atlas_is_a_diagonal_in_texels_whatever_the_tile_resolution() {
    // Two UV tiles side by side: an atlas twice as wide as it is high
    let mut session = Session::new_project("Flow", primitives::udim_plane(), "builtin:udim_plane", None, RESOLUTION).unwrap();
    session.viewport = Vec2::splat(VIEWPORT);
    session.camera.yaw = 0.0;
    session.camera.pitch = 1.3;
    session.update();
    execute(&mut session, "textureSet.channel.add", &json!({"channel": "flow"})).unwrap();
    execute(&mut session, "tool.paint", &serde_json::Value::Null).unwrap();
    execute(&mut session, "tool.material", &json!({"channel": "flow", "solo": true})).unwrap();
    execute(&mut session, "tool.brush", &json!({"size": 4, "hardness": 100})).unwrap();
    assert_eq!(session.caches[0].size, (2 * RESOLUTION, RESOLUTION));

    // Paint the same diagonal on the surface around x = `center`; returns what the middle stores,
    // the direction between the stroke's ends as the atlas has them, and their distance in texels
    let paint_diagonal = |session: &mut Session, center: f32| {
        let (from, to) = (screen(session, center - 0.3, -0.3), screen(session, center + 0.3, 0.3));
        let atlas = Vec2::new(session.caches[0].size.0 as f32, session.caches[0].size.1 as f32);
        let texel = |session: &Session, pixel: Vec2| session.pick(pixel, session.viewport).unwrap().uv * atlas;
        let (texel_from, texel_to) = (texel(session, from), texel(session, to));
        stroke_3d(session, &[from, to]);

        let middle = (texel_from + texel_to) / 2.0;
        let stored = vector_of(session.caches[0].composite.value(Channel::Flow, middle.x as u32, middle.y as u32));
        (stored, (texel_to - texel_from).normalize(), (texel_to - texel_from).length())
    };
    let diagonal = Vec2::ONE.normalize();

    // Tile 1001 at full resolution. In UV units the atlas would squeeze U to half and tilt the direction
    let (stored, between_ends, full_length) = paint_diagonal(&mut session, -1.0);
    assert!(near(between_ends, diagonal), "{between_ends}");
    assert!(near(stored, diagonal), "{stored}");

    // Tile 1002 at half resolution: fewer texels under the same stroke, the same direction
    let reply = execute(&mut session, "textureSet.tile.resolution", &json!({"tile": 1002, "resolution": RESOLUTION / 2})).unwrap();
    assert_eq!(reply["resolution"], json!(RESOLUTION / 2));
    session.update();
    let (stored, between_ends, half_length) = paint_diagonal(&mut session, 1.0);
    assert!((half_length / full_length - 0.5).abs() < 0.01, "{half_length} {full_length}");
    assert!(near(between_ends, diagonal), "{between_ends}");
    assert!(near(stored, diagonal), "{stored}");

    // The 2D view steps in UV units: the same step is twice as many texels across this atlas
    stroke_uv(&mut session, &[[0.05, 0.6], [0.15, 0.8]]);
    let (width, height) = session.caches[0].size;
    let stored = vector_of(session.caches[0].composite.value(Channel::Flow, (0.1 * width as f32) as u32, (0.7 * height as f32) as u32));
    assert_eq!((width, height), (2 * RESOLUTION, RESOLUTION));
    assert!(near(stored, diagonal), "{stored}");
}

/// The four places a stroke through the plane's point (0.5, 0) and its quarter-turn copies paint,
/// as texture coordinates, with the direction each must store when the stroke heads along (2, 1)
fn quarter_turns() -> [(Vec2, Vec2); 4] {
    let heading = Vec2::new(2.0, 1.0).normalize();
    [(Vec2::new(0.75, 0.5), heading), (Vec2::new(0.5, 0.25), heading.perp() * -1.0), (Vec2::new(0.25, 0.5), -heading), (Vec2::new(0.5, 0.75), heading.perp())]
}

#[test]
fn symmetry_copies_in_the_3d_view_paint_the_reflected_and_the_turned_direction() {
    let mut session = follow_session();
    look_down(&mut session);
    let reply = execute(&mut session, "tool.brush", &json!({"symmetryX": true, "followPath": false})).unwrap();
    assert_eq!(reply["followPath"], json!(false), "the dabs themselves do not turn with the stroke");

    // A diagonal on the right half; its mirror image runs the other way along U
    let points = [screen(&session, 0.3, -0.4), screen(&session, 0.7, 0.0)];
    stroke_3d(&mut session, &points);

    let diagonal = Vec2::ONE.normalize();
    assert!(near(vector_at(&session, 0.75, 0.4), diagonal), "{}", vector_at(&session, 0.75, 0.4));
    assert!(near(vector_at(&session, 0.25, 0.4), diagonal * Vec2::new(-1.0, 1.0)), "{}", vector_at(&session, 0.25, 0.4));

    // Four copies a quarter turn apart about the plane's normal
    let mut session = follow_session();
    look_down(&mut session);
    execute(&mut session, "tool.brush", &json!({"symmetryRadial": 4, "symmetryAxis": "y", "followPath": false})).unwrap();
    let points = [screen(&session, 0.3, -0.1), screen(&session, 0.7, 0.1)];
    stroke_3d(&mut session, &points);

    for (at, direction) in quarter_turns() {
        assert!(near(vector_at(&session, at.x, at.y), direction), "at {at}: {} for {direction}", vector_at(&session, at.x, at.y));
    }
}

#[test]
fn symmetry_copies_in_the_2d_view_paint_the_reflected_and_the_turned_direction() {
    let mut session = follow_session();
    execute(&mut session, "tool.brush", &json!({"symmetryX": true, "followPath": false})).unwrap();

    stroke_uv(&mut session, &[[0.65, 0.3], [0.85, 0.5]]);

    let diagonal = Vec2::ONE.normalize();
    assert!(near(vector_at(&session, 0.75, 0.4), diagonal), "{}", vector_at(&session, 0.75, 0.4));
    assert!(near(vector_at(&session, 0.25, 0.4), diagonal * Vec2::new(-1.0, 1.0)), "{}", vector_at(&session, 0.25, 0.4));

    let mut session = follow_session();
    execute(&mut session, "tool.brush", &json!({"symmetryRadial": 4, "symmetryAxis": "y", "followPath": false})).unwrap();
    stroke_uv(&mut session, &[[0.65, 0.45], [0.85, 0.55]]);

    for (at, direction) in quarter_turns() {
        assert!(near(vector_at(&session, at.x, at.y), direction), "at {at}: {} for {direction}", vector_at(&session, at.x, at.y));
    }
}

#[test]
fn the_quick_mask_limits_where_a_stroke_leaves_flow() {
    let mut session = follow_session();

    // Open the mask around (0.35, 0.5) only
    execute(&mut session, "viewport.quickMask", &json!({"enabled": true})).unwrap();
    execute(&mut session, "viewport.quickMask.edit", &json!({"editing": true})).unwrap();
    execute(&mut session, "tool.brush", &json!({"size": 16, "hardness": 100})).unwrap();
    stroke_uv(&mut session, &[[0.35, 0.5]]);
    execute(&mut session, "viewport.quickMask.edit", &json!({"editing": false})).unwrap();

    execute(&mut session, "tool.brush", &json!({"size": 6, "hardness": 100})).unwrap();
    stroke_uv(&mut session, &[[0.2, 0.5], [0.8, 0.5]]);

    assert!(near(vector_at(&session, 0.35, 0.5), Vec2::X), "inside the mask: {}", vector_at(&session, 0.35, 0.5));
    assert!(close(flow_at(&session, 0.6, 0.5), FLOW_STILL), "outside the mask: {:?}", flow_at(&session, 0.6, 0.5));
    assert!(close(flow_at(&session, 0.22, 0.5), FLOW_STILL));

    // Just past the mask's edge, on a tile the stroke does paint
    assert!(close(flow_at(&session, 0.46, 0.5), FLOW_STILL), "{:?}", flow_at(&session, 0.46, 0.5));
    assert!(near(vector_at(&session, 0.42, 0.5), Vec2::X));
}
