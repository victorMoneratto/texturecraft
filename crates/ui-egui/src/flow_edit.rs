use egui::Ui;
use texturecraft_engine::core::{flow_from_polar, flow_to_polar};

use crate::i18n::tr;
use crate::widgets::slider_edit;

/// A change made with the direction and speed sliders of a flow map value: the brush's
/// material and a fill layer's source both show these two instead of the packed vector.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FlowEdit {
    /// Counter-clockwise on the image from pointing right, as `flow_from_polar` takes it
    pub angle_degrees: f32,
    pub speed: f32,
    /// A later change of one slider drag: it continues the drag's undo step
    pub merge: bool,
}

/// How far the stored direction may be from the remembered one and still be it, in degrees:
/// under what the direction slider shows, over what packing a vector loses
const SAME_DIRECTION_DEGREES: f32 = 0.5;

impl FlowEdit {
    /// The sliders for the packed flow `value`, and the change when one of them moved. `id`
    /// names the editor: a still value has no direction, so the direction last shown there
    /// is kept in egui's memory and the slider stays where it was put.
    pub(crate) fn sliders(ui: &mut Ui, id: egui::Id, value: [f32; 3]) -> Option<FlowEdit> {
        let (stored_angle, mut speed) = flow_to_polar(value);
        let remembered = ui.data(|data| data.get_temp::<f32>(id));
        let mut angle_degrees = shown_angle(remembered, stored_angle, speed);

        let turned = slider_edit(ui, tr("Flow direction"), &mut angle_degrees, 0.0..=360.0, 0);
        let paced = slider_edit(ui, tr("Flow speed"), &mut speed, 0.0..=1.0, 2);
        ui.data_mut(|data| data.insert_temp(id, angle_degrees));

        turned.or(paced).map(|merge| FlowEdit { angle_degrees, speed, merge })
    }

    /// The packed flow map value of the change
    pub(crate) fn value(&self) -> [f32; 3] {
        flow_from_polar(self.angle_degrees, self.speed)
    }
}

/// The direction an editor shows for a value stored as `stored_angle` at `speed`: the one it
/// `remembered` while the value is still or still points that way (so 360 does not turn into
/// 0), and the stored one after the value was turned elsewhere.
fn shown_angle(remembered: Option<f32>, stored_angle: f32, speed: f32) -> f32 {
    let turn = |angle: f32| (angle - stored_angle).rem_euclid(360.0);
    let same_direction = |angle: f32| turn(angle).min(360.0 - turn(angle)) <= SAME_DIRECTION_DEGREES;
    remembered.filter(|angle| angle.is_finite() && (speed <= 0.0 || same_direction(*angle))).unwrap_or(stored_angle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use texturecraft_engine::core::FLOW_STILL;

    #[test]
    fn a_still_value_keeps_the_direction_last_shown() {
        let (stored_angle, speed) = flow_to_polar(FLOW_STILL);
        assert_eq!((stored_angle, speed), (0.0, 0.0), "the stored value has no direction");

        assert_eq!(shown_angle(Some(120.0), stored_angle, speed), 120.0);
        assert_eq!(shown_angle(None, stored_angle, speed), 0.0, "nothing was shown here yet");
    }

    #[test]
    fn a_moving_value_shows_its_own_direction() {
        let (stored_angle, speed) = flow_to_polar(flow_from_polar(45.0, 0.5));

        // Turned by a command or another editor since this one last showed it
        assert!((shown_angle(Some(120.0), stored_angle, speed) - 45.0).abs() < 1e-3);
        assert!((shown_angle(None, stored_angle, speed) - 45.0).abs() < 1e-3);

        // The direction it was given here is kept as given, not as it packs
        assert_eq!(shown_angle(Some(45.2), stored_angle, speed), 45.2);
    }

    #[test]
    fn a_full_turn_stays_where_the_slider_was_put() {
        let (stored_angle, speed) = flow_to_polar(flow_from_polar(360.0, 1.0));
        assert!(!(1.0..359.0).contains(&stored_angle), "a full turn packs as no turn: {stored_angle}");

        assert_eq!(shown_angle(Some(360.0), stored_angle, speed), 360.0);
        assert_eq!(shown_angle(Some(f32::NAN), stored_angle, speed), stored_angle, "a hostile memory is not shown");
    }

    #[test]
    fn the_change_packs_its_direction_and_speed() {
        let edit = FlowEdit { angle_degrees: 90.0, speed: 0.5, merge: false };
        let packed = edit.value();
        assert!((packed[0] - 0.5).abs() < 1e-6 && (packed[1] - 0.25).abs() < 1e-6 && packed[2] == 0.0, "{packed:?}");

        // No speed is still water whatever the direction: the direction lives in the editor only
        assert_eq!(FlowEdit { angle_degrees: 200.0, speed: 0.0, merge: false }.value(), FLOW_STILL);
    }
}
