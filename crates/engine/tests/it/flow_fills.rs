//! Which fill layers take the flow map: none by default, every one whose caller asks for it

use serde_json::{Value, json};
use texturecraft_engine::core::{Channel, FLOW_STILL_LEVEL, LayerId, flow_from_polar, flow_stored_level};
use texturecraft_engine::geom::primitives;
use texturecraft_engine::{Session, execute};

const RESOLUTION: u32 = 128;

fn flow_session() -> Session {
    let mut session = Session::new_project("Flow", primitives::plane(2.0, 4), "builtin:plane", None, RESOLUTION).unwrap();
    execute(&mut session, "textureSet.channel.add", &json!({"channel": "flow"})).unwrap();
    execute(&mut session, "tool.paint", &Value::Null).unwrap();
    session
}

/// The stored red and green levels of the composited flow map at (`u`, `v`)
fn flow_levels_at(session: &mut Session, u: f32, v: f32) -> [u16; 2] {
    session.update();
    let texel = session.caches[0].composite.channels[&Channel::Flow].get((u * RESOLUTION as f32) as u32, (v * RESOLUTION as f32) as u32);
    [texel[0], texel[1]]
}

fn has_flow(session: &Session, layer: u64) -> bool {
    session.doc.project.layer(LayerId(layer)).unwrap().channels.contains_key(&Channel::Flow)
}

/// A small flow map image: every texel flows right at full speed
fn import_flow_image(session: &mut Session) -> String {
    let mut png = Vec::new();
    let image = image::RgbaImage::from_fn(8, 8, |_, _| image::Rgba([255, 128, 0, 255]));
    image.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    session.import_resource_bytes("C:/x/river flow.png", png).unwrap()
}

#[test]
fn a_brush_material_paints_the_flow_direction_the_brush_was_given() {
    let mut session = flow_session();
    execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": 90.0, "speed": 1.0, "solo": true})).unwrap();
    execute(&mut session, "tool.material", &json!({"material": "steel"})).unwrap();
    assert_eq!(session.tool.enabled.iter().copied().collect::<Vec<_>>(), [Channel::Flow], "the material keeps the channel toggles");
    execute(&mut session, "tool.brush", &json!({"size": 10, "hardness": 100})).unwrap();

    execute(&mut session, "tool.strokeUv", &json!({"points": [[0.3, 0.5], [0.7, 0.5]]})).unwrap();

    let north = flow_from_polar(90.0, 1.0);
    assert_eq!(flow_levels_at(&mut session, 0.5, 0.5), [flow_stored_level(north[0]), flow_stored_level(north[1])]);
    assert_eq!(flow_levels_at(&mut session, 0.5, 0.9), [FLOW_STILL_LEVEL; 2], "away from the stroke nothing moves");
}

#[test]
fn an_image_fill_asked_for_the_flow_map_gets_it() {
    let mut session = flow_session();
    let name = import_flow_image(&mut session);

    let layer = execute(&mut session, "layer.add.image", &json!({"name": name, "channel": "flow"})).unwrap()["layer"].as_u64().unwrap();

    assert!(has_flow(&session, layer));
    let shown = flow_levels_at(&mut session, 0.5, 0.5);
    assert!(shown[0] == u16::MAX && shown[1] != 0, "the image's flow shows: {shown:?}");

    // One asked for another channel stays out of the flow map
    let other = execute(&mut session, "layer.add.image", &json!({"name": name, "channel": "roughness"})).unwrap()["layer"].as_u64().unwrap();
    assert!(!has_flow(&session, other));
}

#[test]
fn a_decal_asked_for_the_flow_map_gets_it_and_a_mask_decal_stays_out() {
    let mut session = flow_session();
    let name = import_flow_image(&mut session);

    let decal = execute(&mut session, "layer.add.decal", &json!({"image": name, "channel": "flow", "position": [0.0, 0.0, 0.0], "normal": [0.0, 1.0, 0.0]}))
        .unwrap()["layer"]
        .as_u64()
        .unwrap();
    assert!(has_flow(&session, decal));

    // A decal that masks a fill of the set's channels is an ordinary fill: no flow map
    let masked = execute(&mut session, "layer.add.decal", &json!({"image": name, "channel": "mask", "position": [0.0, 0.0, 0.0], "normal": [0.0, 1.0, 0.0]}))
        .unwrap()["layer"]
        .as_u64()
        .unwrap();
    assert!(!has_flow(&session, masked));
    assert!(session.doc.project.layer(LayerId(masked)).unwrap().channels.contains_key(&Channel::BaseColor));
}

#[test]
fn an_ordinary_fill_still_stays_out_of_the_flow_map() {
    let mut session = flow_session();

    let fill = execute(&mut session, "layer.add.fill", &json!({"color": [0.2, 0.4, 0.6]})).unwrap()["layer"].as_u64().unwrap();

    assert!(!has_flow(&session, fill));
    assert!(session.doc.project.layer(LayerId(fill)).unwrap().channels.contains_key(&Channel::BaseColor));
    assert_eq!(flow_levels_at(&mut session, 0.5, 0.5), [FLOW_STILL_LEVEL; 2]);
}
