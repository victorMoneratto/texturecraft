//! What every effect the compositor can apply to layer content does to a packed vector channel
//! (the flow map), and the rules that hold whatever the effect: still water stays exactly
//! still, the third component stays empty, and the channel blends in Normal mode

use texturecraft_core::{
    ChannelBlend, ChannelFormat, CompareOp, Effect, FLOW_STILL, FLOW_STILL_LEVEL, GeneratorKind, Layer, Levels, Procedural, ProceduralKind, TextureSet,
    flow_from_polar, flow_reversed, flow_stored_level,
};
use texturecraft_geom::primitives;

use super::*;

const SIDE: u32 = 64;

/// What an effect does to a vector channel
#[derive(Clone, Copy, Debug, PartialEq)]
enum OnVector {
    /// Leaves every texel as it was: the effect has no meaning on a direction
    PassThrough,
    /// Moves or averages texels: a uniform field stays as it was, a varied one changes
    Rearranges,
    /// Reverses the flow about still water
    Reverses,
    /// Keeps what differs from the surroundings: a uniform field becomes still
    KeepsDetail,
}

/// The rule for each filter. No catch-all arm: a new filter does not compile until it is given
/// a behaviour here
fn filter_on_vector(filter: &Filter) -> OnVector {
    match filter {
        Filter::Blur { .. }
        | Filter::Sharpen { .. }
        | Filter::Warp { .. }
        | Filter::DirectionalBlur { .. }
        | Filter::SlopeBlur { .. }
        | Filter::DirectionalWarp { .. }
        | Filter::Mosaic { .. } => OnVector::Rearranges,
        Filter::Invert => OnVector::Reverses,
        Filter::HighPass { .. } => OnVector::KeepsDetail,
        Filter::Threshold { .. }
        | Filter::Hsl { .. }
        | Filter::ContrastLuminosity { .. }
        | Filter::ColorBalance { .. }
        | Filter::GradientMap { .. }
        | Filter::Posterize { .. }
        | Filter::Clamp { .. }
        | Filter::HistogramScan { .. }
        | Filter::HistogramRange { .. }
        | Filter::HistogramShift { .. }
        | Filter::EdgeDetect { .. }
        | Filter::Grayscale
        | Filter::NormalInvert
        | Filter::Bevel { .. }
        | Filter::Glow { .. }
        | Filter::BakedLighting { .. }
        | Filter::ReplaceColor { .. } => OnVector::PassThrough,
    }
}

/// One of every filter, with settings that visibly change an ordinary channel
fn every_filter() -> Vec<Filter> {
    vec![
        Filter::Blur { radius: 64.0 },
        Filter::Sharpen { amount: 2.0 },
        Filter::Invert,
        Filter::Threshold { level: 0.6 },
        Filter::Hsl { hue: 0.3, saturation: 0.5, lightness: -0.4 },
        Filter::ContrastLuminosity { contrast: 0.6, luminosity: 0.2 },
        Filter::ColorBalance { shadows: [0.3, -0.2, 0.4], midtones: [0.2, 0.3, 0.5], highlights: [-0.3, 0.2, 0.6] },
        Filter::GradientMap { colors: vec![[0.02, 0.02, 0.04], [0.45, 0.16, 0.05], [1.0, 0.85, 0.55]] },
        Filter::Posterize { levels: 3.0 },
        Filter::Clamp { min: 0.6, max: 0.7 },
        Filter::HistogramScan { position: 0.3, contrast: 0.5 },
        Filter::HistogramRange { range: 0.5, position: 0.2 },
        Filter::HistogramShift { offset: 0.25 },
        Filter::Warp { intensity: 256.0, scale: 4.0, seed: 3 },
        Filter::DirectionalBlur { radius: 64.0, angle: 30.0 },
        Filter::SlopeBlur { radius: 64.0, scale: 4.0, seed: 3 },
        Filter::HighPass { radius: 64.0 },
        Filter::EdgeDetect { width: 64.0 },
        Filter::DirectionalWarp { intensity: 256.0, angle: 90.0, scale: 4.0, seed: 3 },
        Filter::Grayscale,
        Filter::NormalInvert,
        Filter::Bevel { width: 128.0 },
        Filter::Mosaic { size: 768.0 },
        Filter::Glow { radius: 32.0, amount: 1.0 },
        Filter::BakedLighting { azimuth: 45.0, elevation: 45.0, light: 0.8, ambient: 0.4, occlusion: 1.0 },
        Filter::ReplaceColor { from: [0.5, 0.5, 0.0], to: [0.05, 0.15, 0.5], tolerance: 0.9, softness: 0.1 },
    ]
}

/// The rule for each effect kind in a content stack; filters by [`filter_on_vector`]. A paint,
/// path or material effect lays its own content over the layer's and is tested on its own
fn effect_on_vector(kind: &EffectKind) -> Option<OnVector> {
    match kind {
        EffectKind::Filter(filter) => Some(filter_on_vector(filter)),
        EffectKind::Levels(_)
        | EffectKind::Fill { .. }
        | EffectKind::Generator(_)
        | EffectKind::ColorSelection { .. }
        | EffectKind::Compare { .. }
        | EffectKind::Anchor => Some(OnVector::PassThrough),
        EffectKind::Paint | EffectKind::Path(_) | EffectKind::Material(_) => None,
    }
}

/// One of every effect that changes content in place or passes it on
fn every_effect() -> Vec<EffectKind> {
    let mut effects: Vec<EffectKind> = every_filter().into_iter().map(EffectKind::Filter).collect();
    effects.extend([
        EffectKind::Levels(Levels { in_low: 0.2, in_high: 0.7, gamma: 2.0, out_low: 0.1, out_high: 0.9 }),
        EffectKind::Fill { source: Source::Uniform([0.3; 3]), projection: Projection::default() },
        EffectKind::Generator(Generator::new(GeneratorKind::Dirt)),
        EffectKind::ColorSelection { colors: vec![[1.0, 0.0, 0.0]], tolerance: 0.5 },
        EffectKind::Compare { op: CompareOp::GreaterThan, value: 0.3, tolerance: 0.1 },
        EffectKind::Anchor,
    ]);
    effects
}

pub(crate) struct Fixture {
    pub(crate) project: Project,
    mesh: Mesh,
    gbuf: UvGBuffer,
    pixels: PixelStore,
    pub(crate) layer: LayerId,
}

/// A plane whose set has the flow map and one fill layer writing `source` into it everywhere
pub(crate) fn flow_fill(source: Source) -> Fixture {
    let mesh = primitives::plane(2.0, 2);
    let gbuf = UvGBuffer::new(&mesh, 0, SIDE, SIDE);
    let mut project = Project::new("t", "builtin:plane");
    let mut set = TextureSet::new("Plane", 0, SIDE);
    set.channels.push((Channel::Flow, ChannelFormat::default_for(Channel::Flow)));
    project.texture_sets.push(set);

    let layer = project.alloc_id();
    let mut fill = Fill::default();
    fill.sources.insert(Channel::Flow, source);
    let mut fill_layer = Layer::new(layer, "Flow", LayerKind::Fill(fill), &[]);
    fill_layer.channels.insert(Channel::Flow, ChannelBlend::default());
    let layer = project.insert_layer(0, fill_layer, None, false).unwrap();
    Fixture { project, mesh, gbuf, pixels: PixelStore::default(), layer }
}

impl Fixture {
    pub(crate) fn with_inputs<R>(&self, read: impl FnOnce(&Inputs) -> R) -> R {
        let (images, baked) = (ImageStore::default(), BakedMaps::default());
        read(&Inputs {
            project: &self.project,
            ts: 0,
            mesh: &self.mesh,
            gbuf: &self.gbuf,
            pixels: &self.pixels,
            baked: &baked,
            block_scales: self.project.texture_sets[0].block_scales(),
            preview: None,
            images: &images,
            bounds: self.mesh.bounds(),
        })
    }

    /// The composited flow map: the three stored levels of every texel, row by row
    fn flow(&self) -> Vec<[u16; 3]> {
        self.with_inputs(|inputs| {
            let mut out = Composite::new(&self.project.texture_sets[0].channel_list(), SIDE);
            composite_tiles(inputs, &mut out, &all_tiles(SIDE, SIDE));
            let image = &out.channels[&Channel::Flow];
            (0..SIDE * SIDE).map(|i| image.get(i % SIDE, i / SIDE)).map(|texel| [texel[0], texel[1], texel[2]]).collect()
        })
    }

    fn add_effect(&mut self, kind: EffectKind) -> LayerId {
        let id = self.project.alloc_id();
        self.project.layer_mut(self.layer).unwrap().content_effects.push(Effect::new(id, kind));
        id
    }

    fn remove_effect(&mut self, id: LayerId) {
        self.project.layer_mut(self.layer).unwrap().content_effects.retain(|effect| effect.id != id);
    }
}

/// The stored levels of a packed value
fn levels(packed: [f32; 3]) -> [u16; 3] {
    packed.map(flow_stored_level)
}

const STILL_LEVELS: [u16; 3] = [FLOW_STILL_LEVEL, FLOW_STILL_LEVEL, 0];

/// A field of two opposite diagonal flows in a checker, so moving or averaging texels shows
fn varied_field() -> Source {
    Source::Procedural {
        pattern: Procedural::new(ProceduralKind::Checker),
        low: flow_from_polar(200.0, 0.75),
        high: flow_from_polar(20.0, 0.75),
        ramp: Vec::new(),
    }
}

#[test]
fn every_effect_keeps_still_water_still_and_blue_empty_and_does_what_its_rule_says() {
    let moving = flow_from_polar(30.0, 0.6);

    for kind in every_effect() {
        let rule = effect_on_vector(&kind).unwrap();
        let name = format!(
            "{} ({rule:?})",
            match &kind {
                EffectKind::Filter(filter) => filter.label(),
                other => other.name(),
            }
        );

        // A still field stays exactly still under every effect
        let mut still = flow_fill(Source::Uniform(FLOW_STILL));
        still.add_effect(kind.clone());
        let texels = still.flow();
        assert_eq!(texels.iter().find(|texel| **texel != STILL_LEVELS), None, "{name}: still water moved");

        // A uniform moving field
        let mut uniform = flow_fill(Source::Uniform(moving));
        let before = uniform.flow();
        assert!(before.iter().all(|texel| *texel == levels(moving)));
        uniform.add_effect(kind.clone());
        let after = uniform.flow();
        let expected = match rule {
            OnVector::PassThrough | OnVector::Rearranges => levels(moving),
            OnVector::Reverses => levels(flow_reversed(moving)),
            OnVector::KeepsDetail => STILL_LEVELS,
        };
        assert_eq!(after.iter().find(|texel| **texel != expected), None, "{name}: a uniform flow of {:?} should become {expected:?}", levels(moving));

        // A field that varies: pass-through effects change nothing, the others do their work
        let mut varied = flow_fill(varied_field());
        let before = varied.flow();
        varied.add_effect(kind.clone());
        let after = varied.flow();
        assert!(after.iter().all(|texel| texel[2] == 0), "{name}: blue is not empty");
        match rule {
            OnVector::PassThrough => assert!(after == before, "{name}: changed a vector it has no meaning on"),
            OnVector::Reverses => {
                let mirrored = |a: u16, b: u16| u32::from(a) + u32::from(b) == 2 * u32::from(FLOW_STILL_LEVEL);
                assert!(after.iter().zip(&before).all(|(a, b)| mirrored(a[0], b[0]) && mirrored(a[1], b[1])), "{name}: not the reverse flow");
                assert!(after != before);
            }
            OnVector::Rearranges | OnVector::KeepsDetail => assert!(after != before, "{name}: left a varied field as it was"),
        }
    }
}

#[test]
fn an_effect_at_part_opacity_keeps_still_water_still() {
    for kind in every_effect() {
        let name = kind.name();
        let mut still = flow_fill(Source::Uniform(FLOW_STILL));
        let effect = still.add_effect(kind);
        still.project.layer_mut(still.layer).unwrap().content_effects.iter_mut().find(|e| e.id == effect).unwrap().opacity = 0.37;

        assert_eq!(still.flow().iter().find(|texel| **texel != STILL_LEVELS), None, "{name}");
    }
}

#[test]
fn reversing_twice_gives_the_flow_back() {
    let mut field = flow_fill(varied_field());
    let before = field.flow();

    let first = field.add_effect(EffectKind::Filter(Filter::Invert));
    field.add_effect(EffectKind::Filter(Filter::Invert));
    assert_eq!(field.flow(), before);

    field.remove_effect(first);
    assert_ne!(field.flow(), before);
}

#[test]
fn blue_stays_empty_whatever_a_source_or_a_stored_texel_holds() {
    let east = flow_from_polar(0.0, 1.0);
    let with_blue = |value: [f32; 3]| [value[0], value[1], 0.8];

    // A uniform value, a pattern between two values and an anchor of another channel
    let mut sources = vec![
        ("uniform", flow_fill(Source::Uniform(with_blue(east)))),
        (
            "procedural",
            flow_fill(Source::Procedural {
                pattern: Procedural::new(ProceduralKind::Checker),
                low: with_blue(FLOW_STILL),
                high: with_blue(east),
                ramp: Vec::new(),
            }),
        ),
    ];
    let mut anchored = flow_fill(Source::Uniform(east));
    let below = anchored.project.alloc_id();
    let mut base = Fill::default();
    base.sources.insert(Channel::BaseColor, Source::Uniform([0.9, 0.2, 0.7]));
    let mut base_layer = Layer::new(below, "Base", LayerKind::Fill(base), &[Channel::BaseColor]);
    let anchor = anchored.project.alloc_id();
    base_layer.content_effects.push(Effect::new(anchor, EffectKind::Anchor));
    anchored.project.insert_layer(0, base_layer, None, false).unwrap();
    let stack = &mut anchored.project.texture_sets[0].stack;
    stack.reverse();
    assert_eq!(stack.first(), Some(&below), "the anchored layer is below the flow fill");
    if let LayerKind::Fill(fill) = &mut anchored.project.layer_mut(anchored.layer).unwrap().kind {
        fill.sources.insert(Channel::Flow, Source::Anchor { anchor, channel: Channel::BaseColor, ramp: Vec::new() });
    }
    sources.push(("anchor", anchored));

    for (name, mut fixture) in sources {
        let texels = fixture.flow();
        assert!(texels.iter().any(|texel| texel[0] != FLOW_STILL_LEVEL), "{name}: the source reaches the channel");
        assert!(texels.iter().all(|texel| texel[2] == 0), "{name}: blue {:?}", texels.iter().find(|texel| texel[2] != 0));

        // What an anchor above records, and so what a mask or fill reading it gets
        fixture.add_effect(EffectKind::Filter(Filter::HighPass { radius: 64.0 }));
        fixture.add_effect(EffectKind::Anchor);
        assert!(fixture.flow().iter().all(|texel| texel[2] == 0), "{name} after a high pass");

        // Flattened to a paint layer
        let flat = fixture.with_inputs(|inputs| flatten_layer(inputs, fixture.layer, &all_tiles(SIDE, SIDE))).unwrap();
        let flow_tiles: Vec<&FlatTile> = flat.iter().filter(|(channel, ..)| *channel == Channel::Flow).collect();
        assert!(!flow_tiles.is_empty(), "{name}: flattening keeps the channel");
        for (_, _, data) in flow_tiles {
            assert!(data.chunks_exact(4).all(|texel| texel[2] == 0), "{name}: flattened blue");
        }
    }

    // Painted texels that hold a blue, as a file or an older build could have stored them
    let mut painted = flow_fill(Source::Uniform(FLOW_STILL));
    let paint = painted.add_effect(EffectKind::Paint);
    let image = painted.pixels.layer_mut(paint, Channel::Flow, (SIDE, SIDE));
    for (x, y) in (0..SIDE).flat_map(|y| (0..SIDE).map(move |x| (x, y))) {
        image.set(x, y, &[u16::MAX, FLOW_STILL_LEVEL, 40000, u16::MAX]);
    }
    assert!(painted.flow().iter().all(|texel| *texel == [u16::MAX, FLOW_STILL_LEVEL, 0]));
}

#[test]
fn a_stored_blend_mode_never_changes_how_a_vector_channel_composites() {
    let (west, north) = (flow_from_polar(180.0, 0.5), flow_from_polar(90.0, 1.0));

    // A soft stroke (coverage rising from nothing to full across the image) on a paint layer at
    // part opacity over a fill, with a material effect laid over the paint at part opacity
    let build = |layer_mode: BlendMode, effect_mode: BlendMode| {
        let mut fixture = flow_fill(Source::Uniform(west));
        let top = fixture.project.alloc_id();
        let mut top_layer = Layer::new(top, "Top", LayerKind::Paint, &[]);
        top_layer.channels.insert(Channel::Flow, ChannelBlend { mode: layer_mode, opacity: 0.5 });

        let stroke = fixture.pixels.layer_mut(top, Channel::Flow, (SIDE, SIDE));
        for (x, y) in (0..SIDE).flat_map(|y| (0..SIDE).map(move |x| (x, y))) {
            let coverage = x as f32 / (SIDE - 1) as f32;
            let premultiplied = north.map(|component| texturecraft_texel::to_u16(component * coverage));
            stroke.set(x, y, &[premultiplied[0], premultiplied[1], 0, texturecraft_texel::to_u16(coverage)]);
        }

        let mut material = Fill::default();
        material.sources.insert(Channel::Flow, Source::Uniform(flow_from_polar(315.0, 0.8)));
        let mut effect = Effect::new(fixture.project.alloc_id(), EffectKind::Material(material));
        (effect.blend, effect.opacity) = (effect_mode, 0.4);
        top_layer.content_effects.push(effect);
        fixture.project.insert_layer(0, top_layer, None, false).unwrap();
        fixture.flow()
    };
    let normal = build(BlendMode::Normal, BlendMode::Normal);
    let row: Vec<[u16; 3]> = normal.iter().take(SIDE as usize).copied().collect();
    assert!(row.windows(2).all(|pair| pair[0] != pair[1]), "the soft edge shows as a ramp: {row:?}");

    for mode in BlendMode::MENU.into_iter().flatten().filter(|mode| *mode != BlendMode::Disable) {
        let (on_layer, on_effect) = (build(mode, BlendMode::Normal), build(BlendMode::Normal, mode));
        let differs = |texels: &[[u16; 3]]| texels.iter().zip(&normal).position(|(a, b)| a != b).map(|at| (at, texels[at], normal[at]));
        assert_eq!(differs(&on_layer), None, "{mode:?} on the layer");
        assert_eq!(differs(&on_effect), None, "{mode:?} on the effect");
    }
}
