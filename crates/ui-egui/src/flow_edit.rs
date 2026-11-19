use egui::Ui;
use serde_json::json;
use texturecraft_engine::core::{Channel, flow_to_polar};
use texturecraft_engine::flow_direction::{direction_shown, tool_direction};
use texturecraft_engine::flow_stroke::{can_follow_stroke, follows_stroke};

use crate::TextureCraftApp;
use crate::i18n::tr;
use crate::widgets::slider_edit;

/// A change made with the direction and speed sliders of a fill's flow map value, shown as
/// these two instead of the packed vector.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FlowEdit {
    /// Counter-clockwise on the image from pointing right, as `flow_from_polar` takes it
    pub angle_degrees: f32,
    pub speed: f32,
    /// A later change of one slider drag: it continues the drag's undo step
    pub merge: bool,
}

impl FlowEdit {
    /// The sliders for the packed flow `value`, and the change when one of them moved: always
    /// both the direction and the speed, so a still value is given both in one drag. `id`
    /// names the editor: a fill stores the vector alone and a still one has no direction, so
    /// the direction last shown there is kept in egui's memory and the slider stays where it
    /// was put.
    pub(crate) fn sliders(ui: &mut Ui, id: egui::Id, value: [f32; 3]) -> Option<FlowEdit> {
        let (_, mut speed) = flow_to_polar(value);
        let remembered = ui.data(|data| data.get_temp::<f32>(id));
        let mut angle_degrees = direction_shown(remembered, value);

        let turned = slider_edit(ui, tr("Flow direction"), &mut angle_degrees, 0.0..=360.0, 0);
        let paced = slider_edit(ui, tr("Flow speed"), &mut speed, 0.0..=1.0, 2);
        ui.data_mut(|data| data.insert_temp(id, angle_degrees));

        turned.or(paced).map(|merge| FlowEdit { angle_degrees, speed, merge })
    }
}

/// The brush's flow map controls for `channel`, whose painted value is `value`: for a tool that
/// strokes, whether its strokes paint the way they travel, then the fixed direction while it is
/// what gets painted, and the speed. Every change goes through `tool.material`, so the panel
/// and a command mean the same by a direction or a speed alone; the direction shown is the one
/// the tool keeps, also while the speed is 0.
pub(crate) fn brush_controls(app: &mut TextureCraftApp, ui: &mut Ui, channel: Channel, value: [f32; 3]) {
    let follow = app.session.tool.flow_follow;
    if can_follow_stroke(&app.session.tool) {
        let mut follow = follow;
        let hint = tr("Paint the direction the stroke travels. Off: paint one fixed direction");
        if ui.checkbox(&mut follow, tr("Follow stroke")).on_hover_text(hint).changed() {
            app.run("tool.material", json!({"channel": channel.id(), "follow": follow}));
        }
    }

    let fixed_direction = !follows_stroke(&app.session.tool);
    let mut angle_degrees = tool_direction(&app.session.tool);
    let (_, mut speed) = flow_to_polar(value);
    let turned = if fixed_direction { slider_edit(ui, tr("Flow direction"), &mut angle_degrees, 0.0..=360.0, 0) } else { None };
    let paced = slider_edit(ui, tr("Flow speed"), &mut speed, 0.0..=1.0, 2);

    // A tool that does not stroke shows its direction with the switch as it is: turning it there
    // must not switch following off for the tools that do stroke
    if let Some(merge) = turned {
        app.run("tool.material", json!({"channel": channel.id(), "angle": angle_degrees, "follow": follow, "merge": merge}));
    }
    if let Some(merge) = paced {
        app.run("tool.material", json!({"channel": channel.id(), "speed": speed, "merge": merge}));
    }
}
