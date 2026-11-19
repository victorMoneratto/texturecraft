//! The flow map's fixed vector as command parameters: an angle and a speed, shared by the
//! brush (`tool.material`) and fill layers (`layer.fill.set`). Everything here reads and checks;
//! the commands change their state only after every parameter passed.

use serde_json::{Value, json};
use texturecraft_core::{Channel, ChannelKind, FLOW_REACH_LEVELS, flow_from_polar, flow_pack, flow_to_polar, flow_unpack};

use crate::Result;
use crate::commands::bad;
use crate::flow_direction::{kept_direction, tool_direction};
use crate::session::ToolState;

/// How much longer than 1 the vector of a packed value given by hand may be: two stored levels,
/// what packing a vector of length 1 can add
const SPEED_TOLERANCE: f32 = 2.0 / FLOW_REACH_LEVELS as f32;

/// What a JSON value is, for a message that refuses it
fn kind_of(given: &Value) -> &'static str {
    match given {
        Value::Null => "null",
        Value::Bool(_) => "true or false",
        Value::Number(_) => "a number out of range",
        Value::String(_) => "text",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
    }
}

/// The number given as `key`, None when the key is absent or null (JSON carries NaN and
/// infinity as null). Anything else that is not a finite number is refused with a message
/// naming the key and saying it takes `accepted`: a malformed value is not an omitted one.
pub(crate) fn number_param(cmd: &str, params: &Value, key: &str, accepted: &str) -> Result<Option<f32>> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(given) => {
            let number = given.as_f64().map(|number| number as f32).filter(|number| number.is_finite());
            number.map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be {accepted}, not {}", kind_of(given))))
        }
    }
}

/// The `angle` (degrees) and `speed` (0..1) of a flow map edit, each None when it is not given,
/// or None when neither is. Refused on a channel that has no direction, and when either is
/// malformed.
pub(crate) fn polar_params(cmd: &str, params: &Value, channel: Option<Channel>) -> Result<Option<(Option<f32>, Option<f32>)>> {
    let angle = number_param(cmd, params, "angle", "a number of degrees")?;
    let speed = number_param(cmd, params, "speed", "a number from 0 to 1")?;
    if angle.is_none() && speed.is_none() {
        return Ok(None);
    }

    if channel.map(Channel::kind) != Some(ChannelKind::Vector) {
        return Err(bad(cmd, format!("`angle` and `speed` set a direction: they go on the {} channel", Channel::Flow.id())));
    }

    Ok(Some((angle, speed.map(|speed| speed.clamp(0.0, 1.0)))))
}

/// The packed flow map value that `angle` and `speed` in `params` give a fill's `channel`,
/// keeping from `current` the one that is not given. None when neither is given. A fill stores
/// the vector alone, so a fill at rest has no direction to keep and none to remember: there an
/// angle needs its speed.
pub(crate) fn fill_flow_param(cmd: &str, params: &Value, channel: Channel, current: [f32; 3]) -> Result<Option<[f32; 3]>> {
    let Some((angle, speed)) = polar_params(cmd, params, Some(channel))? else { return Ok(None) };

    let (current_angle, current_speed) = flow_to_polar(current);
    if speed.is_none() && current_speed <= 0.0 {
        return Err(bad(cmd, "the fill is still, and a still fill keeps no direction: give `speed` with `angle`"));
    }

    Ok(Some(flow_from_polar(angle.unwrap_or(current_angle), speed.unwrap_or(current_speed))))
}

/// A value given directly for `channel` under `key`, made valid for it. A flow map keeps its
/// vector in the first two components, on the levels it is stored as, and nothing in the third;
/// a vector longer than the maximum speed is refused (a value picked from the canvas never is).
pub(crate) fn uniform_for(cmd: &str, key: &str, channel: Channel, value: [f32; 3]) -> Result<[f32; 3]> {
    if channel.kind() != ChannelKind::Vector {
        return Ok(value);
    }

    let (x, y) = (flow_unpack(value[0]), flow_unpack(value[1]));
    let length = x.hypot(y);
    if length > 1.0 + SPEED_TOLERANCE {
        return Err(bad(cmd, format!("`{key}` decodes to a flow of length {length:.3}: the maximum speed is 1 (use `angle` and `speed`)")));
    }

    Ok([flow_pack(x), flow_pack(y), 0.0])
}

/// The `follow` param of the brush's material: whether strokes paint the direction they travel
/// into `channel` (the flow map) instead of the fixed direction. None when it is not given.
/// Anything but true or false is refused, and so is a channel that has no direction.
pub(crate) fn follow_param(cmd: &str, params: &Value, channel: Option<Channel>) -> Result<Option<bool>> {
    let follow = match params.get("follow") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Bool(follow)) => *follow,
        Some(given) => return Err(bad(cmd, format!("`follow` must be true or false, not {}", kind_of(given)))),
    };

    if channel.map(Channel::kind) != Some(ChannelKind::Vector) {
        return Err(bad(cmd, format!("`follow` makes strokes paint the direction they travel: it goes on the {} channel", Channel::Flow.id())));
    }

    Ok(Some(follow))
}

/// Give the brush's flow map the direction and the speed of a material command, keeping the
/// one that is not given. The direction is kept as it was given beside the packed vector: a
/// speed of 0 leaves it in place for when the speed comes back, and a direction given at
/// speed 0 is remembered while the brush stays at rest.
pub(crate) fn set_tool_polar(tool: &mut ToolState, (angle, speed): (Option<f32>, Option<f32>)) {
    let current = tool.values.get(&Channel::Flow).copied().unwrap_or(Channel::Flow.default_value());
    let direction = angle.map_or_else(|| tool_direction(tool), kept_direction);
    let speed = speed.unwrap_or(flow_to_polar(current).1);

    tool.flow_angle = direction;
    tool.values.insert(Channel::Flow, flow_from_polar(direction, speed));
}

/// Give the brush's flow map the packed `value` of a material command. A value that moves
/// brings its own direction; one at rest leaves the remembered direction alone.
pub(crate) fn set_tool_flow_value(tool: &mut ToolState, value: [f32; 3]) {
    let (angle, speed) = flow_to_polar(value);
    if speed > 0.0 {
        tool.flow_angle = angle;
    }
    tool.values.insert(Channel::Flow, value);
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
/// their direction, the fixed direction as it was given, the speed, and the packed value.
pub(crate) fn tool_flow_json(tool: &ToolState) -> Value {
    let value = tool.values.get(&Channel::Flow).copied().unwrap_or(Channel::Flow.default_value());
    json!({ "follow": tool.flow_follow, "angle": tool_direction(tool), "speed": flow_to_polar(value).1, "value": value })
}

#[cfg(test)]
mod tests {
    use texturecraft_core::FLOW_STILL;

    use super::*;

    #[test]
    fn a_number_is_given_absent_or_refused() {
        assert_eq!(number_param("t", &json!({"angle": 12.5}), "angle", "degrees").unwrap(), Some(12.5));
        assert_eq!(number_param("t", &json!({"angle": 3}), "angle", "degrees").unwrap(), Some(3.0));
        assert_eq!(number_param("t", &json!({}), "angle", "degrees").unwrap(), None);
        assert_eq!(number_param("t", &json!({"angle": null}), "angle", "degrees").unwrap(), None);
        assert_eq!(number_param("t", &Value::Null, "angle", "degrees").unwrap(), None);

        // A number f32 cannot hold is not a number here
        for malformed in [json!("north"), json!([1, 2]), json!(true), json!({"degrees": 1}), json!(1e100), json!(-1e300)] {
            let error = number_param("t", &json!({"angle": malformed}), "angle", "a number of degrees").unwrap_err().to_string();
            assert!(error.contains("`angle`") && error.contains("a number of degrees"), "{error}");
        }
    }

    #[test]
    fn a_fill_keeps_the_one_of_angle_and_speed_that_is_not_given() {
        let east = flow_from_polar(0.0, 0.5);

        let turned = fill_flow_param("t", &json!({"angle": 90.0}), Channel::Flow, east).unwrap().unwrap();
        assert_eq!(flow_to_polar(turned).1, 0.5, "the speed is kept");
        assert!((flow_to_polar(turned).0 - 90.0).abs() < 1e-3);

        let slowed = fill_flow_param("t", &json!({"speed": 0.25}), Channel::Flow, east).unwrap().unwrap();
        assert!((flow_to_polar(slowed).1 - 0.25).abs() < 1e-6);
        assert!(flow_to_polar(slowed).0.abs() < 1e-3, "the direction is kept");

        assert_eq!(fill_flow_param("t", &json!({"color": [1, 0, 0]}), Channel::Flow, east).unwrap(), None);
    }

    #[test]
    fn a_still_fill_needs_a_speed_with_its_direction() {
        let error = fill_flow_param("t", &json!({"angle": 180.0}), Channel::Flow, FLOW_STILL).unwrap_err().to_string();
        assert!(error.contains("`speed`"), "{error}");

        let both = fill_flow_param("t", &json!({"angle": 180.0, "speed": 0.5}), Channel::Flow, FLOW_STILL).unwrap().unwrap();
        assert_eq!(both, flow_from_polar(180.0, 0.5));

        // A speed alone moves a still fill the way a still value reads: to the right
        let moved = fill_flow_param("t", &json!({"speed": 1.0}), Channel::Flow, FLOW_STILL).unwrap().unwrap();
        assert_eq!(moved, flow_from_polar(0.0, 1.0));
        assert_eq!(fill_flow_param("t", &json!({"angle": 180.0, "speed": 0.0}), Channel::Flow, FLOW_STILL).unwrap(), Some(FLOW_STILL));
    }

    #[test]
    fn other_channels_refuse_a_direction() {
        assert!(fill_flow_param("t", &json!({"angle": 10.0}), Channel::Roughness, [0.5; 3]).is_err());
        assert!(polar_params("t", &json!({"speed": 0.5}), Some(Channel::BaseColor)).is_err());
        assert!(polar_params("t", &json!({"angle": 10.0}), None).is_err(), "a direction without a channel has nowhere to go");
        assert_eq!(polar_params("t", &json!({"value": 0.5}), None).unwrap(), None);
    }

    #[test]
    fn the_brush_keeps_its_direction_through_a_speed_of_zero() {
        let mut tool = ToolState::default();
        set_tool_polar(&mut tool, (Some(90.0), Some(1.0)));
        assert_eq!(tool.values[&Channel::Flow], flow_from_polar(90.0, 1.0));

        set_tool_polar(&mut tool, (None, Some(0.0)));
        assert_eq!(tool.values[&Channel::Flow], FLOW_STILL);
        assert_eq!(tool_direction(&tool), 90.0);

        set_tool_polar(&mut tool, (None, Some(1.0)));
        assert_eq!(tool.values[&Channel::Flow], flow_from_polar(90.0, 1.0), "the speed comes back the way it went");
    }

    #[test]
    fn a_direction_given_at_rest_is_remembered_and_keeps_the_brush_at_rest() {
        let mut tool = ToolState::default();
        set_tool_polar(&mut tool, (None, Some(0.0)));

        set_tool_polar(&mut tool, (Some(200.0), None));
        assert_eq!(tool.values[&Channel::Flow], FLOW_STILL, "the speed that was not given is kept");
        assert_eq!(tool_direction(&tool), 200.0);

        set_tool_polar(&mut tool, (None, Some(0.5)));
        assert_eq!(tool.values[&Channel::Flow], flow_from_polar(200.0, 0.5));
    }

    #[test]
    fn a_packed_value_brings_its_direction_unless_it_is_at_rest() {
        let mut tool = ToolState::default();
        set_tool_polar(&mut tool, (Some(30.0), Some(1.0)));

        set_tool_flow_value(&mut tool, flow_from_polar(270.0, 0.5));
        assert!((tool_direction(&tool) - 270.0).abs() < 1e-2);

        set_tool_flow_value(&mut tool, FLOW_STILL);
        assert!((tool_direction(&tool) - 270.0).abs() < 1e-2, "still water says nothing about the direction");
    }

    #[test]
    fn follow_is_a_switch_of_the_flow_channel_only() {
        assert_eq!(follow_param("t", &json!({"follow": true}), Some(Channel::Flow)).unwrap(), Some(true));
        assert_eq!(follow_param("t", &json!({"follow": false}), Some(Channel::Flow)).unwrap(), Some(false));
        assert_eq!(follow_param("t", &json!({"follow": null}), Some(Channel::Flow)).unwrap(), None);
        assert_eq!(follow_param("t", &json!({"angle": 3.0}), Some(Channel::Roughness)).unwrap(), None);

        for malformed in [json!("yes"), json!(1), json!(0.0), json!([true]), json!({"on": true})] {
            let error = follow_param("t", &json!({"follow": malformed}), Some(Channel::Flow)).unwrap_err().to_string();
            assert!(error.contains("`follow`") && error.contains("true or false"), "{error}");
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
    fn the_report_names_the_mode_the_direction_the_speed_and_the_value() {
        let mut tool = ToolState::default();
        assert_eq!(tool_flow_json(&tool), json!({"follow": true, "angle": 0.0, "speed": 1.0, "value": flow_from_polar(0.0, 1.0)}));

        tool.flow_follow = false;
        set_tool_polar(&mut tool, (Some(90.0), Some(0.5)));
        assert_eq!(tool_flow_json(&tool), json!({"follow": false, "angle": 90.0, "speed": 0.5, "value": flow_from_polar(90.0, 0.5)}));

        // At rest the report still says where the brush will flow
        set_tool_polar(&mut tool, (None, Some(0.0)));
        assert_eq!(tool_flow_json(&tool), json!({"follow": false, "angle": 90.0, "speed": 0.0, "value": FLOW_STILL}));
    }

    #[test]
    fn a_direct_flow_value_keeps_blue_empty_and_sits_on_stored_levels() {
        let east = uniform_for("t", "value", Channel::Flow, [1.0, 0.5, 0.9]).unwrap();
        assert_eq!(east, flow_from_polar(0.0, 1.0));
        assert_eq!(uniform_for("t", "value", Channel::Flow, [0.5; 3]).unwrap(), FLOW_STILL);
        assert_eq!(uniform_for("t", "value", Channel::BaseColor, [0.75, 2.0, 0.9]).unwrap(), [0.75, 2.0, 0.9]);

        // What the canvas can hold is accepted as it is
        for (angle, speed) in [(45.0, 1.0), (133.0, 1.0), (300.0, 0.6), (17.0, 0.001)] {
            let packed = flow_from_polar(angle, speed);
            assert_eq!(uniform_for("t", "value", Channel::Flow, packed).unwrap(), packed, "{angle} {speed}");
        }
    }

    #[test]
    fn a_direct_flow_value_faster_than_the_maximum_is_refused() {
        let error = uniform_for("t", "value", Channel::Flow, [1.0, 1.0, 0.0]).unwrap_err().to_string();
        assert!(error.contains("`value`") && error.contains("1.414") && error.contains("maximum speed is 1"), "{error}");

        // Components out of range are clamped first, so they cannot hide a long vector
        assert!(uniform_for("t", "color", Channel::Flow, [7.0, -3.0, 0.0]).unwrap_err().to_string().contains("`color`"));
        assert!(uniform_for("t", "value", Channel::Flow, [1.0, 0.51, 0.0]).is_err());

        // Within two stored levels of the maximum it is the rounding of a full-speed vector
        let just_over = [flow_pack(1.0), flow_pack(2.0 / f32::from(FLOW_REACH_LEVELS)), 0.0];
        assert!(uniform_for("t", "value", Channel::Flow, just_over).is_ok());
    }
}
