//! The raw flow map view, rendered without a window: the 3D and 2D views show the channel in
//! the colours it exports, follow a stroke in progress and never keep a stale picture

use serde_json::{Value, json};
use texturecraft_automation::render::Gpu;
use texturecraft_engine::geom::glam::Vec2;
use texturecraft_engine::geom::primitives;
use texturecraft_engine::{Session, execute};
use texturecraft_render::ViewKind;

const RESOLUTION: u32 = 256;

/// Side of the square renders, in pixels
const VIEW: u32 = 240;

/// Still water as the view and an 8-bit export show it
const STILL: [u8; 3] = [128, 128, 0];

/// A flow to the right at full speed: red at its top level, green still
const RIGHT: [u8; 3] = [255, 128, 0];

/// A texture coordinate inside the stroke of `stroke_points`, and one far from it. Both keep
/// clear of the plane's edges, which the 2D view draws as a wireframe
const PAINTED_UV: [f32; 2] = [0.44, 0.36];
const UNPAINTED_UV: [f32; 2] = [0.44, 0.86];

/// A plane whose UVs fill the texture, with the flow map, showing it, ready to paint it flowing right
fn flow_session() -> Session {
    let mut session = Session::new_project("Flow", primitives::plane(2.0, 4), "builtin:plane", None, RESOLUTION).unwrap();
    execute(&mut session, "textureSet.channel.add", &json!({"channel": "flow"})).unwrap();
    execute(&mut session, "tool.paint", &Value::Null).unwrap();
    execute(&mut session, "tool.brush", &json!({"size": 20, "hardness": 100})).unwrap();
    execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": 0.0, "speed": 1.0, "solo": true})).unwrap();
    execute(&mut session, "viewport.view", &json!({"mode": "flow"})).unwrap();
    session
}

/// The points of one stroke through `PAINTED_UV`
fn stroke_points() -> [Vec2; 2] {
    [Vec2::new(0.3, PAINTED_UV[1]), Vec2::new(0.7, PAINTED_UV[1])]
}

fn gpu() -> Option<Gpu> {
    match Gpu::new() {
        Ok(gpu) => Some(gpu),
        Err(error) => {
            eprintln!("skipping: {error}");
            None
        }
    }
}

/// The 2D view's pixel at texture coordinate `uv`: the texture fills the middle 92% of a square view
fn pixel_2d(gpu: &mut Gpu, session: &mut Session, uv: [f32; 2]) -> [u8; 3] {
    let camera = session.camera;
    let (width, _, pixels) = gpu.render(session, ViewKind::Uv { ts: 0, zoom: 1.0, pan: [0.0, 0.0] }, [VIEW, VIEW], camera).unwrap();
    let at = |t: f32| ((0.5 + (t - 0.5) * 0.92) * VIEW as f32) as u32;
    rgb(&pixels, width, at(uv[0]), at(uv[1]))
}

/// The 3D view's pixel where the camera sees texture coordinate `uv`
fn pixel_3d(gpu: &mut Gpu, session: &mut Session, uv: [f32; 2]) -> [u8; 3] {
    let camera = session.camera;
    let viewport = Vec2::splat(VIEW as f32);
    let distance = |x: u32, y: u32| {
        let hit = session.pick(Vec2::new(x as f32 + 0.5, y as f32 + 0.5), viewport)?;
        Some((hit.uv - Vec2::from(uv)).length())
    };
    let (x, y, nearest) = (0..VIEW)
        .flat_map(|y| (0..VIEW).map(move |x| (x, y)))
        .filter_map(|(x, y)| distance(x, y).map(|d| (x, y, d)))
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .expect("the camera sees the plane");
    assert!(nearest < 0.02, "a pixel shows {uv:?}: nearest is {nearest} away");

    let (width, _, pixels) = gpu.render(session, ViewKind::Scene, [VIEW, VIEW], camera).unwrap();
    rgb(&pixels, width, x, y)
}

fn rgb(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 3] {
    let start = ((y * width + x) * 4) as usize;
    [pixels[start], pixels[start + 1], pixels[start + 2]]
}

/// Within one level per component: the view rounds once more than the export
fn shows(actual: [u8; 3], expected: [u8; 3]) -> bool {
    actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 1)
}

#[test]
fn the_flow_view_shows_a_painted_vector_and_still_water_in_both_views() {
    let Some(mut gpu) = gpu() else { return };
    let mut session = flow_session();
    session.stroke_uv_points(&stroke_points()).unwrap();

    for (name, painted, still) in [
        ("2D", pixel_2d(&mut gpu, &mut session, PAINTED_UV), pixel_2d(&mut gpu, &mut session, UNPAINTED_UV)),
        ("3D", pixel_3d(&mut gpu, &mut session, PAINTED_UV), pixel_3d(&mut gpu, &mut session, UNPAINTED_UV)),
    ] {
        assert!(shows(painted, RIGHT), "{name} view, in the stroke: {painted:?}");
        assert!(shows(still, STILL), "{name} view, away from the stroke: {still:?}");
    }
}

#[test]
fn the_flow_view_shows_a_filled_vector_in_both_views() {
    let Some(mut gpu) = gpu() else { return };
    let mut session = flow_session();
    execute(&mut session, "layer.add.fill", &Value::Null).unwrap();

    // 90 degrees points up the image at half speed: green falls half its reach, red stays still
    execute(&mut session, "layer.fill.set", &json!({"channel": "flow", "angle": 90.0, "speed": 0.5})).unwrap();

    let up_at_half_speed = [128, 64, 0];
    let (flat, lit) = (pixel_2d(&mut gpu, &mut session, PAINTED_UV), pixel_3d(&mut gpu, &mut session, PAINTED_UV));
    assert!(shows(flat, up_at_half_speed), "2D view: {flat:?}");
    assert!(shows(lit, up_at_half_speed), "3D view: {lit:?}");
}

#[test]
fn the_flow_view_follows_a_stroke_before_it_ends() {
    let Some(mut gpu) = gpu() else { return };
    let mut session = flow_session();
    assert!(shows(pixel_2d(&mut gpu, &mut session, PAINTED_UV), STILL), "nothing painted yet");

    let [from, to] = stroke_points();
    session.begin_stroke_uv(from, 1.0).unwrap();
    session.continue_stroke_uv(to, 1.0);
    assert!(session.stroke_active());

    let (flat, lit) = (pixel_2d(&mut gpu, &mut session, PAINTED_UV), pixel_3d(&mut gpu, &mut session, PAINTED_UV));
    assert!(session.stroke_active(), "the stroke is still in progress");
    assert!(shows(flat, RIGHT), "2D view during the stroke: {flat:?}");
    assert!(shows(lit, RIGHT), "3D view during the stroke: {lit:?}");

    // An abandoned stroke leaves nothing behind
    session.cancel_stroke();
    let abandoned = pixel_2d(&mut gpu, &mut session, PAINTED_UV);
    assert!(shows(abandoned, STILL), "{abandoned:?}");
}

#[test]
fn undo_and_removing_the_channel_clear_the_flow_view() {
    let Some(mut gpu) = gpu() else { return };
    let mut session = flow_session();
    session.stroke_uv_points(&stroke_points()).unwrap();
    assert!(shows(pixel_2d(&mut gpu, &mut session, PAINTED_UV), RIGHT));

    execute(&mut session, "edit.undo", &Value::Null).unwrap();
    let undone = pixel_2d(&mut gpu, &mut session, PAINTED_UV);
    assert!(shows(undone, STILL), "after undo: {undone:?}");

    execute(&mut session, "edit.redo", &Value::Null).unwrap();
    assert!(shows(pixel_2d(&mut gpu, &mut session, PAINTED_UV), RIGHT));

    // The view stays on the channel, which the set no longer has: nothing flows
    execute(&mut session, "textureSet.channel.remove", &json!({"channel": "flow"})).unwrap();
    let (flat, lit) = (pixel_2d(&mut gpu, &mut session, PAINTED_UV), pixel_3d(&mut gpu, &mut session, PAINTED_UV));
    assert!(shows(flat, STILL), "2D view without the channel: {flat:?}");
    assert!(shows(lit, STILL), "3D view without the channel: {lit:?}");
}
