//! The flow map's fixed vector as command parameters: an angle and a speed, shared by the
//! brush (`tool.material`) and fill layers (`layer.fill.set`).

use serde_json::Value;
use texturecraft_core::{Channel, ChannelKind, flow_from_polar, flow_to_polar};

use crate::Result;
use crate::commands::{bad, f32p};

/// A flow that was still starts moving at full speed when only its direction is given
const SPEED_OF_A_NEW_DIRECTION: f32 = 1.0;

/// The packed flow map value that `angle` and `speed` in `params` give `channel`, keeping from
/// `current` the one that is not given. None when neither is given.
pub(crate) fn flow_value_param(cmd: &str, params: &Value, channel: Channel, current: [f32; 3]) -> Result<Option<[f32; 3]>> {
    let (angle, speed) = (f32p(params, "angle"), f32p(params, "speed"));
    if angle.is_none() && speed.is_none() {
        return Ok(None);
    }

    if channel.kind() != ChannelKind::Vector {
        return Err(bad(cmd, format!("`angle` and `speed` set a direction: they go on the {} channel", Channel::Flow.id())));
    }

    let (current_angle, current_speed) = flow_to_polar(current);
    let speed = speed.unwrap_or(if current_speed > 0.0 { current_speed } else { SPEED_OF_A_NEW_DIRECTION });
    Ok(Some(flow_from_polar(angle.unwrap_or(current_angle), speed)))
}

/// A value given directly for `channel`, made valid for it: a flow map keeps its vector in the
/// first two components and nothing in the third.
pub(crate) fn uniform_for(channel: Channel, value: [f32; 3]) -> [f32; 3] {
    match channel.kind() {
        ChannelKind::Vector => [value[0].clamp(0.0, 1.0), value[1].clamp(0.0, 1.0), 0.0],
        _ => value,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use texturecraft_core::FLOW_STILL;

    use super::*;

    #[test]
    fn angle_and_speed_edit_the_current_flow() {
        let east = flow_from_polar(0.0, 0.5);

        let turned = flow_value_param("t", &json!({"angle": 90.0}), Channel::Flow, east).unwrap().unwrap();
        assert_eq!(flow_to_polar(turned).1, 0.5, "the speed is kept");
        assert!((flow_to_polar(turned).0 - 90.0).abs() < 1e-3);

        let slowed = flow_value_param("t", &json!({"speed": 0.25}), Channel::Flow, east).unwrap().unwrap();
        assert!((flow_to_polar(slowed).1 - 0.25).abs() < 1e-6);
        assert!(flow_to_polar(slowed).0.abs() < 1e-3, "the direction is kept");

        assert_eq!(flow_value_param("t", &json!({"color": [1, 0, 0]}), Channel::Flow, east).unwrap(), None);
    }

    #[test]
    fn a_direction_given_to_still_water_moves_it() {
        let value = flow_value_param("t", &json!({"angle": 180.0}), Channel::Flow, FLOW_STILL).unwrap().unwrap();
        assert!((flow_to_polar(value).1 - 1.0).abs() < 1e-6);
    }

    #[test]
    fn other_channels_refuse_a_direction() {
        assert!(flow_value_param("t", &json!({"angle": 10.0}), Channel::Roughness, [0.5; 3]).is_err());
    }

    #[test]
    fn a_direct_flow_value_keeps_blue_empty() {
        assert_eq!(uniform_for(Channel::Flow, [0.75, 2.0, 0.9]), [0.75, 1.0, 0.0]);
        assert_eq!(uniform_for(Channel::BaseColor, [0.75, 2.0, 0.9]), [0.75, 2.0, 0.9]);
    }
}
