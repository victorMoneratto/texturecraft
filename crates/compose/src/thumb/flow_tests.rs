//! Thumbnails of the flow map show the colours of the raw view and of an 8-bit export

use texturecraft_core::{Channel, FLOW_STILL, Source, flow_from_polar};

use super::*;
use crate::eval::vector_ops_tests::flow_fill;
use crate::flow_view_texel;

/// The thumbnail's colour of a fill layer's flow map where the whole layer holds `value`
fn fill_thumbnail(value: [f32; 3]) -> [u8; 4] {
    let fixture = flow_fill(Source::Uniform(value));
    let thumbnail = fixture.with_inputs(|inputs| render(inputs, ThumbTarget::Content { layer: fixture.layer, channel: Channel::Flow })).unwrap();

    let texel = [thumbnail.rgba[0], thumbnail.rgba[1], thumbnail.rgba[2], thumbnail.rgba[3]];
    assert!(thumbnail.rgba.as_chunks::<4>().0.iter().all(|other| *other == texel), "a uniform fill has one colour");
    texel
}

#[test]
fn a_flow_thumbnail_shows_the_colours_of_the_raw_view() {
    // Full speed to the left is the lowest level a flow reaches: 1, not 0
    assert_eq!(fill_thumbnail(flow_from_polar(180.0, 1.0)), [1, 128, 0, 255]);
    assert_eq!(fill_thumbnail(flow_from_polar(0.0, 1.0)), [255, 128, 0, 255]);
    assert_eq!(fill_thumbnail(FLOW_STILL), [128, 128, 0, 255]);

    for (angle, speed) in [(30.0, 0.5), (135.0, 0.25), (250.0, 0.9), (300.0, 0.1)] {
        let value = flow_from_polar(angle, speed);
        let shown = flow_view_texel(&value.map(texturecraft_core::flow_stored_level));
        assert_eq!(fill_thumbnail(value), shown, "{angle} degrees at {speed}");
    }
}

#[test]
fn a_flow_thumbnail_keeps_blue_empty_and_coverage_in_alpha() {
    let east = flow_from_polar(0.0, 1.0);

    assert_eq!(fill_thumbnail([east[0], east[1], 0.9]), [255, 128, 0, 255]);
}

#[test]
fn a_folders_flow_thumbnail_shows_the_same_colours() {
    use texturecraft_core::{Fill, Layer, LayerKind};

    let mut fixture = flow_fill(Source::Uniform(FLOW_STILL));
    let folder = fixture.project.alloc_id();
    let mut group = Layer::new(folder, "Folder", LayerKind::Group { children: Vec::new(), collapsed: false }, &[]);
    group.channels.insert(Channel::Flow, ChannelBlend { mode: BlendMode::Passthrough, opacity: 1.0 });
    fixture.project.insert_layer(0, group, None, false).unwrap();

    let inside = fixture.project.alloc_id();
    let mut fill = Fill::default();
    fill.sources.insert(Channel::Flow, Source::Uniform(flow_from_polar(180.0, 1.0)));
    fixture.project.insert_layer(0, Layer::new(inside, "Inside", LayerKind::Fill(fill), &[Channel::Flow]), Some(folder), true).unwrap();

    let thumbnail = fixture.with_inputs(|inputs| render(inputs, ThumbTarget::Content { layer: folder, channel: Channel::Flow })).unwrap();

    assert_eq!(thumbnail.rgba[..4], [1, 128, 0, 255]);
}
