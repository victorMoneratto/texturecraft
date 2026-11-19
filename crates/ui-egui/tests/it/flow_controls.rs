//! Controls that cannot do what they say on the flow map: the layer row's blend mode, which
//! the compositor always takes as Normal, and the levels editor's channel picker

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texturecraft_engine::core::{BlendMode, Channel, LayerId};
use texturecraft_engine::geom::primitives;
use texturecraft_engine::{Session, execute};
use texturecraft_ui_egui::TextureCraftApp;
use texturecraft_ui_egui::docking::Panel;

const SETTLE_FRAMES: usize = 4;

/// Hover text of a layer row's blend mode while the flow map is the displayed channel
const FIXED_MODE_HINT: &str = "A flow map always blends as Normal";

/// A plane project with the flow map and one paint layer named "River", selected
fn flow_session() -> (Session, LayerId) {
    let mut session = Session::new_project("Flow", primitives::plane(2.0, 4), "builtin:plane", None, 256).unwrap();
    execute(&mut session, "textureSet.channel.add", &json!({"channel": "flow"})).unwrap();
    let layer = session.selected_layer().unwrap();
    execute(&mut session, "layer.rename", &json!({"layer": layer.0, "name": "River"})).unwrap();
    (session, layer)
}

fn harness(session: Session, displayed: Channel) -> Harness<'static, TextureCraftApp> {
    let mut harness = Harness::builder().with_size(egui::vec2(1600.0, 1400.0)).build_eframe(|context| TextureCraftApp::new(context, session));
    harness.state_mut().ui.prefs.language = "en".into();
    harness.state_mut().ui.layer_channel = displayed;
    harness.state_mut().ui.dock.focus(Panel::Layers);
    harness.run_steps(SETTLE_FRAMES);
    harness
}

/// Where a layer row's blend mode sits: from the row's right edge a 4-point gap, then the
/// 58-point column that stacks the mode over the opacity
fn mode_position(harness: &Harness<'_, TextureCraftApp>, row_label: &str) -> egui::Pos2 {
    let row = harness.get_by_label(row_label).rect();
    egui::pos2(row.right() - 4.0 - 29.0, row.center().y - 9.0)
}

fn click_at(harness: &mut Harness<'_, TextureCraftApp>, position: egui::Pos2) {
    harness.drag_at(position);
    harness.drop_at(position);
    harness.run_steps(SETTLE_FRAMES);
}

fn stored_mode(harness: &Harness<'_, TextureCraftApp>, layer: LayerId, channel: Channel) -> BlendMode {
    harness.state().session.doc.project.layers[&layer].channels[&channel].mode
}

#[test]
fn the_blend_mode_of_the_flow_map_is_shown_as_normal_and_cannot_be_changed() {
    let (mut session, layer) = flow_session();

    // A mode a command stored: the compositor blends the flow map as Normal all the same
    execute(&mut session, "layer.blend", &json!({"layer": layer.0, "channel": "flow", "mode": "Multiply"})).unwrap();
    let mut harness = harness(session, Channel::Flow);
    assert!(harness.query_by_label(FIXED_MODE_HINT).is_some(), "the row says why its mode is fixed");

    let at = mode_position(&harness, "River");
    click_at(&mut harness, at);

    assert!(harness.query_by_label("Multiply").is_none() && harness.query_by_label("Overlay").is_none(), "no menu of modes opens");
    assert_eq!(stored_mode(&harness, layer, Channel::Flow), BlendMode::Multiply, "nothing was changed");

    // Opacity stays an ordinary control of the channel
    let opacity = harness.get_by_label("Opacity of River").rect().center();
    harness.drag_at(opacity);
    harness.run_steps(2);
    harness.hover_at(opacity - egui::vec2(100.0, 0.0));
    harness.run_steps(2);
    harness.drop_at(opacity - egui::vec2(100.0, 0.0));
    harness.run_steps(SETTLE_FRAMES);
    assert!(harness.state().session.doc.project.layers[&layer].channels[&Channel::Flow].opacity < 0.9);
}

#[test]
fn the_blend_mode_of_another_channel_still_opens_its_menu() {
    let (session, layer) = flow_session();
    let mut harness = harness(session, Channel::BaseColor);
    assert!(harness.query_by_label(FIXED_MODE_HINT).is_none());

    let at = mode_position(&harness, "River");
    click_at(&mut harness, at);
    harness.get_by_label("Multiply").click();
    harness.run_steps(SETTLE_FRAMES);

    assert_eq!(stored_mode(&harness, layer, Channel::BaseColor), BlendMode::Multiply);
}

#[test]
fn the_levels_channel_picker_leaves_out_the_flow_map() {
    let (mut session, layer) = flow_session();
    let effect = execute(&mut session, "effect.add.levels", &json!({"layer": layer.0, "target": "content"})).unwrap()["effect"].as_u64().unwrap();
    execute(&mut session, "layer.select", &json!({"layer": layer.0, "target": effect})).unwrap();

    // The Layers panel displays another channel, so the one box showing base colour is the levels picker
    let mut harness = harness(session, Channel::Metallic);
    harness.state_mut().ui.dock.focus(Panel::Properties);
    harness.run_steps(SETTLE_FRAMES);

    // Other panels name the channels too: count what opening the picker adds
    let named = |harness: &Harness<'_, TextureCraftApp>, label: &str| harness.query_all_by_label(label).count();
    let before = ["Roughness", "Flow map", "Normal"].map(|label| named(&harness, label));

    // The picker shows the first channel levels can remap; open it
    let picker = harness.get_all_by_role(egui::accesskit::Role::ComboBox).find(|node| node.value().as_deref() == Some("Base color")).unwrap();
    picker.click();
    harness.run_steps(SETTLE_FRAMES);

    let after = ["Roughness", "Flow map", "Normal"].map(|label| named(&harness, label));
    assert_eq!(after[0], before[0] + 1, "the menu of channels is open and offers roughness");
    assert_eq!(after[1], before[1], "levels pass the flow map through: it is not offered");
    assert_eq!(after[2], before[2], "as the normal map is not");
}

#[test]
fn a_ui_state_change_by_command_keeps_the_flow_map_displayed() {
    let (session, _) = flow_session();
    let mut harness = harness(session, Channel::BaseColor);
    let context = egui::Context::default();
    let set = |harness: &mut Harness<'_, TextureCraftApp>, patch: serde_json::Value| {
        let reply = texturecraft_ui_egui::control::handle(harness.state_mut(), &context, "ui.set", &patch);
        assert_eq!(reply["ok"], true, "{reply}");
    };

    set(&mut harness, json!({"layer_channel": "Flow"}));
    assert_eq!(harness.state().ui.layer_channel, Channel::Flow);

    // The state is rebuilt from its saved form, where base colour stands in for the flow map
    set(&mut harness, json!({"props_tab": "Material"}));
    assert_eq!(harness.state().ui.layer_channel, Channel::Flow, "a change of something else leaves the displayed channel");

    set(&mut harness, json!({"layer_channel": "Roughness"}));
    assert_eq!(harness.state().ui.layer_channel, Channel::Roughness, "naming another channel still changes it");
}
