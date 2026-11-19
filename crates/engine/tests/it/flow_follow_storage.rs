//! What paths and saved brushes keep of the flow map brush's follow switch: the mode they were
//! made in when they paint the flow map, and nothing of it when they do not

use serde_json::{Value, json};
use texturecraft_engine::core::{Channel, FLOW_STILL, LayerId};
use texturecraft_engine::geom::glam::Vec2;
use texturecraft_engine::geom::primitives;
use texturecraft_engine::{Session, execute};

use super::flow_follow::{RED, follow_session, near, vector_at};
use super::flow_map::{RESOLUTION, close, flow_at, flow_session};

/// The stored settings of path `index` of path sublayer `effect`
fn stored_paint(session: &Session, effect: u64, index: usize) -> Value {
    session.paths(LayerId(effect)).unwrap()[index].paint.clone()
}

#[test]
fn a_path_follows_its_curve_and_survives_save_reopen_edit_and_undo() {
    let mut session = follow_session();
    execute(&mut session, "tool.brush", &json!({"size": 8, "hardness": 100})).unwrap();
    let committed = execute(&mut session, "tool.path", &json!({"uvPoints": [[0.2, 0.3], [0.8, 0.3]], "commit": true})).unwrap();
    let effect = committed["committed"]["effect"].as_u64().unwrap();
    session.update();

    assert!(near(vector_at(&session, 0.5, 0.3), Vec2::X), "{}", vector_at(&session, 0.5, 0.3));
    assert_eq!(stored_paint(&session, effect, 0)["flow_follow"], json!(true), "the path keeps the mode it was painted in");

    // Saved and reopened, with the tool meanwhile painting a fixed direction
    let bytes = texturecraft_engine::formats::save_project_bytes(&session.project_file()).unwrap();
    let reopened = texturecraft_engine::formats::load_project_bytes(&bytes).unwrap();
    session.doc.project = reopened.project;
    assert_eq!(stored_paint(&session, effect, 0)["flow_follow"], json!(true), "the file kept it");
    execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": 180.0})).unwrap();
    assert!(!session.tool.flow_follow);

    execute(&mut session, "path.edit", &json!({"effect": effect, "index": 0})).unwrap();
    assert!(session.tool.flow_follow, "the path's own mode is back in the tool");
    assert!(!session.path_live().unwrap(), "a loaded path matches its settings");

    // Edited to run towards larger V: it repaints at once, following its new course
    let steps = session.history_labels().len();
    execute(&mut session, "tool.path", &json!({"uvPoints": [[0.3, 0.2], [0.3, 0.8]]})).unwrap();
    assert!(near(vector_at(&session, 0.3, 0.5), Vec2::Y), "{}", vector_at(&session, 0.3, 0.5));
    assert!(close(flow_at(&session, 0.6, 0.3), FLOW_STILL), "the old course is gone");
    execute(&mut session, "tool.path", &json!({"commit": true})).unwrap();
    assert_eq!(session.history_labels().len(), steps + 1, "one step for the edit");

    execute(&mut session, "edit.undo", &Value::Null).unwrap();
    session.update();
    assert!(near(vector_at(&session, 0.5, 0.3), Vec2::X), "undo brings the first course back");
    assert!(close(flow_at(&session, 0.3, 0.6), FLOW_STILL));
}

#[test]
fn a_path_names_the_follow_mode_only_when_it_paints_the_flow_map() {
    // The flow map is off in the brush: nothing of it is stored
    let mut session = flow_session();
    execute(&mut session, "tool.material", &json!({"channel": "baseColor", "color": RED})).unwrap();
    let committed = execute(&mut session, "tool.path", &json!({"uvPoints": [[0.2, 0.8], [0.8, 0.8]], "commit": true})).unwrap();
    let plain = stored_paint(&session, committed["committed"]["effect"].as_u64().unwrap(), 0).to_string();
    assert!(!plain.contains("flow_follow") && !plain.contains("Flow"), "{plain}");

    // The flow map is on in the brush, but the texture set has no flow map to paint
    let mut session = Session::new_project("Flow", primitives::plane(2.0, 4), "builtin:plane", None, RESOLUTION).unwrap();
    execute(&mut session, "tool.material", &json!({"channel": "flow", "enabled": true})).unwrap();
    let committed = execute(&mut session, "tool.path", &json!({"uvPoints": [[0.2, 0.8], [0.8, 0.8]], "commit": true})).unwrap();
    let no_channel = stored_paint(&session, committed["committed"]["effect"].as_u64().unwrap(), 0).to_string();
    assert!(!no_channel.contains("flow_follow"), "{no_channel}");

    // Painted with a fixed direction: the path says so
    let mut session = follow_session();
    execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": 90.0})).unwrap();
    let committed = execute(&mut session, "tool.path", &json!({"uvPoints": [[0.2, 0.8], [0.8, 0.8]], "commit": true})).unwrap();
    let effect = committed["committed"]["effect"].as_u64().unwrap();
    session.update();
    assert_eq!(stored_paint(&session, effect, 0)["flow_follow"], json!(false));
    assert!(near(vector_at(&session, 0.5, 0.8), Vec2::NEG_Y), "{}", vector_at(&session, 0.5, 0.8));
}

/// `value` without any `flow_follow` key, as a build from before strokes could follow wrote it
fn without_follow(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("flow_follow");
            map.values_mut().for_each(without_follow);
        }
        Value::Array(items) => items.iter_mut().for_each(without_follow),
        _ => {}
    }
}

#[test]
fn a_flow_path_stored_before_strokes_could_follow_keeps_its_fixed_direction() {
    let mut session = follow_session();
    execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": 90.0})).unwrap();
    let committed = execute(&mut session, "tool.path", &json!({"uvPoints": [[0.2, 0.8], [0.8, 0.8]], "commit": true})).unwrap();
    let effect = committed["committed"]["effect"].as_u64().unwrap();
    let mut project = serde_json::to_value(&session.doc.project).unwrap();
    without_follow(&mut project);
    session.doc.project = serde_json::from_value(project).unwrap();
    assert_eq!(stored_paint(&session, effect, 0).get("flow_follow"), None);

    // The tool follows strokes; loading the old path must not make the path follow
    execute(&mut session, "tool.material", &json!({"channel": "flow", "follow": true})).unwrap();
    execute(&mut session, "path.edit", &json!({"effect": effect, "index": 0})).unwrap();
    assert!(!session.tool.flow_follow);
    execute(&mut session, "tool.path", &json!({"uvPoints": [[0.2, 0.6], [0.8, 0.6]], "commit": true})).unwrap();
    session.update();

    assert!(near(vector_at(&session, 0.5, 0.6), Vec2::NEG_Y), "moved, it still paints its fixed direction: {}", vector_at(&session, 0.5, 0.6));
}

#[test]
fn a_saved_brush_names_the_follow_mode_only_when_it_paints_the_flow_map() {
    let directory = std::env::temp_dir().join(format!("tc-flow-brush-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    let mut session = follow_session();
    session.load_brush_presets(directory.clone());
    let file = |name: &str| std::fs::read_to_string(directory.join(format!("{name}.tcbrush"))).unwrap();

    execute(&mut session, "brush.preset.store", &json!({"name": "Follows"})).unwrap();
    execute(&mut session, "tool.material", &json!({"channel": "flow", "follow": false})).unwrap();
    execute(&mut session, "brush.preset.store", &json!({"name": "Fixed"})).unwrap();
    execute(&mut session, "tool.material", &json!({"channel": "baseColor", "color": RED, "solo": true})).unwrap();
    execute(&mut session, "brush.preset.store", &json!({"name": "Plain"})).unwrap();

    let stored = |name: &str| serde_json::from_str::<Value>(&file(name)).unwrap();
    assert_eq!(stored("Follows")["flow_follow"], json!(true));
    assert_eq!(stored("Fixed")["flow_follow"], json!(false));
    assert!(!file("Plain").contains("flow_follow") && !file("Plain").contains("Flow"), "{}", file("Plain"));

    // A brush that paints the flow map brings its mode back; one that does not leaves it alone
    execute(&mut session, "brush.preset.apply", &json!({"preset": "Follows"})).unwrap();
    assert!(session.tool.flow_follow && session.tool.enabled.contains(&Channel::Flow));
    execute(&mut session, "brush.preset.apply", &json!({"preset": "Plain"})).unwrap();
    assert!(session.tool.flow_follow && !session.tool.enabled.contains(&Channel::Flow));
    execute(&mut session, "brush.preset.apply", &json!({"preset": "Fixed"})).unwrap();
    assert!(!session.tool.flow_follow);

    let _ = std::fs::remove_dir_all(directory);
}
