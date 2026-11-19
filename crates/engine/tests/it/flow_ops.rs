//! The compositor's rules for the flow map, driven through commands: every filter of the
//! library keeps still water still and the third component empty

use serde_json::{Value, json};
use texturecraft_engine::core::{Channel, FLOW_STILL_LEVEL, flow_from_polar, flow_reversed, flow_stored_level};
use texturecraft_engine::geom::primitives;
use texturecraft_engine::{Session, execute};

const RESOLUTION: u32 = 128;

const STILL_LEVELS: [u16; 3] = [FLOW_STILL_LEVEL, FLOW_STILL_LEVEL, 0];

fn flow_session() -> Session {
    let mut session = Session::new_project("Flow", primitives::plane(2.0, 4), "builtin:plane", None, RESOLUTION).unwrap();
    execute(&mut session, "textureSet.channel.add", &json!({"channel": "flow"})).unwrap();
    execute(&mut session, "tool.paint", &Value::Null).unwrap();
    session
}

/// The stored levels of every texel of the composited flow map, row by row
fn flow_levels(session: &mut Session) -> Vec<[u16; 3]> {
    session.update();
    let image = &session.caches[0].composite.channels[&Channel::Flow];
    (0..RESOLUTION * RESOLUTION).map(|i| image.get(i % RESOLUTION, i / RESOLUTION)).map(|texel| [texel[0], texel[1], texel[2]]).collect()
}

/// What a library filter does to a uniform flow: nothing, its reverse, or still water
fn expected_of_a_uniform_flow(filter_id: &str, moving: [f32; 3]) -> [u16; 3] {
    match filter_id {
        "invert" => flow_reversed(moving).map(flow_stored_level),
        "highPass" => STILL_LEVELS,

        // Moving or averaging texels leaves a uniform field as it is; the rest pass it through
        "blur" | "blurDirectional" | "slopeBlur" | "sharpen" | "warp" | "warpDirectional" | "mosaic" | "hsl" | "levels" | "contrastLuminosity"
        | "colorBalance" | "gradientMap" | "posterize" | "clamp" | "histogramScan" | "histogramRange" | "histogramShift" | "threshold" | "grayscale"
        | "edgeDetect" | "normalInvert" | "bakedLighting" | "replaceColor" | "bevel" | "glow" | "matFinishGrind" | "matFinishRaw" | "matFinishSanded"
        | "matFinishHammered" => moving.map(flow_stored_level),
        unknown => panic!("filter `{unknown}` is new: say here what it does to a flow map"),
    }
}

#[test]
fn every_library_filter_keeps_still_water_still_and_the_third_component_empty() {
    let filters = execute(&mut flow_session(), "filter.list", &Value::Null).unwrap();
    let filters: Vec<String> = filters.as_array().unwrap().iter().map(|filter| filter["id"].as_str().unwrap().to_string()).collect();
    assert!(filters.len() >= 31, "{filters:?}");
    let moving = flow_from_polar(120.0, 0.7);

    let mut session = flow_session();
    let fill = execute(&mut session, "layer.add.fill", &json!({})).unwrap()["layer"].as_u64().unwrap();
    for filter in &filters {
        // A still fill under the filter
        execute(&mut session, "layer.fill.set", &json!({"layer": fill, "channel": "flow", "angle": 0.0, "speed": 0.0, "solo": true})).unwrap();
        let effect = execute(&mut session, "effect.add.filter", &json!({"layer": fill, "target": "content", "filter": filter})).unwrap();
        let effect = effect["effect"].as_u64().unwrap();
        let still = flow_levels(&mut session);
        assert_eq!(still.iter().find(|texel| **texel != STILL_LEVELS), None, "{filter}: still water moved");

        // The same fill flowing one way everywhere
        execute(&mut session, "layer.fill.set", &json!({"layer": fill, "channel": "flow", "angle": 120.0, "speed": 0.7})).unwrap();
        let flowing = flow_levels(&mut session);
        let expected = expected_of_a_uniform_flow(filter, moving);
        assert_eq!(flowing.iter().find(|texel| **texel != expected), None, "{filter}: a uniform flow should read {expected:?}");

        execute(&mut session, "effect.remove", &json!({"effect": effect})).unwrap();
    }
}
