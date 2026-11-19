//! The raw flow map view through the desktop app's GPU link: what each frame sends to the aux
//! texture, and what the renderer then shows

use serde_json::{Value, json};
use texturecraft_engine::compose::FLOW_VIEW_STILL;
use texturecraft_engine::execute;
use texturecraft_engine::geom::glam::Vec2;
use texturecraft_engine::geom::primitives;
use texturecraft_render::{AuxRegion, Shade, ViewKind};

use super::tests::link;
use super::*;

/// A flow to the right at full speed, as the view and an 8-bit export show it
const RIGHT: [u8; 4] = [255, 128, 0, 255];

/// Bytes of one aux texel
const TEXEL_BYTES: usize = 4;

/// A plane whose UVs fill a `resolution` texture, with the flow map, ready to paint it flowing right
fn flow_session(resolution: u32) -> Session {
    let mut session = Session::new_project("Flow", primitives::plane(2.0, 4), "builtin:plane", None, resolution).unwrap();
    execute(&mut session, "textureSet.channel.add", &json!({"channel": "flow"})).unwrap();
    execute(&mut session, "tool.paint", &Value::Null).unwrap();
    execute(&mut session, "tool.brush", &json!({"size": 4, "hardness": 100})).unwrap();
    execute(&mut session, "tool.material", &json!({"channel": "flow", "angle": 0.0, "speed": 1.0, "solo": true})).unwrap();
    execute(&mut session, "viewport.view", &json!({"mode": "flow"})).unwrap();
    session
}

/// Whole aux images and regions sent over the frames it took to show `view` completely
#[derive(Default)]
struct Sent {
    whole: Vec<(usize, u32, Vec<u8>)>,
    regions: Vec<(usize, AuxRegion)>,
}

impl Sent {
    fn bytes(&self) -> usize {
        self.whole.iter().map(|(_, _, rgba)| rgba.len()).sum::<usize>() + self.regions.iter().map(|(_, region)| region.rgba.len()).sum::<usize>()
    }

    /// The colour the regions sent for aux texel (`x`, `y`) of `texture_set`, the last one winning
    fn region_texel(&self, texture_set: usize, x: u32, y: u32) -> Option<[u8; 4]> {
        let holds = |region: &AuxRegion| (region.x..region.x + region.width).contains(&x) && (region.y..region.y + region.height).contains(&y);
        let (_, region) = self.regions.iter().rev().find(|(set, region)| *set == texture_set && holds(region))?;
        let start = ((y - region.y) * region.width + (x - region.x)) as usize * TEXEL_BYTES;
        region.rgba.get(start..start + TEXEL_BYTES)?.try_into().ok()
    }
}

/// Draw frames of `view` until nothing waits to be composited or uploaded: a frame has a time budget
fn show(gpu: &mut GpuLink, session: &mut Session, view: Option<AuxView>) -> Sent {
    let mut sent = Sent::default();
    loop {
        let mut frame = gpu.sync(session, view, None, 0, 0.0);
        sent.whole.append(&mut frame.aux);
        sent.regions.append(&mut frame.aux_regions);
        if session.pending_tiles() == 0 && session.caches.iter().all(|cache| cache.upload.is_empty()) {
            return sent;
        }
    }
}

fn texel(rgba: &[u8], side: u32, x: u32, y: u32) -> [u8; 4] {
    let start = (y * side + x) as usize * TEXEL_BYTES;
    [rgba[start], rgba[start + 1], rgba[start + 2], rgba[start + 3]]
}

#[test]
fn the_whole_flow_map_goes_up_once() {
    let (mut gpu, mut session) = (link(), flow_session(256));
    session.stroke_uv_points(&[Vec2::new(0.3, 0.5), Vec2::new(0.7, 0.5)]).unwrap();

    let first = show(&mut gpu, &mut session, Some(AuxView::Flow));

    let (set, side, rgba) = first.whole.last().expect("the flow map is sent");
    assert_eq!((*set, *side, rgba.len()), (0, 256, 256 * 256 * TEXEL_BYTES));
    assert_eq!(texel(rgba, 256, 128, 128), RIGHT, "the stroke");
    assert_eq!(texel(rgba, 256, 128, 230), FLOW_VIEW_STILL, "away from the stroke");

    let later = show(&mut gpu, &mut session, Some(AuxView::Flow));
    assert_eq!(later.bytes(), 0, "nothing changed: nothing is sent");
}

#[test]
fn a_stroke_in_progress_sends_only_the_tiles_it_changes() {
    const SIDE: u32 = 2048;
    let (mut gpu, mut session) = (link(), flow_session(SIDE));
    let whole = show(&mut gpu, &mut session, Some(AuxView::Flow)).whole.last().map_or(0, |(_, _, rgba)| rgba.len());
    assert_eq!(whole, (SIDE * SIDE) as usize * TEXEL_BYTES);

    // A short stroke of a small brush in the middle of one tile, not yet released
    execute(&mut session, "tool.brush", &json!({"size": 1})).unwrap();
    let (middle, tile_side) = (SIDE / 2 + 32, 64);
    let at = |texel: u32| texel as f32 / SIDE as f32;
    session.begin_stroke_uv(Vec2::new(at(middle), at(middle)), 1.0).unwrap();
    session.continue_stroke_uv(Vec2::new(at(middle + 8), at(middle)), 1.0);
    let during = show(&mut gpu, &mut session, Some(AuxView::Flow));

    assert!(session.stroke_active());
    assert!(during.whole.is_empty(), "the image is not sent again for a stroke");
    assert_eq!(during.region_texel(0, middle, middle), Some(RIGHT), "the stroke shows before it ends");
    assert!(during.regions.iter().all(|(_, region)| (region.width, region.height) == (tile_side, tile_side)), "one composite tile each");
    let touched = session.stroke_tiles();
    assert!(touched >= 1 && during.regions.len() <= touched * 2, "{} regions for {touched} painted tiles", during.regions.len());
    eprintln!(
        "flow view, one short stroke on a {SIDE} set: {} bytes in {} regions ({touched} painted tiles); the whole image is {whole} bytes",
        during.bytes(),
        during.regions.len()
    );
    assert!(during.bytes() * 100 < whole, "under 1% of the image: {} of {whole} bytes", during.bytes());

    // Releasing it settles the same tiles; undo takes the stroke out of them
    session.end_stroke().unwrap();
    let released = show(&mut gpu, &mut session, Some(AuxView::Flow));
    assert!(released.whole.is_empty() && released.bytes() * 100 < whole);

    execute(&mut session, "edit.undo", &Value::Null).unwrap();
    let undone = show(&mut gpu, &mut session, Some(AuxView::Flow));
    assert!(undone.whole.is_empty(), "undo sends what it changed, not the image");
    assert_eq!(undone.region_texel(0, middle, middle), Some(FLOW_VIEW_STILL));
}

#[test]
fn a_fill_layer_edit_reaches_the_flow_view() {
    let (mut gpu, mut session) = (link(), flow_session(256));
    show(&mut gpu, &mut session, Some(AuxView::Flow));

    execute(&mut session, "layer.add.fill", &Value::Null).unwrap();
    execute(&mut session, "layer.fill.set", &json!({"channel": "flow", "angle": 0.0, "speed": 1.0})).unwrap();
    let filled = show(&mut gpu, &mut session, Some(AuxView::Flow));

    assert!(filled.whole.is_empty());
    for (x, y) in [(0, 0), (130, 77), (255, 255)] {
        assert_eq!(filled.region_texel(0, x, y), Some(RIGHT), "the fill at {x}, {y}");
    }
}

#[test]
fn tiles_painted_while_another_view_is_shown_come_with_the_next_whole_image() {
    let (mut gpu, mut session) = (link(), flow_session(256));
    show(&mut gpu, &mut session, Some(AuxView::Flow));

    // The material view takes the stroke's tiles for the display textures: the flow view never saw them
    let elsewhere = show(&mut gpu, &mut session, None);
    assert_eq!(elsewhere.bytes(), 0);
    session.stroke_uv_points(&[Vec2::new(0.3, 0.5), Vec2::new(0.7, 0.5)]).unwrap();
    assert_eq!(show(&mut gpu, &mut session, None).bytes(), 0, "another view sends nothing to the aux texture");

    let back = show(&mut gpu, &mut session, Some(AuxView::Flow));
    let (_, _, rgba) = back.whole.last().expect("coming back sends the whole flow map");
    assert_eq!(texel(rgba, 256, 128, 128), RIGHT, "with the stroke painted meanwhile");
}

#[test]
fn the_quick_mask_takes_the_aux_texture_and_the_flow_view_takes_it_back() {
    let (mut gpu, mut session) = (link(), flow_session(256));
    session.stroke_uv_points(&[Vec2::new(0.3, 0.5), Vec2::new(0.7, 0.5)]).unwrap();
    show(&mut gpu, &mut session, Some(AuxView::Flow));

    execute(&mut session, "viewport.quickMask", &json!({"enabled": true})).unwrap();
    execute(&mut session, "viewport.quickMask.edit", &json!({"editing": true})).unwrap();
    let masked = show(&mut gpu, &mut session, Some(AuxView::QuickMask));
    let (_, _, overlay) = masked.whole.last().expect("the quick mask overlay is sent");
    assert_ne!(texel(overlay, 256, 128, 128), RIGHT, "the overlay is the tinted base colour, not the flow map");

    execute(&mut session, "viewport.quickMask.edit", &json!({"editing": false})).unwrap();
    let back = show(&mut gpu, &mut session, Some(AuxView::Flow));
    let (_, _, rgba) = back.whole.last().expect("the flow map replaces the overlay");
    assert_eq!(texel(rgba, 256, 128, 128), RIGHT);
}

#[test]
fn removing_the_channel_sends_still_water_and_nothing_after_it() {
    let (mut gpu, mut session) = (link(), flow_session(256));
    session.stroke_uv_points(&[Vec2::new(0.3, 0.5), Vec2::new(0.7, 0.5)]).unwrap();
    show(&mut gpu, &mut session, Some(AuxView::Flow));

    execute(&mut session, "textureSet.channel.remove", &json!({"channel": "flow"})).unwrap();
    let removed = show(&mut gpu, &mut session, Some(AuxView::Flow));

    let (_, _, rgba) = removed.whole.last().expect("the stale picture is replaced");
    assert!(rgba.as_chunks::<4>().0.iter().all(|pixel| *pixel == FLOW_VIEW_STILL), "nothing flows without the channel");
    assert!(removed.regions.is_empty(), "the tiles composited again have no flow to show");

    // Undo brings the channel and the stroke back
    execute(&mut session, "edit.undo", &Value::Null).unwrap();
    let restored = show(&mut gpu, &mut session, Some(AuxView::Flow));
    let (_, _, rgba) = restored.whole.first().expect("the flow map is sent again");
    let shown = restored.region_texel(0, 128, 128).unwrap_or(texel(rgba, 256, 128, 128));
    assert_eq!(shown, RIGHT);
}

#[test]
fn each_texture_set_shows_its_own_flow_map() {
    let mut session = Session::new_project("Sets", primitives::material_ball(), "builtin:material_ball", None, 128).unwrap();
    assert!(session.set_count() >= 2, "needs a set without the channel");
    execute(&mut session, "textureSet.channel.add", &json!({"ts": 0, "channel": "flow"})).unwrap();
    execute(&mut session, "layer.add.fill", &json!({"ts": 0})).unwrap();
    execute(&mut session, "layer.fill.set", &json!({"channel": "flow", "angle": 0.0, "speed": 1.0})).unwrap();
    let mut gpu = link();

    let first = show(&mut gpu, &mut session, Some(AuxView::Flow));
    assert_eq!(first.whole.iter().map(|(set, _, _)| *set).collect::<BTreeSet<_>>(), (0..session.set_count()).collect(), "every set's aux texture is filled");
    for (set, _, rgba) in &first.whole {
        let still = rgba.as_chunks::<4>().0.iter().all(|pixel| *pixel == FLOW_VIEW_STILL);
        assert_eq!(still, *set != 0, "set {set}: only the set with the channel flows");
    }

    // Making another set the active one changes nothing on screen: nothing is sent
    execute(&mut session, "textureSet.select", &json!({"ts": 1})).unwrap();
    assert_eq!(show(&mut gpu, &mut session, Some(AuxView::Flow)).bytes(), 0);

    // An edit to a set without the channel sends its tiles to the display, not to the flow view
    execute(&mut session, "layer.add.fill", &json!({"color": [0.9, 0.1, 0.1]})).unwrap();
    assert_eq!(show(&mut gpu, &mut session, Some(AuxView::Flow)).bytes(), 0);
}

/// The renderer the app draws with, as egui holds it, on a windowless device
struct AppGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    resources: CallbackResources,
    link: GpuLink,
}

impl AppGpu {
    fn new() -> Option<AppGpu> {
        let Ok(headless) = texturecraft_render::headless::Headless::new() else {
            eprintln!("skipping: no GPU adapter");
            return None;
        };
        let link = link();
        let mut resources = CallbackResources::default();
        resources.insert(GpuState {
            renderer: headless.renderer,
            reset: link.reset.clone(),
            recreated: link.recreated.clone(),
            reports: link.reports.clone(),
            ctx: egui::Context::default(),
        });
        Some(AppGpu { device: headless.device, queue: headless.queue, resources, link })
    }

    /// Draw one frame of the 2D view as the app does (scene sync, uploads, render) and return
    /// the pixel at texture coordinate (`u`, `v`)
    fn frame_2d(&mut self, session: &mut Session, view: Option<AuxView>, [u, v]: [f32; 2]) -> [u8; 4] {
        const VIEW: u32 = 200;
        let sync = self.link.sync(session, view, None, 0, 0.0);
        let frame = Frame {
            view_id: crate::viewport::VIEW_2D,
            size: [VIEW, VIEW],
            kind: ViewKind::Uv { ts: 0, zoom: 1.0, pan: [0.0, 0.0] },
            camera: session.camera,
            shade: Shade { view_mode: 2, ..Shade::from_project(&session.doc.project) },
            visible: vec![true; session.set_count()],
            overlay: Vec::new(),
        };
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(VIEW as f32, VIEW as f32));
        let callback = ViewCallback { frame, sync: Mutex::new(Some(sync)), rect, lost: self.link.lost.clone() };
        let screen = ScreenDescriptor { size_in_pixels: [VIEW, VIEW], pixels_per_point: 1.0 };

        let mut encoder = self.device.create_command_encoder(&Default::default());
        callback.prepare(&self.device, &self.queue, &screen, &mut encoder, &mut self.resources);
        self.queue.submit([encoder.finish()]);

        let renderer = &self.resources.get::<GpuState>().unwrap().renderer;
        let (width, _, pixels) = renderer.read_view(&self.device, &self.queue, crate::viewport::VIEW_2D).unwrap();

        // The texture fills the middle 92% of a square view
        let pixel = |t: f32| ((0.5 + (t - 0.5) * 0.92) * VIEW as f32) as u32;
        texel(&pixels, width, pixel(u), pixel(v))
    }

    /// [`AppGpu::frame_2d`] once everything waiting was composited and uploaded
    fn settled_2d(&mut self, session: &mut Session, view: Option<AuxView>, uv: [f32; 2]) -> [u8; 4] {
        loop {
            let pixel = self.frame_2d(session, view, uv);
            let waiting = session.pending_tiles() > 0 || session.caches.iter().any(|cache| !cache.upload.is_empty());
            if !waiting && !self.link.reset.load(Ordering::Acquire) {
                return pixel;
            }
        }
    }
}

/// Within one level per component of `expected`
fn shows(actual: [u8; 4], expected: [u8; 4]) -> bool {
    actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 1)
}

#[test]
fn the_app_draws_a_stroke_in_the_flow_view_while_it_is_painted() {
    let Some(mut gpu) = AppGpu::new() else { return };
    let mut session = flow_session(256);

    // Clear of the plane's edges, which the 2D view draws as lines
    let (painted, away) = ([0.44, 0.36], [0.44, 0.86]);
    let before = gpu.settled_2d(&mut session, Some(AuxView::Flow), painted);
    assert!(shows(before, FLOW_VIEW_STILL), "still water before the stroke: {before:?}");

    session.begin_stroke_uv(Vec2::new(0.3, painted[1]), 1.0).unwrap();
    session.continue_stroke_uv(Vec2::new(0.7, painted[1]), 1.0);
    let during = gpu.settled_2d(&mut session, Some(AuxView::Flow), painted);
    assert!(session.stroke_active());
    assert!(shows(during, RIGHT), "the stroke before it ends: {during:?}");
    let untouched = gpu.settled_2d(&mut session, Some(AuxView::Flow), away);
    assert!(shows(untouched, FLOW_VIEW_STILL), "{untouched:?}");

    session.cancel_stroke();
    let cancelled = gpu.settled_2d(&mut session, Some(AuxView::Flow), painted);
    assert!(shows(cancelled, FLOW_VIEW_STILL), "an abandoned stroke leaves the view: {cancelled:?}");
}

#[test]
fn the_app_clears_the_flow_view_when_the_channel_is_removed() {
    let Some(mut gpu) = AppGpu::new() else { return };
    let mut session = flow_session(256);
    let painted = [0.44, 0.36];
    session.stroke_uv_points(&[Vec2::new(0.3, painted[1]), Vec2::new(0.7, painted[1])]).unwrap();
    let shown = gpu.settled_2d(&mut session, Some(AuxView::Flow), painted);
    assert!(shows(shown, RIGHT), "{shown:?}");

    execute(&mut session, "textureSet.channel.remove", &json!({"channel": "flow"})).unwrap();

    let removed = gpu.settled_2d(&mut session, Some(AuxView::Flow), painted);
    assert!(shows(removed, FLOW_VIEW_STILL), "{removed:?}");
}
