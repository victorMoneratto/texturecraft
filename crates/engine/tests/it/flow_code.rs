//! The flow map's stored levels from an authored direction to the exported files: opposite
//! flows at one speed are mirrored about still water at every step

use serde_json::{Value, json};
use texturecraft_engine::core::{Channel, FLOW_STILL, LayerId};
use texturecraft_engine::geom::primitives;
use texturecraft_engine::{Session, execute};

const RESOLUTION: u32 = 128;

/// The stored level of still water and its 8-bit and float counterparts
const STILL_16: i32 = 32768;
const STILL_8: i32 = 128;
const STILL_FLOAT: f32 = 0.5;

/// A plane project whose set has the flow map, with one fill layer that sets it everywhere
fn filled_session() -> (Session, LayerId) {
    let mut session = Session::new_project("Flow", primitives::plane(2.0, 4), "builtin:plane", None, RESOLUTION).unwrap();
    execute(&mut session, "textureSet.channel.add", &json!({"channel": "flow"})).unwrap();
    let fill = execute(&mut session, "layer.add.fill", &json!({})).unwrap()["layer"].as_u64().unwrap();
    (session, LayerId(fill))
}

fn flow_everywhere(session: &mut Session, fill: LayerId, angle: f32, speed: f32) {
    execute(session, "layer.fill.set", &json!({"layer": fill.0, "channel": "flow", "angle": angle, "speed": speed, "solo": true})).unwrap();
    session.update();
}

/// One flow map file per kind (8 bit, 16 bit, float), each as painted and with green flipped
fn flow_preset() -> Value {
    let map = |file: &str, format: &str, bit_depth: u8, flip_green: bool| {
        json!({
            "file": file, "format": format, "bit_depth": bit_depth,
            "channels": [
                ["R", {"Channel": {"channel": "Flow", "component": 0}}],
                ["G", {"Channel": {"channel": "Flow", "component": 1}}],
                ["B", {"Constant": 0.0}],
            ],
            "inverted": if flip_green { json!(["G"]) } else { json!([]) },
        })
    };
    json!({"name": "Flow kinds", "maps": [
        map("flow8", "Png", 8, false), map("flow8_flipped", "Png", 8, true),
        map("flow16", "Png", 16, false), map("flow16_flipped", "Png", 16, true),
        map("flow32", "Exr", 32, false), map("flow32_flipped", "Exr", 32, true),
    ]})
}

/// What one flow reads as, stored and in every exported file: red and green
struct Levels {
    stored: [i32; 2],
    eight: [i32; 2],
    eight_flipped: [i32; 2],
    sixteen: [i32; 2],
    sixteen_flipped: [i32; 2],
    float: [f32; 2],
    float_flipped: [f32; 2],
    blue_is_empty: bool,
}

fn levels_of(session: &mut Session, tag: &str) -> Levels {
    let dir = std::env::temp_dir().join(format!("tc_flow_code_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    execute(session, "file.exportTextures", &json!({"dir": dir.display().to_string(), "preset": flow_preset(), "padding": "none"})).unwrap();

    let middle = RESOLUTION / 2;
    let stored = session.caches[0].composite.channels[&Channel::Flow].get(middle, middle);
    let open = |name: &str| image::open(dir.join(name)).unwrap();
    let eight = |name: &str| open(name).to_rgb8().get_pixel(middle, middle).0.map(i32::from);
    let sixteen = |name: &str| open(name).to_rgb16().get_pixel(middle, middle).0.map(i32::from);
    let float = |name: &str| open(name).to_rgb32f().get_pixel(middle, middle).0;
    let (e, ef, s, sf) = (eight("flow8.png"), eight("flow8_flipped.png"), sixteen("flow16.png"), sixteen("flow16_flipped.png"));
    let (f, ff) = (float("flow32.exr"), float("flow32_flipped.exr"));
    let levels = Levels {
        stored: [i32::from(stored[0]), i32::from(stored[1])],
        eight: [e[0], e[1]],
        eight_flipped: [ef[0], ef[1]],
        sixteen: [s[0], s[1]],
        sixteen_flipped: [sf[0], sf[1]],
        float: [f[0], f[1]],
        float_flipped: [ff[0], ff[1]],
        blue_is_empty: stored[2] == 0 && e[2] == 0 && ef[2] == 0 && s[2] == 0 && sf[2] == 0 && f[2] == 0.0 && ff[2] == 0.0,
    };

    let _ = std::fs::remove_dir_all(&dir);
    levels
}

#[test]
fn opposite_flows_at_one_speed_are_mirrored_about_still_water_from_the_layer_to_the_file() {
    let (mut session, fill) = filled_session();

    // Along both axes and both diagonals
    for angle in [0.0f32, 90.0, 45.0, 135.0] {
        for speed in [0.25f32, 0.5, 0.75, 1.0] {
            let what = format!("{angle} degrees and its opposite at speed {speed}");
            flow_everywhere(&mut session, fill, angle, speed);
            let forward = levels_of(&mut session, "forward");
            flow_everywhere(&mut session, fill, angle + 180.0, speed);
            let reverse = levels_of(&mut session, "reverse");

            for k in 0..2 {
                assert_eq!(forward.stored[k] - STILL_16, STILL_16 - reverse.stored[k], "stored, component {k}: {what}");
                assert_eq!(forward.eight[k] - STILL_8, STILL_8 - reverse.eight[k], "8 bit, component {k}: {what}");
                assert_eq!(forward.sixteen[k] - STILL_16, STILL_16 - reverse.sixteen[k], "16 bit, component {k}: {what}");
                assert_eq!(forward.float[k] - STILL_FLOAT, STILL_FLOAT - reverse.float[k], "float, component {k}: {what}");

                // The 16-bit file is the stored map, level for level
                assert_eq!(forward.sixteen[k], forward.stored[k], "{what}");
                assert_eq!(reverse.sixteen[k], reverse.stored[k], "{what}");
            }

            // A flipped green is the opposite flow's green; red is as painted
            for (one, other) in [(&forward, &reverse), (&reverse, &forward)] {
                assert_eq!((one.eight_flipped[0], one.eight_flipped[1]), (one.eight[0], other.eight[1]), "8 bit flipped: {what}");
                assert_eq!((one.sixteen_flipped[0], one.sixteen_flipped[1]), (one.sixteen[0], other.sixteen[1]), "16 bit flipped: {what}");
                assert_eq!((one.float_flipped[0], one.float_flipped[1]), (one.float[0], other.float[1]), "float flipped: {what}");
                assert!(one.blue_is_empty, "{what}");
            }

            // The flow really moves: a mirrored pair of still texels would pass everything above
            assert!(forward.stored != reverse.stored && forward.eight != reverse.eight, "{what}");
        }
    }
}

#[test]
fn still_water_is_one_exact_level_in_the_layer_and_in_every_file_kind() {
    let (mut session, fill) = filled_session();

    // A fill at speed 0, and a set whose flow map nothing has touched
    flow_everywhere(&mut session, fill, 70.0, 0.0);
    let filled = levels_of(&mut session, "still_fill");
    execute(&mut session, "layer.remove", &json!({"layer": fill.0})).unwrap();
    session.update();
    let untouched = levels_of(&mut session, "still_empty");

    for (levels, what) in [(&filled, "a fill at speed 0"), (&untouched, "an untouched flow map")] {
        assert_eq!(levels.stored, [STILL_16; 2], "{what}");
        assert_eq!((levels.eight, levels.eight_flipped), ([STILL_8; 2], [STILL_8; 2]), "{what}");
        assert_eq!((levels.sixteen, levels.sixteen_flipped), ([STILL_16; 2], [STILL_16; 2]), "{what}");
        assert_eq!((levels.float, levels.float_flipped), ([STILL_FLOAT; 2], [STILL_FLOAT; 2]), "{what}");
        assert!(levels.blue_is_empty, "{what}");
    }

    // The packed still value is the one the composite stores as its still level
    assert_eq!(session.caches[0].composite.value(Channel::Flow, 3, 3), Channel::Flow.default_value());
    assert_eq!(Channel::Flow.default_value(), FLOW_STILL);
}

#[test]
fn an_opaque_stroke_stores_the_levels_of_its_direction_mirrored_like_a_fill() {
    let mut session = Session::new_project("Flow", primitives::plane(2.0, 4), "builtin:plane", None, RESOLUTION).unwrap();
    execute(&mut session, "textureSet.channel.add", &json!({"channel": "flow"})).unwrap();
    execute(&mut session, "tool.paint", &Value::Null).unwrap();
    execute(&mut session, "tool.brush", &json!({"size": 10, "hardness": 100, "flow": 100, "opacity": 100})).unwrap();
    let middle = RESOLUTION / 2;

    for (angle, speed) in [(0.0f32, 0.25f32), (0.0, 0.5), (90.0, 0.75), (45.0, 1.0), (135.0, 0.5)] {
        let mut stored = Vec::new();
        for direction in [angle, angle + 180.0] {
            execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": direction, "speed": speed, "solo": true})).unwrap();
            execute(&mut session, "tool.strokeUv", &json!({"points": [[0.3, 0.5], [0.7, 0.5]]})).unwrap();
            session.update();
            let texel = session.caches[0].composite.channels[&Channel::Flow].get(middle, middle);
            stored.push([i32::from(texel[0]), i32::from(texel[1])]);

            // What the stroke stored is what the direction packs to
            let packed = texturecraft_engine::core::flow_from_polar(direction, speed);
            let expected = [0, 1].map(|k| i32::from(texturecraft_engine::core::flow_stored_level(packed[k])));
            assert_eq!(stored.last(), Some(&expected), "{direction} degrees at {speed}");
            execute(&mut session, "edit.undo", &Value::Null).unwrap();
        }

        for k in 0..2 {
            assert_eq!(stored[0][k] - STILL_16, STILL_16 - stored[1][k], "component {k}: {angle} degrees and its opposite at {speed}");
        }
        assert_ne!(stored[0], stored[1]);
    }
}
