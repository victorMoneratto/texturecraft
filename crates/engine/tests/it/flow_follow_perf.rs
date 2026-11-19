//! What a stroke costs with and without the flow map's own producer on a 4K set.
//! `cargo test --release -p texturecraft-engine --test it -- --ignored benchmark_followed_strokes --nocapture`

use std::time::Instant;

use serde_json::{Value, json};
use texturecraft_engine::geom::glam::Vec2;
use texturecraft_engine::geom::primitives;
use texturecraft_engine::{Session, execute};

const RESOLUTION: u32 = 4096;
const VIEWPORT: f32 = 800.0;
const RUNS: usize = 7;

/// A 4K sphere seen from the front whose brush paints `channels`, the flow map by its strokes' direction or not
fn session(channels: &[&str], follow: bool) -> Session {
    let mut session = Session::new_project("benchmark", primitives::sphere(1.0, 128, 64), "builtin:sphere", None, RESOLUTION).unwrap();
    session.viewport = Vec2::splat(VIEWPORT);
    session.camera.yaw = 0.0;
    session.camera.pitch = 0.0;
    execute(&mut session, "textureSet.channel.add", &json!({"channel": "flow"})).unwrap();
    execute(&mut session, "tool.paint", &Value::Null).unwrap();
    execute(&mut session, "tool.brush", &json!({"size": 12, "hardness": 50})).unwrap();
    for (index, channel) in channels.iter().enumerate() {
        execute(&mut session, "tool.material", &json!({"channel": channel, "enabled": true, "solo": index == 0})).unwrap();
    }
    execute(&mut session, "tool.material", &json!({"channel": "flow", "follow": follow, "enabled": channels.contains(&"flow")})).unwrap();
    session.update();
    session
}

/// One S-shaped drag across the sphere as a pointer would feed it: each sample is rasterized and
/// staged, then the live preview is composited. Milliseconds spent on the samples, on the
/// previews and on the commit, and the tiles the stroke touched.
fn drag(session: &mut Session) -> (f64, f64, f64, usize) {
    let samples: Vec<Vec2> = (0..=40)
        .map(|step| {
            let along = step as f32 / 40.0;
            Vec2::new(VIEWPORT * (0.3 + 0.4 * along), VIEWPORT * (0.5 + 0.12 * (along * std::f32::consts::TAU).sin()))
        })
        .collect();
    let milliseconds = |start: Instant| start.elapsed().as_secs_f64() * 1000.0;
    let (mut sampling, mut previewing) = (0.0, 0.0);

    let start = Instant::now();
    session.begin_stroke(samples[0], session.viewport, 1.0).unwrap();
    sampling += milliseconds(start);
    for sample in &samples[1..] {
        let start = Instant::now();
        session.continue_stroke(*sample, 1.0);
        sampling += milliseconds(start);
        let start = Instant::now();
        session.update();
        previewing += milliseconds(start);
    }
    let tiles = session.stroke_tiles();
    let start = Instant::now();
    session.end_stroke().unwrap();
    (sampling, previewing, milliseconds(start), tiles)
}

#[test]
#[ignore = "release performance measurement"]
fn benchmark_followed_strokes_at_4k() {
    let median = |mut times: Vec<f64>| {
        times.sort_by(f64::total_cmp);
        times[times.len() / 2]
    };

    for channels in [&["flow"][..], &["baseColor", "roughness", "height", "flow"][..], &["baseColor", "roughness", "height"][..]] {
        for follow in [false, true] {
            let (mut sampling, mut previewing, mut committing, mut tiles) = (Vec::new(), Vec::new(), Vec::new(), 0);
            for _ in 0..RUNS {
                let mut session = session(channels, follow);
                let run = drag(&mut session);
                sampling.push(run.0);
                previewing.push(run.1);
                committing.push(run.2);
                tiles = run.3;
            }
            eprintln!(
                "{channels:?} follow {follow}: 40 samples {:.1} ms, their previews {:.1} ms, commit {:.1} ms, {tiles} tiles",
                median(sampling),
                median(previewing),
                median(committing)
            );
        }
    }

    // For scale: the same drag with a brush material, whose pattern is carried per texel too
    let (mut sampling, mut previewing) = (Vec::new(), Vec::new());
    for _ in 0..RUNS {
        let mut session = session(&["baseColor", "roughness", "height"], false);
        execute(&mut session, "tool.material", &json!({"material": "copper_hammered"})).unwrap();
        let run = drag(&mut session);
        sampling.push(run.0);
        previewing.push(run.1);
    }
    eprintln!("brush material, no flow map: 40 samples {:.1} ms, their previews {:.1} ms", median(sampling), median(previewing));
}
