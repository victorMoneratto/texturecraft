//! The direction a flow map value was given, kept beside the packed vector. A vector at rest has
//! no direction and a packed one loses a little of it, so whoever edits a value by direction
//! and speed remembers the direction as it was given: the brush in its tool state, a fill's
//! editor for as long as it is open.

use texturecraft_core::{Channel, FLOW_REACH_LEVELS, flow_to_polar};

use crate::session::ToolState;

/// How far a stored direction may be from the remembered one and still be it, in degrees:
/// under what a direction slider shows
const SAME_DIRECTION_DEGREES: f32 = 0.5;

/// A direction in degrees as it is kept: as given within one turn (so a slider put at 360 stays
/// there), wrapped into a turn otherwise. A number that is not finite is no turn.
pub fn kept_direction(angle_degrees: f32) -> f32 {
    if !angle_degrees.is_finite() {
        return 0.0;
    }

    if (0.0..=360.0).contains(&angle_degrees) { angle_degrees } else { angle_degrees.rem_euclid(360.0) }
}

/// The direction of packed flow `value` for an editor that `remembered` one: the remembered one
/// while the value is at rest or still points that way, and the stored one after the value was
/// turned elsewhere (a picked value, a path's settings, another editor).
pub fn direction_shown(remembered: Option<f32>, value: [f32; 3]) -> f32 {
    let (stored_angle, speed) = flow_to_polar(value);

    // A slow vector spans few levels: packing turns it by up to about one level over its length
    let packing_degrees = if speed > 0.0 { (1.0 / (speed * f32::from(FLOW_REACH_LEVELS))).to_degrees() } else { 0.0 };
    let turn = |angle: f32| (angle - stored_angle).rem_euclid(360.0);
    let same_direction = |angle: f32| turn(angle).min(360.0 - turn(angle)) <= SAME_DIRECTION_DEGREES + packing_degrees;
    remembered.filter(|angle| angle.is_finite() && (speed <= 0.0 || same_direction(*angle))).unwrap_or(stored_angle)
}

/// The brush's fixed flow direction in degrees, as it was given: kept while the speed is 0, so
/// the direction is still there when the speed comes back.
pub fn tool_direction(tool: &ToolState) -> f32 {
    direction_shown(Some(tool.flow_angle), tool.values.get(&Channel::Flow).copied().unwrap_or(Channel::Flow.default_value()))
}

#[cfg(test)]
mod tests {
    use texturecraft_core::{FLOW_STILL, flow_from_polar};

    use super::*;

    #[test]
    fn a_still_value_keeps_the_direction_last_given() {
        assert_eq!(flow_to_polar(FLOW_STILL), (0.0, 0.0), "the stored value has no direction");

        assert_eq!(direction_shown(Some(120.0), FLOW_STILL), 120.0);
        assert_eq!(direction_shown(None, FLOW_STILL), 0.0, "nothing was given yet");
    }

    #[test]
    fn a_moving_value_shows_its_own_direction() {
        let value = flow_from_polar(45.0, 0.5);

        // Turned by a picker, a path or another editor since the direction was given
        assert!((direction_shown(Some(120.0), value) - 45.0).abs() < 1e-3);
        assert!((direction_shown(None, value) - 45.0).abs() < 1e-3);

        // The direction it was given is kept as given, not as it packs
        assert_eq!(direction_shown(Some(45.2), value), 45.2);
    }

    #[test]
    fn a_slow_value_keeps_the_direction_it_was_given() {
        // A hundredth of the speed spans 328 levels: packing turns 33.3 degrees by up to 0.17
        for speed in [0.01, 0.003, 0.0005] {
            let value = flow_from_polar(33.3, speed);
            assert!(flow_to_polar(value).1 > 0.0, "speed {speed} is stored");
            assert_eq!(direction_shown(Some(33.3), value), 33.3, "speed {speed}");
        }

        // Another direction is still told apart at a speed an artist would set
        assert!((direction_shown(Some(40.0), flow_from_polar(33.3, 0.01)) - 33.3).abs() < 0.5);
    }

    #[test]
    fn a_full_turn_stays_where_it_was_put() {
        let value = flow_from_polar(360.0, 1.0);
        assert!(!(1.0..359.0).contains(&flow_to_polar(value).0), "a full turn packs as no turn");

        assert_eq!(direction_shown(Some(360.0), value), 360.0);
        assert_eq!(direction_shown(Some(f32::NAN), value), flow_to_polar(value).0, "a hostile memory is not shown");
    }

    #[test]
    fn a_kept_direction_lies_within_one_turn() {
        assert_eq!(kept_direction(0.0), 0.0);
        assert_eq!(kept_direction(360.0), 360.0);
        assert_eq!(kept_direction(725.0), 5.0);
        assert_eq!(kept_direction(-90.0), 270.0);
        for hostile in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(kept_direction(hostile), 0.0);
        }
        assert!((0.0..=360.0).contains(&kept_direction(1e30)));
    }

    #[test]
    fn the_brush_reports_the_direction_it_was_given_until_its_value_is_turned() {
        let mut tool = ToolState::default();
        assert_eq!(tool_direction(&tool), 0.0);

        tool.flow_angle = 200.0;
        tool.values.insert(Channel::Flow, FLOW_STILL);
        assert_eq!(tool_direction(&tool), 200.0, "at rest the direction is remembered");

        // Written by something that does not know the direction, like the material picker
        tool.values.insert(Channel::Flow, flow_from_polar(90.0, 0.7));
        assert!((tool_direction(&tool) - 90.0).abs() < 1e-2);
    }
}
