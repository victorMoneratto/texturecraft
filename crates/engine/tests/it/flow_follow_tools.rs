//! The follow switch of the flow map brush: how commands set and report it, and how the tools
//! that do not follow a stroke treat the flow map

use serde_json::{Value, json};
use texturecraft_engine::core::{Channel, FLOW_STILL, flow_from_polar};
use texturecraft_engine::flow_stroke::follows_stroke;
use texturecraft_engine::geom::glam::Vec2;
use texturecraft_engine::{Session, execute};

use super::flow_follow::{COMMIT_ROUNDING, RED, base_color_at, follow_session, largest_difference, look_down, near, screen, snapshot, stroke_uv, vector_at};
use super::flow_map::{RESOLUTION, close, flow_at, flow_session};

#[test]
fn the_tool_and_inspect_report_whether_strokes_follow() {
    let mut session = flow_session();
    let state = execute(&mut session, "tool.state", &Value::Null).unwrap();
    let inspect = execute(&mut session, "project.inspect", &Value::Null).unwrap();
    assert_eq!(state["flow_follow"], json!(true), "the default");
    assert_eq!(inspect["tool"]["flow"], json!({"follow": true, "angle": 0.0, "speed": 1.0, "value": flow_from_polar(0.0, 1.0)}));
    assert_eq!(state["flow"], inspect["tool"]["flow"], "the tool state says the same");

    let reply = execute(&mut session, "tool.material", &json!({"channel": "flow", "follow": false, "speed": 0.25})).unwrap();

    assert_eq!(reply["flow"]["follow"], json!(false));
    assert!((reply["flow"]["speed"].as_f64().unwrap() - 0.25).abs() < 1e-6);
    assert_eq!(execute(&mut session, "tool.state", &Value::Null).unwrap()["flow_follow"], json!(false));
    assert_eq!(execute(&mut session, "project.inspect", &Value::Null).unwrap()["tool"]["flow"]["follow"], json!(false));

    // A direction given with follow keeps strokes following; it waits for when they stop
    let reply = execute(&mut session, "tool.material", &json!({"channel": "flow", "follow": true, "angle": 270.0})).unwrap();
    assert_eq!(reply["flow"]["follow"], json!(true));
    assert!((reply["flow"]["angle"].as_f64().unwrap() - 270.0).abs() < 1e-2);
}

#[test]
fn a_malformed_or_misplaced_follow_is_refused_and_changes_nothing() {
    let mut session = flow_session();
    let (tool, revision) = (session.tool.clone(), session.revision);

    let refused = [
        json!({"channel": "flow", "follow": "yes"}),
        json!({"channel": "flow", "follow": 1}),
        json!({"channel": "flow", "follow": [true]}),
        json!({"channel": "flow", "follow": {"on": true}}),
        json!({"channel": "roughness", "follow": true}),
        json!({"follow": false}),
        // Everything else in the request is fine: none of it may be applied
        json!({"channel": "flow", "follow": "no", "angle": 45.0, "speed": 0.5, "enabled": false, "material": "copper", "maskValue": 0.2}),
    ];
    for params in refused {
        let error = execute(&mut session, "tool.material", &params).unwrap_err().to_string();
        assert!(error.contains("follow"), "{params}: {error}");
        assert!(session.tool == tool, "{params} changed the tool");
        assert_eq!(session.revision, revision, "{params} counted as a change");
    }

    // A follow that JSON could not carry arrives as null and counts as absent
    execute(&mut session, "tool.material", &json!({"channel": "flow", "follow": null})).unwrap();
    assert!(session.tool.flow_follow);
}

#[test]
fn fills_paint_the_fixed_direction_while_strokes_follow() {
    let mut session = follow_session();
    execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": 90.0, "follow": true})).unwrap();
    assert!(follows_stroke(&session.tool), "the paint tool follows");

    // A polygon fill has no stroke to follow
    execute(&mut session, "tool.polygonFill", &Value::Null).unwrap();
    assert!(!follows_stroke(&session.tool));
    execute(&mut session, "tool.click", &json!({"uv": [0.2, 0.2]})).unwrap();
    assert!(near(vector_at(&session, 0.2, 0.2), Vec2::NEG_Y), "{}", vector_at(&session, 0.2, 0.2));

    // Nor has a filled path; the same path stroked follows its outline
    let square = json!([[0.6, 0.6], [0.9, 0.6], [0.9, 0.9], [0.6, 0.9]]);
    execute(&mut session, "tool.brush", &json!({"size": 4, "hardness": 100})).unwrap();
    execute(&mut session, "tool.path", &json!({"uvPoints": square, "corners": [true, true, true, true], "closed": true, "fill": true})).unwrap();
    assert!(!follows_stroke(&session.tool));
    execute(&mut session, "tool.path", &json!({"commit": true})).unwrap();
    session.update();
    assert!(near(vector_at(&session, 0.75, 0.75), Vec2::NEG_Y), "{}", vector_at(&session, 0.75, 0.75));

    execute(&mut session, "tool.path", &json!({"uvPoints": square, "corners": [true, true, true, true], "closed": true, "fill": false})).unwrap();
    assert!(follows_stroke(&session.tool));
    execute(&mut session, "tool.path", &json!({"commit": true})).unwrap();
    session.update();
    assert!(near(vector_at(&session, 0.75, 0.6), Vec2::X), "along the top edge: {}", vector_at(&session, 0.75, 0.6));
    assert!(near(vector_at(&session, 0.9, 0.75), Vec2::Y), "down the right edge: {}", vector_at(&session, 0.9, 0.75));
}

#[test]
fn the_eraser_takes_flow_away_and_smudge_carries_it() {
    let mut session = follow_session();
    stroke_uv(&mut session, &[[0.2, 0.5], [0.8, 0.5]]);

    // Erasing across the stroke leaves what is below, still water: it paints no direction of its own
    execute(&mut session, "tool.eraser", &Value::Null).unwrap();
    assert!(!follows_stroke(&session.tool));
    stroke_uv(&mut session, &[[0.5, 0.35], [0.5, 0.65]]);
    assert!(close(flow_at(&session, 0.5, 0.5), FLOW_STILL), "{:?}", flow_at(&session, 0.5, 0.5));
    assert!(close(flow_at(&session, 0.5, 0.4), FLOW_STILL), "the eraser's own way is not painted");
    assert!(near(vector_at(&session, 0.3, 0.5), Vec2::X));

    // Smudging up out of the stroke drags its flow along: the smudge's own way is not painted either
    execute(&mut session, "tool.smudge", &Value::Null).unwrap();
    assert!(!follows_stroke(&session.tool));
    execute(&mut session, "tool.brush", &json!({"smudgeStrength": 100, "spacing": 5})).unwrap();
    stroke_uv(&mut session, &[[0.3, 0.5], [0.3, 0.38]]);
    let dragged = vector_at(&session, 0.3, 0.42);
    assert!(dragged.x > 0.3 && dragged.y.abs() < 0.05, "{dragged}");
}

#[test]
fn a_brush_material_keeps_its_pattern_while_the_flow_map_follows() {
    let mut session = flow_session();
    execute(&mut session, "tool.material", &json!({"material": "copper"})).unwrap();
    execute(&mut session, "tool.material", &json!({"channel": "flow", "enabled": true})).unwrap();
    execute(&mut session, "tool.brush", &json!({"size": 10, "hardness": 100})).unwrap();
    assert!(session.tool.material.is_some() && session.tool.flow_follow);
    let metallic = |session: &Session, v: f32| session.caches[0].composite.value(Channel::Metallic, RESOLUTION / 2, (v * RESOLUTION as f32) as u32)[0];

    stroke_uv(&mut session, &[[0.3, 0.5], [0.7, 0.5]]);
    assert!(metallic(&session, 0.5) > 0.9, "the material is painted: {}", metallic(&session, 0.5));
    assert!(near(vector_at(&session, 0.5, 0.5), Vec2::X), "with the stroke's direction: {}", vector_at(&session, 0.5, 0.5));

    // Not following, the fixed direction rides along with the material's pattern as before
    execute(&mut session, "tool.material", &json!({"channel": "flow", "follow": false})).unwrap();
    session.tool.values.insert(Channel::Flow, flow_from_polar(90.0, 1.0));
    stroke_uv(&mut session, &[[0.3, 0.8], [0.7, 0.8]]);
    assert!(metallic(&session, 0.8) > 0.9);
    assert!(near(vector_at(&session, 0.5, 0.8), Vec2::NEG_Y), "{}", vector_at(&session, 0.5, 0.8));
}

#[test]
fn the_material_picker_leaves_a_following_brush_its_speed() {
    let mut session = follow_session();
    look_down(&mut session);
    let over_still_water = screen(&session, 0.2, 0.2);
    let speed_one = session.tool.values[&Channel::Flow];

    // Picking a colour from still water would otherwise leave a brush that paints no flow
    execute(&mut session, "tool.colorPicker", &Value::Null).unwrap();
    execute(&mut session, "tool.click", &json!({"x": over_still_water.x, "y": over_still_water.y})).unwrap();
    assert_eq!(session.tool.values[&Channel::Flow], speed_one);

    // A brush with a fixed direction picks the flow like any other value
    execute(&mut session, "tool.material", &json!({"channel": "flow", "follow": false})).unwrap();
    execute(&mut session, "tool.click", &json!({"x": over_still_water.x, "y": over_still_water.y})).unwrap();
    assert!(close(session.tool.values[&Channel::Flow], FLOW_STILL));
}

#[test]
fn the_projection_stencil_shapes_the_flow_a_stroke_leaves() {
    let mut session = follow_session();
    execute(&mut session, "tool.material", &json!({"channel": "baseColor", "color": RED})).unwrap();
    look_down(&mut session);

    // A stencil that is solid on its left half and clear on its right
    let mut png = Vec::new();
    let image = image::RgbaImage::from_fn(16, 16, |x, _| image::Rgba([0, 255, 0, if x < 8 { 255 } else { 0 }]));
    image.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let name = session.import_resource_bytes("half.png", png).unwrap();
    execute(&mut session, "tool.projection", &Value::Null).unwrap();
    execute(&mut session, "tool.stencil", &json!({"image": name, "scale": 1.0, "tile": true})).unwrap();
    assert!(follows_stroke(&session.tool), "a projection stroke follows too");

    // One drag across the plane, kept live first
    let (from, to) = (screen(&session, -0.7, 0.0), screen(&session, 0.7, 0.0));
    session.begin_stroke(from, session.viewport, 1.0).unwrap();
    session.continue_stroke(to, 1.0);
    session.update();
    let live = snapshot(&session);
    session.end_stroke().unwrap();
    session.update();
    let difference = largest_difference(&live, &snapshot(&session));
    assert!(difference <= COMMIT_ROUNDING, "the live stroke differs from the stored one by {difference}");

    // Along the drag: where the stencil let the colour through the flow went through, and only there
    let (green, unpainted) = ([0.0, 1.0, 0.0], Channel::BaseColor.default_value());
    let (mut solid, mut clear) = (0, 0);
    for step in 0..40 {
        let u = 0.2 + 0.6 * step as f32 / 40.0;
        let (color, flow) = (base_color_at(&session, u, 0.5), vector_at(&session, u, 0.5));
        if close(color, green) {
            assert!(near(flow, Vec2::X), "u {u}: {flow}");
            solid += 1;
        } else if close(color, unpainted) {
            assert!(close(flow_at(&session, u, 0.5), FLOW_STILL), "u {u}: {flow}");
            clear += 1;
        }
    }
    assert!(solid > 5 && clear > 5, "the drag crosses both halves of the stencil: {solid} solid, {clear} clear");
}
