//! The flow map's fixed vector as command parameters: an angle and a speed, shared by the
//! brush (`tool.material`) and fill layers (`layer.fill.set`).

use serde_json::{Value, json};
use texturecraft_core::{Channel, ChannelKind, flow_from_polar, flow_to_polar};

use crate::Result;
use crate::commands::{bad, f32p};
use crate::session::ToolState;

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

/// The `follow` param of the brush's material: whether strokes paint the direction they travel
/// into `channel` (the flow map) instead of the fixed direction. None when it is not given.
/// Anything but true or false is refused, and so is a channel that has no direction.
pub(crate) fn follow_param(cmd: &str, params: &Value, channel: Option<Channel>) -> Result<Option<bool>> {
    let follow = match params.get("follow") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Bool(follow)) => *follow,
        Some(_) => return Err(bad(cmd, "`follow` must be true or false")),
    };

    if channel.map(Channel::kind) != Some(ChannelKind::Vector) {
        return Err(bad(cmd, format!("`follow` makes strokes paint the direction they travel: it goes on the {} channel", Channel::Flow.id())));
    }

    Ok(Some(follow))
}

/// Make `tool` follow strokes or not after a material command on `channel`: as `follow` says,
/// and without it a fixed direction that was given (`direction_given`: an angle, or the packed
/// value itself) is what the next stroke paints. A speed alone changes neither.
pub(crate) fn set_follow(tool: &mut ToolState, channel: Channel, follow: Option<bool>, direction_given: bool) {
    if channel.kind() != ChannelKind::Vector {
        return;
    }

    match follow {
        Some(follow) => tool.flow_follow = follow,
        None if direction_given => tool.flow_follow = false,
        None => {}
    }
}

/// The brush's flow map settings as commands and inspect report them: whether strokes follow
/// their direction, and the fixed direction and speed.
pub(crate) fn tool_flow_json(tool: &ToolState) -> Value {
    let (angle, speed) = flow_to_polar(tool.values.get(&Channel::Flow).copied().unwrap_or(Channel::Flow.default_value()));
    json!({ "follow": tool.flow_follow, "angle": angle, "speed": speed })
}

#[cfg(test)]
mod tests {
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
    fn follow_is_a_switch_of_the_flow_channel_only() {
        assert_eq!(follow_param("t", &json!({"follow": true}), Some(Channel::Flow)).unwrap(), Some(true));
        assert_eq!(follow_param("t", &json!({"follow": false}), Some(Channel::Flow)).unwrap(), Some(false));
        assert_eq!(follow_param("t", &json!({"follow": null}), Some(Channel::Flow)).unwrap(), None);
        assert_eq!(follow_param("t", &json!({"angle": 3.0}), Some(Channel::Roughness)).unwrap(), None);

        for malformed in [json!("yes"), json!(1), json!(0.0), json!([true]), json!({"on": true})] {
            let error = follow_param("t", &json!({"follow": malformed}), Some(Channel::Flow)).unwrap_err().to_string();
            assert!(error.contains("true or false"), "{error}");
        }
        for channel in [Some(Channel::Roughness), Some(Channel::Normal), None] {
            let error = follow_param("t", &json!({"follow": true}), channel).unwrap_err().to_string();
            assert!(error.contains("flow"), "{error}");
        }
    }

    #[test]
    fn a_fixed_direction_given_alone_stops_following() {
        let mut tool = ToolState::default();
        assert!(tool.flow_follow, "strokes follow their direction until told otherwise");

        set_follow(&mut tool, Channel::Flow, None, false);
        assert!(tool.flow_follow, "a speed alone changes nothing");
        set_follow(&mut tool, Channel::Roughness, Some(false), true);
        assert!(tool.flow_follow, "another channel has no say");

        set_follow(&mut tool, Channel::Flow, None, true);
        assert!(!tool.flow_follow);
        set_follow(&mut tool, Channel::Flow, Some(true), true);
        assert!(tool.flow_follow, "asked for by name, it wins over the direction given with it");
    }

    #[test]
    fn the_report_names_the_mode_the_direction_and_the_speed() {
        let mut tool = ToolState::default();
        assert_eq!(tool_flow_json(&tool), json!({"follow": true, "angle": 0.0, "speed": 1.0}));

        tool.flow_follow = false;
        tool.values.insert(Channel::Flow, flow_from_polar(90.0, 0.5));
        let report = tool_flow_json(&tool);
        assert_eq!(report["follow"], json!(false));
        assert!((report["angle"].as_f64().unwrap() - 90.0).abs() < 1e-3 && (report["speed"].as_f64().unwrap() - 0.5).abs() < 1e-6, "{report}");
    }

    #[test]
    fn a_direct_flow_value_keeps_blue_empty() {
        assert_eq!(uniform_for(Channel::Flow, [0.75, 2.0, 0.9]), [0.75, 1.0, 0.0]);
        assert_eq!(uniform_for(Channel::BaseColor, [0.75, 2.0, 0.9]), [0.75, 2.0, 0.9]);
    }
}
