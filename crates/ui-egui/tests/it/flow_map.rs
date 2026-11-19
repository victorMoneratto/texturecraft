//! The flow map channel in the panels: where channels are listed, and its direction and speed
//! editors for the brush and for a fill layer

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{Queryable, by};
use serde_json::{Value, json};
use texturecraft_engine::core::{Channel, FLOW_STILL, LayerId, LayerKind, Source, flow_from_polar};
use texturecraft_engine::geom::primitives;
use texturecraft_engine::{Session, ViewMode, execute};
use texturecraft_ui_egui::TextureCraftApp;
use texturecraft_ui_egui::docking::Panel;
use texturecraft_ui_egui::state::PropsTab;

/// Frames for a click or a typed value to reach the session and the panels
const SETTLE_FRAMES: usize = 4;

/// Frames for a scroll to a control to finish before it is used
const SCROLL_FRAMES: usize = 30;

/// A plane project with the paint tool, without the flow map
fn plain_session() -> Session {
    let mut session = Session::new_project("Flow", primitives::plane(2.0, 4), "builtin:plane", None, 256).unwrap();
    execute(&mut session, "tool.paint", &Value::Null).unwrap();
    session
}

fn flow_session() -> Session {
    let mut session = plain_session();
    execute(&mut session, "textureSet.channel.add", &json!({"channel": "flow"})).unwrap();
    session
}

fn harness(session: Session) -> Harness<'static, TextureCraftApp> {
    let mut harness = Harness::builder().with_size(egui::vec2(1600.0, 1400.0)).build_eframe(|context| TextureCraftApp::new(context, session));

    // Use English so first-run locale does not change queried labels
    harness.state_mut().ui.prefs.language = "en".into();
    harness.run_steps(SETTLE_FRAMES);
    harness
}

/// The number field in the row of the slider labelled `label`
fn number_field<'a>(harness: &'a Harness<'_, TextureCraftApp>, label: &str) -> egui_kittest::Node<'a> {
    let row = harness.get_by_label(label).rect().center().y;
    harness.get_all(by().role(Role::TextInput)).find(|node| (node.rect().center().y - row).abs() < 4.0).unwrap()
}

fn field_text(harness: &Harness<'_, TextureCraftApp>, label: &str) -> String {
    number_field(harness, label).value().unwrap_or_default()
}

/// Replace what the number field of `label` holds with `text`, one key per frame like a person
fn type_value(harness: &mut Harness<'_, TextureCraftApp>, label: &str, text: &str) {
    number_field(harness, label).focus();
    harness.run_steps(2);
    harness.key_press(egui::Key::End);
    harness.run_steps(1);
    for _ in 0..8 {
        harness.key_press(egui::Key::Backspace);
        harness.run_steps(1);
    }
    for character in text.chars() {
        number_field(harness, label).type_text(&character.to_string());
        harness.run_steps(1);
    }
    harness.key_press(egui::Key::Enter);
    harness.run_steps(SETTLE_FRAMES);
}

fn close(actual: [f32; 3], expected: [f32; 3]) -> bool {
    actual.iter().zip(expected).all(|(a, e)| (a - e).abs() < 1e-4)
}

fn brush_flow(harness: &Harness<'_, TextureCraftApp>) -> [f32; 3] {
    harness.state().session.tool.values[&Channel::Flow]
}

#[test]
fn texture_set_settings_offer_the_flow_map_and_list_it_once_added() {
    let mut harness = harness(plain_session());
    harness.state_mut().ui.dock.focus(Panel::TextureSetSettings);
    harness.run_steps(SETTLE_FRAMES);
    assert!(harness.query_by_label("Flow map").is_none(), "the set does not have it yet");

    harness.get_by_label("Add channel").click();
    harness.run_steps(SETTLE_FRAMES);
    harness.get_by_role_and_label(Role::Button, "Flow map").click();
    harness.run_steps(SETTLE_FRAMES);

    assert!(harness.state().session.doc.project.texture_sets[0].has_channel(Channel::Flow));
    assert!(harness.query_by_label("Flow map").is_some(), "the set's channel list names it");
}

#[test]
fn the_view_picker_lists_the_flow_map_and_shows_it() {
    let mut harness = harness(flow_session());
    let pickers = |harness: &Harness<'_, TextureCraftApp>, shown: &str| {
        harness.get_all(by().role(Role::ComboBox)).filter(|node| node.value().as_deref() == Some(shown)).count()
    };
    assert!(pickers(&harness, "Material") >= 1, "a view starts on the material");

    let picker = harness.get_all(by().role(Role::ComboBox)).find(|node| node.value().as_deref() == Some("Material")).unwrap();
    picker.click();
    harness.run_steps(SETTLE_FRAMES);
    harness.get_by_label("Flow map").click();
    harness.run_steps(SETTLE_FRAMES);

    assert_eq!(harness.state().session.view, ViewMode::Channel(Channel::Flow));
    assert!(pickers(&harness, "Flow map") >= 1, "the picker names the view");
    assert_eq!(pickers(&harness, "Material"), 0);
}

#[test]
fn the_output_template_editor_offers_the_flow_map_as_an_input() {
    let mut harness = harness(flow_session());
    let app = harness.state_mut();
    app.ui.export.tab = 1;
    app.ui.windows.export = true;
    harness.run_steps(SETTLE_FRAMES);

    assert!(harness.query_by_label("Texture set channels").is_some(), "the input palette is open");
    assert!(harness.query_all_by_label("Flow map").next().is_some(), "the palette lists the flow map");
}

#[test]
fn the_brush_keeps_its_flow_direction_while_the_speed_is_zero() {
    let mut session = flow_session();
    execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": 120.0, "speed": 0.5, "solo": true})).unwrap();
    let mut harness = harness(session);
    harness.state_mut().ui.props_tab = PropsTab::Material;
    harness.run_steps(SETTLE_FRAMES);
    assert_eq!((field_text(&harness, "Flow direction").as_str(), field_text(&harness, "Flow speed").as_str()), ("120", "0.50"));

    type_value(&mut harness, "Flow speed", "0");

    assert_eq!(brush_flow(&harness), FLOW_STILL, "no speed paints still water");
    assert_eq!(field_text(&harness, "Flow direction"), "120", "the direction slider stays where it was put");

    // Turning it while still changes nothing painted, and is kept for when the speed comes back
    type_value(&mut harness, "Flow direction", "200");
    assert_eq!(brush_flow(&harness), FLOW_STILL);
    assert_eq!(field_text(&harness, "Flow direction"), "200");

    type_value(&mut harness, "Flow speed", "1");
    assert!(close(brush_flow(&harness), flow_from_polar(200.0, 1.0)), "{:?}", brush_flow(&harness));
    assert_eq!(field_text(&harness, "Flow direction"), "200");
}

#[test]
fn the_brush_shows_a_direction_set_by_a_command() {
    let mut session = flow_session();
    execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": 120.0, "speed": 0.5, "solo": true})).unwrap();
    let mut harness = harness(session);
    harness.state_mut().ui.props_tab = PropsTab::Material;
    harness.run_steps(SETTLE_FRAMES);
    assert_eq!(field_text(&harness, "Flow direction"), "120");

    execute(&mut harness.state_mut().session, "tool.material", &json!({"channel": "flow", "angle": 45.0})).unwrap();
    harness.run_steps(SETTLE_FRAMES);

    assert_eq!(field_text(&harness, "Flow direction"), "45", "the editor does not hold on to a direction the value left");
}

/// The uniform flow of fill layer `layer`
fn fill_flow(harness: &Harness<'_, TextureCraftApp>, layer: LayerId) -> [f32; 3] {
    let LayerKind::Fill(fill) = &harness.state().session.doc.project.layers[&layer].kind else { panic!("a fill layer") };
    let Source::Uniform(value) = fill.source(Channel::Flow) else { panic!("a uniform flow") };
    value
}

#[test]
fn a_fill_layer_sets_its_flow_by_direction_and_speed() {
    let mut session = flow_session();
    let layer = LayerId(execute(&mut session, "layer.add.fill", &Value::Null).unwrap()["layer"].as_u64().unwrap());
    execute(&mut session, "layer.fill.set", &json!({"layer": layer.0, "channel": "flow", "angle": 0.0, "speed": 1.0})).unwrap();
    let mut harness = harness(session);
    harness.get_by_label("Flow direction").scroll_to_me();
    harness.run_steps(SCROLL_FRAMES);
    assert_eq!((field_text(&harness, "Flow direction").as_str(), field_text(&harness, "Flow speed").as_str()), ("0", "1.00"));
    let steps = harness.state().session.history_labels().len();

    type_value(&mut harness, "Flow direction", "90");

    assert!(close(fill_flow(&harness, layer), flow_from_polar(90.0, 1.0)), "{:?}", fill_flow(&harness, layer));
    assert_eq!(harness.state().session.history_labels().len(), steps + 1, "one undo step, through the fill command");

    // No speed fills still water; the direction stays in the editor for when the speed comes back
    type_value(&mut harness, "Flow speed", "0");
    assert!(close(fill_flow(&harness, layer), FLOW_STILL), "{:?}", fill_flow(&harness, layer));
    assert_eq!(field_text(&harness, "Flow direction"), "90");

    type_value(&mut harness, "Flow speed", "0.5");
    assert!(close(fill_flow(&harness, layer), flow_from_polar(90.0, 0.5)), "{:?}", fill_flow(&harness, layer));
}
