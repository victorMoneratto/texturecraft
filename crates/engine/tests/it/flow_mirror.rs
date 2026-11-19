//! Strokes that follow the hand store their direction in the one packing every flow map value
//! uses: opposite strokes at one speed lie the same number of levels from still water

use serde_json::json;
use texturecraft_engine::core::{Channel, FLOW_REACH_LEVELS, FLOW_STILL_LEVEL};
use texturecraft_engine::{Session, execute};

use super::flow_follow::{follow_session, stroke_uv};
use super::flow_map::RESOLUTION;

/// Texture coordinates of the texels read along the middle of a stroke from 0.3 to 0.7, clear
/// of both ends
const ALONG_THE_MIDDLE: [f32; 5] = [0.4, 0.45, 0.5, 0.55, 0.6];

/// How many levels the composited flow map lies from still water at (`u`, `v`), in red and green
fn levels_from_still(session: &Session, u: f32, v: f32) -> [i32; 2] {
    let texel = session.caches[0].composite.channels[&Channel::Flow].get((u * RESOLUTION as f32) as u32, (v * RESOLUTION as f32) as u32);
    [texel[0], texel[1]].map(|level| i32::from(level) - i32::from(FLOW_STILL_LEVEL))
}

/// The levels from still water along the middle of one stroke through `points` at `speed`, on
/// a fresh project; `along_u` says which way the stroke runs
fn stroke_levels(speed: f32, points: [[f32; 2]; 2], along_u: bool) -> Vec<[i32; 2]> {
    let mut session = follow_session();
    execute(&mut session, "tool.material", &json!({"channel": "flow", "speed": speed})).unwrap();
    stroke_uv(&mut session, &points);

    ALONG_THE_MIDDLE.iter().map(|at| if along_u { levels_from_still(&session, *at, 0.5) } else { levels_from_still(&session, 0.5, *at) }).collect()
}

#[test]
fn opposite_strokes_store_mirrored_levels() {
    // Half speed is the tie of the code (0.5 × 32767 is 16383.5): plain rounding stored 16383 one way and 16384 the other
    for speed in [1.0f32, 0.5, 0.3] {
        let reach = (speed * f32::from(FLOW_REACH_LEVELS)).round() as i32;

        let right = stroke_levels(speed, [[0.3, 0.5], [0.7, 0.5]], true);
        let left = stroke_levels(speed, [[0.7, 0.5], [0.3, 0.5]], true);
        let down = stroke_levels(speed, [[0.5, 0.3], [0.5, 0.7]], false);
        let up = stroke_levels(speed, [[0.5, 0.7], [0.5, 0.3]], false);

        for index in 0..ALONG_THE_MIDDLE.len() {
            assert_eq!(right[index], [reach, 0], "speed {speed}, right, texel {index}");
            assert_eq!(left[index], [-reach, 0], "speed {speed}, left, texel {index}");
            assert_eq!(down[index], [0, reach], "speed {speed}, down the image, texel {index}");
            assert_eq!(up[index], [0, -reach], "speed {speed}, up the image, texel {index}");
        }
    }
}

#[test]
fn a_fixed_direction_and_a_followed_stroke_store_the_same_levels() {
    let followed = stroke_levels(0.5, [[0.7, 0.5], [0.3, 0.5]], true);

    let mut session = follow_session();
    execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": 180.0, "speed": 0.5})).unwrap();
    assert!(!session.tool.flow_follow, "a direction given by name is the one painted");
    stroke_uv(&mut session, &[[0.3, 0.5], [0.7, 0.5]]);

    for (index, at) in ALONG_THE_MIDDLE.iter().enumerate() {
        assert_eq!(levels_from_still(&session, *at, 0.5), followed[index], "texel {index}");
    }
}
