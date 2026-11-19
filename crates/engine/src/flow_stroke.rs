//! Strokes that paint their own direction into the flow map: when a stroke does, and how its
//! flow map part is staged, committed and previewed apart from the channels that share the
//! stroke alpha.

use std::collections::BTreeMap;

use texturecraft_compose::PaintTarget;
use texturecraft_core::{Channel, flow_to_polar};
use texturecraft_paint::{CommitSource, FlowProducer};
use texturecraft_texel::TileImage;

use crate::session::{ActiveStroke, Session, Tool, ToolState};

/// Whether `tool` makes strokes that can paint their own direction into the flow map. Paint,
/// Projection and stroked paths can; a fill has no direction of its own, the eraser removes
/// paint, and clone and smudge carry the flow they picked up.
pub fn can_follow_stroke(tool: &ToolState) -> bool {
    match tool.tool {
        Tool::Paint | Tool::Projection => true,
        Tool::Path => !(tool.path.closed && tool.path.fill),
        Tool::Eraser | Tool::PolygonFill | Tool::Smudge | Tool::Clone | Tool::ColorPicker | Tool::GeometryMask => false,
    }
}

/// Whether a stroke of `tool` paints the direction it travels into the flow map, instead of
/// the brush's fixed direction.
pub fn follows_stroke(tool: &ToolState) -> bool {
    tool.flow_follow && can_follow_stroke(tool)
}

/// Whether the tool's paint reaches a flow map: the channel is on in the brush's material, the
/// active texture set has it and so does the layer that would be painted. Flow settings are
/// stored with a path or a brush only then, so files that do not use the flow map never name it.
pub(crate) fn paints_flow(session: &Session) -> bool {
    let project = &session.doc.project;
    let on_set = project.texture_set(session.ts()).is_ok_and(|set| set.has_channel(Channel::Flow));
    let on_layer = session.selected_layer().is_some_and(|selected| {
        let owner = project.effect(selected).map_or(selected, |(owner, _)| owner);
        let painted = project.content_layer(owner).map_or(owner, |layer| layer.id);
        project.layer(painted).is_ok_and(|layer| layer.channels.contains_key(&Channel::Flow))
    });
    session.tool.enabled.contains(&Channel::Flow) && on_set && on_layer
}

/// The flow map's own producer for a stroke that starts now on `target` painting `values`
/// (the stroke's channels) at `size` texels with dabs `spacing` diameters apart. None when the
/// stroke paints the flow map like any other channel, or not at all.
pub(crate) fn producer(
    session: &Session,
    target: Option<PaintTarget>,
    values: &BTreeMap<Channel, [f32; 3]>,
    size: (u32, u32),
    spacing: f32,
) -> Option<FlowProducer> {
    let fixed = values.get(&Channel::Flow)?;
    if !follows_stroke(&session.tool) || !matches!(target, Some(PaintTarget::Layer(_))) {
        return None;
    }

    // The speed control sets the length of the fixed vector: a followed stroke paints that length
    let (_, speed) = flow_to_polar(*fixed);
    Some(FlowProducer::new(size.0, size.1, speed, spacing))
}

/// The channels of a stroke's `values` that share the stroke alpha: all of them, or all but
/// the flow map when it has its own producer (`own_flow`).
pub(crate) fn shared_values(values: &BTreeMap<Channel, [f32; 3]>, own_flow: bool) -> BTreeMap<Channel, [f32; 3]> {
    values.iter().filter(|(channel, _)| !(own_flow && **channel == Channel::Flow)).map(|(channel, value)| (*channel, *value)).collect()
}

/// Run `part` once for each part of layer stroke `stroke` that has its own source: the channels
/// sharing the stroke alpha with `shared`, then the flow map with its producer's coverage and
/// values. Staging and commit both go through here, so what is staged is what is committed.
pub(crate) fn layer_parts<'a>(stroke: &'a ActiveStroke, shared: CommitSource<'a>, mut part: impl FnMut(&BTreeMap<Channel, [f32; 3]>, CommitSource<'a>)) {
    let Some(flow) = &stroke.canvas.flow else { return part(&stroke.values, shared) };

    let rest = shared_values(&stroke.values, true);
    if !rest.is_empty() {
        part(&rest, shared);
    }
    if let Some(fixed) = stroke.values.get(&Channel::Flow) {
        let own = [(Channel::Flow, *fixed)].into_iter().collect();
        part(&own, CommitSource { alpha: &flow.coverage, carried: Some(&flow.values), ..shared });
    }
}

/// Whether `stroke` ends without anything to commit although it touched texels: it paints the
/// flow map only, and no dab of it had a direction (a click) or every direction cancelled.
pub(crate) fn deposited_nothing(stroke: &ActiveStroke) -> bool {
    let only_flow = stroke.values.keys().all(|channel| *channel == Channel::Flow);
    stroke.canvas.flow.as_ref().is_some_and(|flow| only_flow && !flow.has_coverage())
}

/// The live preview's per-channel coverage for `stroke`: the flow map's own, when it has one.
pub(crate) fn preview_coverage(stroke: &ActiveStroke) -> BTreeMap<Channel, TileImage> {
    stroke.canvas.flow.iter().map(|flow| (Channel::Flow, flow.coverage.clone())).collect()
}

/// `carried` (what a pickup carries for the live preview) with the flow map values of
/// `stroke`'s own producer, when it has one.
pub(crate) fn preview_carried(stroke: &ActiveStroke, mut carried: BTreeMap<Channel, TileImage>) -> BTreeMap<Channel, TileImage> {
    if let Some(flow) = &stroke.canvas.flow {
        carried.extend(flow.values.iter().map(|(channel, values)| (*channel, values.clone())));
    }
    carried
}

/// The brush's flow value to put back after the material picker ran, when strokes follow their
/// direction: the picker would otherwise set the brush's speed to the flow under the cursor,
/// and picking a colour from still water would leave a brush that paints no flow at all.
pub(crate) fn kept_from_picker(tool: &ToolState) -> Option<[f32; 3]> {
    tool.flow_follow.then(|| tool.values.get(&Channel::Flow).copied()).flatten()
}
