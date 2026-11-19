//! The flow map channel driven through engine commands: adding it, painting and filling a fixed
//! direction, the rules that keep the vector intact, and what a path stores of it

use serde_json::{Value, json};
use texturecraft_engine::core::{Channel, FLOW_STILL, LayerId, flow_from_polar, flow_to_polar};
use texturecraft_engine::geom::primitives;
use texturecraft_engine::{Session, execute};

pub(super) const RESOLUTION: u32 = 256;

/// Stored as 16 bits: one step of the packed value
pub(super) const STORAGE_STEP: f32 = 1.0 / 65535.0;

/// A plane project (its UVs fill the texture) whose set has the flow map
pub(super) fn flow_session() -> Session {
    let mut session = Session::new_project("Flow", primitives::plane(2.0, 4), "builtin:plane", None, RESOLUTION).unwrap();
    execute(&mut session, "textureSet.channel.add", &json!({"channel": "flow"})).unwrap();
    execute(&mut session, "tool.paint", &Value::Null).unwrap();
    session
}

/// The composited flow map value at (`u`, `v`)
pub(super) fn flow_at(session: &Session, u: f32, v: f32) -> [f32; 3] {
    session.caches[0].composite.value(Channel::Flow, (u * RESOLUTION as f32) as u32, (v * RESOLUTION as f32) as u32)
}

pub(super) fn close(actual: [f32; 3], expected: [f32; 3]) -> bool {
    actual.iter().zip(expected).all(|(a, e)| (a - e).abs() <= 2.0 * STORAGE_STEP)
}

/// The smaller turn between two directions: 359.9 and 0 are 0.1 apart
pub(super) fn degrees_apart(a: f32, b: f32) -> f32 {
    let turn = (a - b).rem_euclid(360.0);
    turn.min(360.0 - turn)
}

/// The channels a layer has enabled, by layer kind, from `layer.list`
fn layers_with_flow(session: &mut Session) -> Vec<(String, bool)> {
    let layers = execute(session, "layer.list", &Value::Null).unwrap();
    layers.as_array().unwrap().iter().map(|layer| (layer["kind"].as_str().unwrap().to_string(), layer["channels"].get("flow").is_some())).collect()
}

/// Paint one horizontal stroke through the middle of the texture with the tool's material
fn stroke_across(session: &mut Session) {
    execute(session, "tool.brush", &json!({"size": 10, "hardness": 100})).unwrap();
    execute(session, "tool.strokeUv", &json!({"points": [[0.3, 0.5], [0.7, 0.5]]})).unwrap();
}

#[test]
fn the_flow_map_is_added_on_request_and_starts_still() {
    let mut session = Session::new_project("Flow", primitives::plane(2.0, 4), "builtin:plane", None, RESOLUTION).unwrap();
    assert!(!session.doc.project.texture_sets[0].has_channel(Channel::Flow), "not part of a new set");

    execute(&mut session, "textureSet.channel.add", &json!({"channel": "flow"})).unwrap();

    assert!(session.doc.project.texture_sets[0].has_channel(Channel::Flow));
    assert_eq!(session.doc.project.texture_sets[0].channel_list().last(), Some(&Channel::Flow));
    assert!(close(flow_at(&session, 0.5, 0.5), FLOW_STILL));
}

#[test]
fn fills_stay_out_of_the_flow_map_until_given_a_direction() {
    let mut session = Session::new_project("Flow", primitives::plane(2.0, 4), "builtin:plane", None, RESOLUTION).unwrap();
    execute(&mut session, "layer.add.fill", &json!({"color": [0.2, 0.4, 0.6]})).unwrap();

    execute(&mut session, "textureSet.channel.add", &json!({"channel": "flow"})).unwrap();
    assert_eq!(layers_with_flow(&mut session), [("fill".to_string(), false), ("paint".to_string(), true)], "top layer first");

    // Layers added afterwards follow the same rule
    execute(&mut session, "layer.add.fill", &json!({"color": [0.9, 0.1, 0.1]})).unwrap();
    execute(&mut session, "layer.add.paint", &Value::Null).unwrap();
    let kinds = layers_with_flow(&mut session);
    assert!(kinds.iter().all(|(kind, has_flow)| *has_flow == (kind == "paint")), "{kinds:?}");

    // A fill that is given a direction takes the channel and sets it everywhere
    let fill = session.doc.project.texture_sets[0].stack.iter().copied().find(|id| layer_kind(&session, *id) == "fill").unwrap();
    execute(&mut session, "layer.fill.set", &json!({"layer": fill.0, "channel": "flow", "angle": 0.0, "speed": 1.0})).unwrap();
    session.update();

    assert!(session.doc.project.layer(fill).unwrap().channels.contains_key(&Channel::Flow));
    assert!(close(flow_at(&session, 0.1, 0.9), [1.0, 0.5, 0.0]), "{:?}", flow_at(&session, 0.1, 0.9));
}

fn layer_kind(session: &Session, id: LayerId) -> &'static str {
    session.doc.project.layer(id).map_or("", |layer| layer.kind.name())
}

#[test]
fn a_fixed_direction_paints_the_vector_and_leaves_the_rest_still() {
    let mut session = flow_session();
    let tool = execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": 90.0, "speed": 0.5, "solo": true})).unwrap();
    assert_eq!(tool["enabled"], json!(["flow"]));

    stroke_across(&mut session);

    // 90 degrees points up the image: towards smaller V
    let painted = flow_at(&session, 0.5, 0.5);
    assert!(close(painted, [0.5, 0.25, 0.0]), "{painted:?}");
    assert!(close(painted, flow_from_polar(90.0, 0.5)));
    assert!(close(flow_at(&session, 0.5, 0.9), FLOW_STILL), "away from the stroke nothing moves");

    // The other channels were off: the stroke did not touch them
    assert!(close(session.caches[0].composite.value(Channel::BaseColor, RESOLUTION / 2, RESOLUTION / 2), Channel::BaseColor.default_value()));
}

#[test]
fn one_of_direction_and_speed_keeps_the_other() {
    let mut session = flow_session();
    execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": 135.0, "speed": 0.4})).unwrap();

    execute(&mut session, "tool.material", &json!({"channel": "flow", "speed": 0.8})).unwrap();
    let (angle, speed) = flow_to_polar(session.tool.values[&Channel::Flow]);
    assert!((angle - 135.0).abs() < 1e-2 && (speed - 0.8).abs() < 1e-5, "{angle} {speed}");

    execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": 270.0})).unwrap();
    let (angle, speed) = flow_to_polar(session.tool.values[&Channel::Flow]);
    assert!((angle - 270.0).abs() < 1e-2 && (speed - 0.8).abs() < 1e-5, "{angle} {speed}");
}

#[test]
fn a_direct_value_keeps_the_third_component_empty() {
    let mut session = flow_session();

    execute(&mut session, "tool.material", &json!({"channel": "flow", "value": [0.8, 0.3, 0.7]})).unwrap();

    // The vector is kept on the levels it is stored as: within a level of what was given
    let stored = session.tool.values[&Channel::Flow];
    assert!(close(stored, [0.8, 0.3, 0.0]), "{stored:?}");
    assert_eq!(stored[2], 0.0);
}

#[test]
fn a_direction_on_another_channel_is_refused_and_changes_nothing() {
    let mut session = flow_session();
    let (values, enabled, history) = (session.tool.values.clone(), session.tool.enabled.clone(), session.history_labels());

    for params in [json!({"channel": "roughness", "angle": 30.0}), json!({"channel": "baseColor", "speed": 0.5, "color": [1, 0, 0]})] {
        let error = execute(&mut session, "tool.material", &params).unwrap_err().to_string();
        assert!(error.contains("flow"), "{error}");
    }
    let fill = execute(&mut session, "layer.add.fill", &json!({"color": [0.5, 0.5, 0.5]})).unwrap()["layer"].as_u64().unwrap();
    let before = session.doc.project.clone();
    assert!(execute(&mut session, "layer.fill.set", &json!({"layer": fill, "channel": "roughness", "angle": 30.0})).is_err());

    assert_eq!((&session.tool.values, &session.tool.enabled), (&values, &enabled));
    assert_eq!(session.doc.project, before);
    assert_eq!(session.history_labels().len(), history.len() + 1, "only the fill layer was added");
}

#[test]
fn hostile_directions_paint_still_water_or_are_refused() {
    let mut session = flow_session();

    // Numbers JSON cannot carry (NaN, infinity) arrive as null and count as absent
    let untouched = session.tool.values[&Channel::Flow];
    execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": null, "speed": null})).unwrap();
    assert_eq!(session.tool.values[&Channel::Flow], untouched);

    // A direction that is there but is not a number is refused, not skipped
    for params in [json!({"channel": "flow", "angle": "east"}), json!({"channel": "flow", "angle": [1, 2], "speed": 0.5})] {
        let error = execute(&mut session, "tool.material", &params).unwrap_err().to_string();
        assert!(error.contains("`angle`"), "{params}: {error}");
        assert_eq!(session.tool.values[&Channel::Flow], untouched, "{params}");
    }

    // Out of range values are clamped or wrapped, never stored raw
    execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": 1e30, "speed": -4.0})).unwrap();
    assert!(close(session.tool.values[&Channel::Flow], FLOW_STILL));
    execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": 720.0, "speed": 50.0})).unwrap();
    assert!(close(session.tool.values[&Channel::Flow], [1.0, 0.5, 0.0]));
}

#[test]
fn a_blend_mode_set_on_the_flow_map_cannot_bend_it() {
    let mut session = flow_session();
    execute(&mut session, "layer.add.fill", &json!({})).unwrap();
    execute(&mut session, "layer.fill.set", &json!({"channel": "flow", "angle": 0.0, "speed": 0.5, "solo": true})).unwrap();
    execute(&mut session, "layer.add.paint", &Value::Null).unwrap();
    execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": 90.0, "speed": 1.0, "solo": true})).unwrap();
    stroke_across(&mut session);
    let normal = flow_at(&session, 0.5, 0.5);
    assert!(close(normal, [0.5, 0.0, 0.0]), "{normal:?}");

    // Modes that add, multiply or renormalise the packed value would all change it
    for mode in ["Linear dodge (Add)", "Multiply", "Normal map detail", "Difference"] {
        execute(&mut session, "layer.blend", &json!({"channel": "flow", "mode": mode})).unwrap();
        session.update();

        assert!(close(flow_at(&session, 0.5, 0.5), normal), "{mode}: {:?}", flow_at(&session, 0.5, 0.5));
        assert!(close(flow_at(&session, 0.5, 0.9), [0.75, 0.5, 0.0]), "{mode}: the fill below is untouched");
    }
}

#[test]
fn layer_opacity_blends_the_flow_towards_what_is_below() {
    let mut session = flow_session();
    execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": 0.0, "speed": 1.0, "solo": true})).unwrap();
    stroke_across(&mut session);

    execute(&mut session, "layer.blend", &json!({"channel": "flow", "opacity": 0.5})).unwrap();
    session.update();

    // Half way between still water below and full speed to the right
    let (angle, speed) = flow_to_polar(flow_at(&session, 0.5, 0.5));
    assert!(degrees_apart(angle, 0.0) < 0.1 && (speed - 0.5).abs() < 1e-3, "{angle} {speed}");
}

#[test]
fn glow_and_bevel_leave_the_flow_map_as_it_is_while_blur_relaxes_it() {
    let mut session = flow_session();
    execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": 0.0, "speed": 1.0, "solo": true})).unwrap();
    stroke_across(&mut session);
    let painted = flow_at(&session, 0.5, 0.5);

    for filter in ["glow", "bevel"] {
        let effect = execute(&mut session, "effect.add.filter", &json!({"target": "content", "filter": filter})).unwrap()["effect"].as_u64().unwrap();
        session.update();
        assert!(close(flow_at(&session, 0.5, 0.5), painted), "{filter}: {:?}", flow_at(&session, 0.5, 0.5));
        execute(&mut session, "effect.remove", &json!({"effect": effect})).unwrap();
    }

    // Blur mixes the stroke's edge with the still water beside it: slower, same direction
    execute(&mut session, "effect.add.filter", &json!({"target": "content", "filter": "blur", "radius": 12})).unwrap();
    session.update();
    let edge = flow_to_polar(flow_at(&session, 0.5, 0.5 + 9.0 / RESOLUTION as f32));
    assert!(edge.1 > 0.05 && edge.1 < 0.95 && degrees_apart(edge.0, 0.0) < 1.0, "{edge:?}");
}

#[test]
fn a_path_that_does_not_paint_the_flow_map_does_not_name_it() {
    let mut session = flow_session();

    // The tool keeps a direction for later, with the channel off
    execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": 45.0, "speed": 0.5, "enabled": false})).unwrap();
    let kept = session.tool.values[&Channel::Flow];
    execute(&mut session, "tool.material", &json!({"channel": "baseColor", "color": [1.0, 0.0, 0.0]})).unwrap();
    let committed = execute(&mut session, "tool.path", &json!({"uvPoints": [[0.2, 0.8], [0.8, 0.8]], "commit": true})).unwrap();
    let effect = committed["committed"]["effect"].as_u64().unwrap();

    // A build without the channel reads this path: nothing in it names the channel
    let stored = serde_json::to_string(&session.paths(LayerId(effect)).unwrap()[0].paint).unwrap();
    assert!(!stored.contains("Flow"), "{stored}");
    assert!(stored.contains("BaseColor"));

    // Loading the path for editing keeps the tool's direction and repaints nothing
    execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": 200.0, "speed": 1.0, "enabled": false})).unwrap();
    let before_edit = session.tool.values[&Channel::Flow];
    execute(&mut session, "path.edit", &json!({"effect": effect, "index": 0})).unwrap();
    assert_eq!(session.tool.values[&Channel::Flow], before_edit);
    assert_ne!(before_edit, kept);
    assert!(!session.path_live().unwrap(), "a loaded path matches its settings");
}

#[test]
fn a_path_that_paints_the_flow_map_keeps_its_direction() {
    let mut session = flow_session();
    execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": 180.0, "speed": 1.0, "solo": true})).unwrap();
    execute(&mut session, "tool.brush", &json!({"size": 8, "hardness": 100})).unwrap();
    let committed = execute(&mut session, "tool.path", &json!({"uvPoints": [[0.2, 0.3], [0.8, 0.3]], "commit": true})).unwrap();
    let effect = committed["committed"]["effect"].as_u64().unwrap();
    session.update();
    assert!(close(flow_at(&session, 0.5, 0.3), [0.0, 0.5, 0.0]), "{:?}", flow_at(&session, 0.5, 0.3));

    // Another direction in the tool does not leak into the stored path
    execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": 0.0, "speed": 1.0})).unwrap();
    let saved = serde_json::to_string(&session.doc.project).unwrap();
    session.doc.project = serde_json::from_str(&saved).unwrap();
    execute(&mut session, "path.edit", &json!({"effect": effect, "index": 0})).unwrap();

    assert!(close(session.tool.values[&Channel::Flow], [0.0, 0.5, 0.0]), "the path's own direction is back in the tool");
    assert!(!session.path_live().unwrap());
}

#[test]
fn a_project_with_a_painted_flow_map_saves_and_reopens() {
    let mut session = flow_session();
    execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": 300.0, "speed": 0.75, "solo": true})).unwrap();
    stroke_across(&mut session);
    let painted = flow_at(&session, 0.5, 0.5);
    let layer = session.selected_layer().unwrap();

    let bytes = texturecraft_engine::formats::save_project_bytes(&session.project_file()).unwrap();
    let reopened = texturecraft_engine::formats::load_project_bytes(&bytes).unwrap();

    assert_eq!(reopened.project, session.doc.project);
    assert!(reopened.project.texture_sets[0].has_channel(Channel::Flow));
    let stored = reopened.pixels.layers.get(&(layer, Channel::Flow)).expect("the painted flow tiles are in the file");
    assert_eq!(stored.get(RESOLUTION / 2, RESOLUTION / 2), session.doc.pixels.layers[&(layer, Channel::Flow)].get(RESOLUTION / 2, RESOLUTION / 2));
    assert!(close(painted, flow_from_polar(300.0, 0.75)));
}

#[test]
fn removing_the_flow_map_takes_it_out_of_the_set() {
    let mut session = flow_session();

    execute(&mut session, "textureSet.channel.remove", &json!({"channel": "flow"})).unwrap();

    assert!(!session.doc.project.texture_sets[0].has_channel(Channel::Flow));
    execute(&mut session, "edit.undo", &Value::Null).unwrap();
    assert!(session.doc.project.texture_sets[0].has_channel(Channel::Flow));
}
