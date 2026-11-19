//! A live stroke whose flow map has its own coverage: the other channels keep the stroke alpha

use std::collections::BTreeMap;

use texturecraft_compose::{Composite, Inputs, PaintTarget, PixelStore, StrokePreview, all_tiles, composite_tiles};
use texturecraft_core::{Channel, ChannelFormat, FLOW_STILL, Layer, LayerId, LayerKind, Project, TextureSet};
use texturecraft_geom::{UvGBuffer, primitives};
use texturecraft_texel::{TileImage, to_u16};

const RES: u32 = 64;
const RED: [f32; 3] = [1.0, 0.0, 0.0];

/// Flow to the right at full speed, packed
const EAST: [f32; 3] = [1.0, 0.5, 0.0];

/// A plane with one paint layer on a set that has the flow map; returns the composite with `preview`
fn composite(preview: impl FnOnce(LayerId) -> StrokePreview) -> Composite {
    let mesh = primitives::plane(2.0, 4);
    let gbuf = UvGBuffer::new(&mesh, 0, RES, RES);
    let mut project = Project::new("t", "builtin:plane");
    let mut set = TextureSet::new("A", 0, RES);
    set.channels.push((Channel::Flow, ChannelFormat::default_for(Channel::Flow)));
    project.texture_sets.push(set);
    let id = project.alloc_id();
    let channels = project.texture_sets[0].channel_list();
    project.insert_layer(0, Layer::new(id, "Paint", LayerKind::Paint, &channels), None, false).unwrap();

    let preview = preview(id);
    let (pixels, images, baked) = (PixelStore::default(), Default::default(), Default::default());
    let inputs = Inputs {
        project: &project,
        ts: 0,
        mesh: &mesh,
        gbuf: &gbuf,
        pixels: &pixels,
        baked: &baked,
        block_scales: project.texture_sets[0].block_scales(),
        preview: Some(&preview),
        images: &images,
        bounds: mesh.bounds(),
    };
    let mut out = Composite::new(&channels, RES);
    composite_tiles(&inputs, &mut out, &all_tiles(RES, RES));
    out
}

fn close(actual: [f32; 3], expected: [f32; 3]) -> bool {
    actual.iter().zip(expected).all(|(a, e)| (a - e).abs() < 1e-4)
}

/// A stroke over texels (10, 10) and (11, 10) whose flow map only took a direction on the first, at half coverage
fn stroke(target: LayerId, own_coverage: bool) -> StrokePreview {
    let mut alpha = TileImage::new(RES, RES, 1);
    alpha.set(10, 10, &[to_u16(1.0)]);
    alpha.set(11, 10, &[to_u16(1.0)]);
    let mut direction = TileImage::new(RES, RES, 3);
    direction.set(10, 10, &EAST.map(to_u16));
    let mut coverage = TileImage::new(RES, RES, 1);
    coverage.set(10, 10, &[to_u16(0.5)]);

    StrokePreview {
        target: PaintTarget::Layer(target),
        alpha,
        opacity: 1.0,
        erase: false,
        values: [(Channel::BaseColor, RED), (Channel::Flow, FLOW_STILL)].into_iter().collect(),
        mask_value: 1.0,
        stencil: None,
        carried: [(Channel::Flow, direction)].into_iter().collect(),
        carried_mask: None,
        coverage: if own_coverage { [(Channel::Flow, coverage)].into_iter().collect() } else { BTreeMap::new() },
    }
}

#[test]
fn a_channel_with_its_own_coverage_ignores_the_stroke_alpha() {
    let out = composite(|layer| stroke(layer, true));

    // The stroke alpha paints the other channel on both texels
    assert!(close(out.value(Channel::BaseColor, 10, 10), RED));
    assert!(close(out.value(Channel::BaseColor, 11, 10), RED));

    // The flow map follows its own coverage: half way to the direction, and nothing beside it
    assert!(close(out.value(Channel::Flow, 10, 10), [0.75, 0.5, 0.0]), "{:?}", out.value(Channel::Flow, 10, 10));
    assert!(close(out.value(Channel::Flow, 11, 10), FLOW_STILL), "{:?}", out.value(Channel::Flow, 11, 10));
    assert!(close(out.value(Channel::Flow, 30, 30), FLOW_STILL));
}

#[test]
fn without_its_own_coverage_a_channel_shares_the_stroke_alpha() {
    let out = composite(|layer| stroke(layer, false));

    // The stroke alpha is full on both texels: the carried direction, and the carried nothing
    assert!(close(out.value(Channel::Flow, 10, 10), EAST));
    assert!(close(out.value(Channel::Flow, 11, 10), [0.0, 0.0, 0.0]));
}

#[test]
fn the_stroke_opacity_caps_a_channels_own_coverage() {
    let out = composite(|layer| StrokePreview { opacity: 0.5, ..stroke(layer, true) });

    // Half the coverage at half the opacity: a quarter of the way to the direction
    assert!(close(out.value(Channel::Flow, 10, 10), [0.625, 0.5, 0.0]), "{:?}", out.value(Channel::Flow, 10, 10));
}
