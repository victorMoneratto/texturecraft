//! One headless renderer used for render after render: a view that reads the aux texture never
//! shows what an earlier view left in it

use serde_json::{Value, json};
use texturecraft_automation::render::Gpu;
use texturecraft_engine::geom::glam::Vec2;
use texturecraft_engine::geom::primitives;
use texturecraft_engine::{Session, execute};
use texturecraft_render::ViewKind;

const RESOLUTION: u32 = 256;

/// Side of the square renders, in pixels
const VIEW: u32 = 200;

/// What a texture set shows in the mask view when the mask belongs to another set
const NOT_THIS_SET: [u8; 3] = [38, 38, 38];

/// A white mask in the mask view
const WHITE: [u8; 3] = [255, 255, 255];

/// A mesh with two materials, so two texture sets, each with the flow map; set 1 flows right
/// everywhere and set 0 has a layer with a white mask
fn two_set_session() -> (Session, u64) {
    let mut session = Session::new_project("Aux", primitives::material_ball(), "builtin:material_ball", None, RESOLUTION).unwrap();
    assert_eq!(session.doc.project.texture_sets.len(), 2);
    for set in [1, 0] {
        execute(&mut session, "textureSet.select", &json!({"ts": set})).unwrap();
        execute(&mut session, "textureSet.channel.add", &json!({"channel": "flow"})).unwrap();
        execute(&mut session, "layer.add.fill", &Value::Null).unwrap();
        execute(&mut session, "layer.fill.set", &json!({"channel": "flow", "angle": 0.0, "speed": 1.0})).unwrap();
    }
    let masked = session.selected_layer().unwrap().0;
    execute(&mut session, "mask.add.white", &json!({"layer": masked})).unwrap();
    (session, masked)
}

fn gpu() -> Option<Gpu> {
    match Gpu::new() {
        Ok(gpu) => Some(gpu),
        Err(error) => {
            eprintln!("skipping: {error}");
            None
        }
    }
}

/// The colours the 3D view shows on texture set `set`, most frequent first. Only pixels well
/// inside the set's surface count: an edge pixel mixes two surfaces
fn colours_3d(gpu: &mut Gpu, session: &mut Session, set: usize) -> Vec<([u8; 3], usize)> {
    let camera = session.camera;
    let viewport = Vec2::splat(VIEW as f32);
    let set_at = |x: i64, y: i64| {
        let hit = session.pick(Vec2::new(x as f32 + 0.5, y as f32 + 0.5), viewport)?;
        session.texture_set_of_material(hit.material)
    };
    let inside: Vec<(u32, u32)> = (EDGE..VIEW as i64 - EDGE)
        .step_by(3)
        .flat_map(|y| (EDGE..VIEW as i64 - EDGE).step_by(3).map(move |x| (x, y)))
        .filter(|&(x, y)| [(0, 0), (-EDGE, 0), (EDGE, 0), (0, -EDGE), (0, EDGE)].iter().all(|(dx, dy)| set_at(x + dx, y + dy) == Some(set)))
        .map(|(x, y)| (x as u32, y as u32))
        .collect();
    assert!(inside.len() > 50, "the camera sees texture set {set}: {} pixels", inside.len());

    let (width, _, pixels) = gpu.render(session, ViewKind::Scene, [VIEW, VIEW], camera).unwrap();
    let mut counts = std::collections::BTreeMap::new();
    for (x, y) in inside {
        let start = ((y * width + x) * 4) as usize;
        *counts.entry([pixels[start], pixels[start + 1], pixels[start + 2]]).or_insert(0usize) += 1;
    }
    let mut colours: Vec<([u8; 3], usize)> = counts.into_iter().collect();
    colours.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    colours
}

/// How far, in pixels, a sampled pixel keeps from another surface
const EDGE: i64 = 3;

/// Whether nearly every sampled texel of the view is `colour`, to within one level
fn shows_only(colours: &[([u8; 3], usize)], colour: [u8; 3]) -> bool {
    let total: usize = colours.iter().map(|(_, count)| count).sum();
    let matching: usize = colours.iter().filter(|(c, _)| c.iter().zip(colour).all(|(a, b)| a.abs_diff(b) <= 1)).map(|(_, count)| count).sum();
    matching * 100 >= total * 97
}

fn has_colour_near(colours: &[([u8; 3], usize)], colour: [u8; 3]) -> bool {
    colours.iter().any(|(c, _)| c.iter().zip(colour).all(|(a, b)| a.abs_diff(b) <= 1))
}

#[test]
fn a_mask_render_after_a_flow_render_shows_no_flow_on_the_other_sets() {
    let Some(mut gpu) = gpu() else { return };
    let (mut session, masked) = two_set_session();

    execute(&mut session, "viewport.view", &json!({"mode": "flow"})).unwrap();
    let flowing = colours_3d(&mut gpu, &mut session, 1);
    assert!(has_colour_near(&flowing, [255, 128, 0]), "set 1 shows its flow first: {:?}", &flowing[..flowing.len().min(4)]);

    // The same renderer, now showing a mask of set 0
    execute(&mut session, "viewport.view", &json!({"mode": "mask", "layer": masked})).unwrap();
    let owner = colours_3d(&mut gpu, &mut session, 0);
    let other = colours_3d(&mut gpu, &mut session, 1);

    assert!(!has_colour_near(&other, [255, 128, 0]), "set 1 still shows the flow map: {:?}", &other[..other.len().min(4)]);
    assert!(has_colour_near(&owner, WHITE), "set 0 shows its mask: {:?}", &owner[..owner.len().min(4)]);
    assert!(shows_only(&other, NOT_THIS_SET), "set 1 shows what the app shows for a set without the mask: {:?}", &other[..other.len().min(4)]);
}

#[test]
fn a_mask_render_after_a_mesh_map_render_shows_no_mesh_map_on_the_other_sets() {
    let Some(mut gpu) = gpu() else { return };
    let (mut session, masked) = two_set_session();

    // Nothing is baked: the view shows the map's neutral colour, a flat normal
    execute(&mut session, "viewport.view", &json!({"mode": "map:normal"})).unwrap();
    let neutral = colours_3d(&mut gpu, &mut session, 1);
    let (map_colour, _) = neutral[0];
    assert!(map_colour != NOT_THIS_SET, "{neutral:?}");

    execute(&mut session, "viewport.view", &json!({"mode": "mask", "layer": masked})).unwrap();
    let other = colours_3d(&mut gpu, &mut session, 1);

    assert!(shows_only(&other, NOT_THIS_SET), "set 1 after the mesh map view: {:?}", &other[..other.len().min(4)]);
}

#[test]
fn a_mask_view_of_a_layer_that_lost_its_mask_shows_nothing_stale() {
    let Some(mut gpu) = gpu() else { return };
    let (mut session, masked) = two_set_session();

    execute(&mut session, "viewport.view", &json!({"mode": "flow"})).unwrap();
    colours_3d(&mut gpu, &mut session, 0);
    execute(&mut session, "viewport.view", &json!({"mode": "mask", "layer": masked})).unwrap();
    execute(&mut session, "mask.toggle", &json!({"layer": masked, "enabled": false})).unwrap();

    let owner = colours_3d(&mut gpu, &mut session, 0);
    assert!(!has_colour_near(&owner, [255, 128, 0]), "set 0 still shows the flow map: {:?}", &owner[..owner.len().min(4)]);
}
