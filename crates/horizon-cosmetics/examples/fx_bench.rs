//! The CPU cost per frame of companions, trails and auras for 0, 1, 16 and 64 wearers in view,
//! with items at the format's limits (512 vertices, the densest trail): what the game does for
//! them each frame, short of the GPU. `cargo run --release -p horizon-cosmetics --example fx_bench`

use glam::{DVec3, Mat4, Vec3};
use horizon_cosmetics::follow::{self, Follow};
use horizon_cosmetics::trail::{Budget, Emitter};
use horizon_cosmetics::{Item, Part, pose_matrix};
use std::collections::HashMap;
use std::hint::black_box;
use std::time::Instant;

fn png(width: u32, height: u32) -> String {
    use base64::Engine;
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    let mut writer = encoder.write_header().unwrap();
    writer
        .write_image_data(&vec![200; (width * height * 4) as usize])
        .unwrap();
    writer.finish().unwrap();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// 21 one-pixel boxes (504 vertices) within `spread` pixels, bobbing and spinning.
fn model(slot: &str, block: &str, spread: f32, lift: f32) -> Item {
    let cubes: Vec<String> = (0..21)
        .map(|i| {
            let a = i as f32 / 21.0 * std::f32::consts::TAU;
            let (x, z) = (a.cos() * spread, a.sin() * spread);
            format!(r#"{{"origin": [{x}, {lift}, {z}], "size": [1, 1, 1], "uv": [0, 0]}}"#)
        })
        .collect();
    let file = format!(
        r#"{{"format": 1, "id": "bench", "slot": "{slot}", "texture": {{"png": "{}"}},
          "parts": [{{"name": "body", "cubes": [{}]}}],
          "animations": [{{"part": "body", "motion": "spin", "axis": "y", "period": 80}}],
          {block}}}"#,
        png(16, 16),
        cubes.join(",")
    );
    Item::parse(file.as_bytes()).unwrap()
}

/// Every vertex of the item posed at `matrix` at `ticks`, as the game's CPU path makes them.
fn pose(
    item: &Item,
    matrix: Mat4,
    ticks: f32,
    poses: &mut HashMap<String, [f32; 9]>,
    out: &mut Vec<Vec3>,
) {
    poses.clear();
    item.animate(ticks, |n, p| {
        poses.insert(n.to_owned(), p);
    });
    fn walk(p: &Part, m: Mat4, poses: &HashMap<String, [f32; 9]>, out: &mut Vec<Vec3>) {
        let m = m * pose_matrix(*poses.get(&p.name).unwrap_or(&p.pose));
        for f in &p.faces {
            for v in f.vertices {
                out.push(m.transform_point3(Vec3::new(v[0], v[1], v[2]) / 16.0));
            }
        }
        for c in &p.children {
            walk(c, m, poses, out);
        }
    }
    walk(&item.root, matrix, poses, out);
}

fn main() {
    let companion = model(
        "companion",
        r#""companion": {"anchor": [-14, -6, 18], "lag": 8}"#,
        6.0,
        0.0,
    );
    let aura = model(
        "aura",
        r#""aura": {"at": "feet", "alpha": 0.5}"#,
        12.0,
        -2.0,
    );
    let trail_file = format!(
        r#"{{"format": 1, "id": "embers", "slot": "trail", "texture": {{"png": "{}"}}, "parts": [],
          "trail": {{"sprites": 4, "rate": 4, "lifetime": 40, "size": [1, 4], "gravity": -0.01,
            "drift": 4, "spin": 20, "fade": true}}}}"#,
        png(32, 8)
    );
    let trail = Item::parse(trail_file.as_bytes()).unwrap();
    let t = trail.trail.as_ref().unwrap();
    let c = companion.companion.as_ref().unwrap();
    const FRAMES: u32 = 2400;
    println!(
        "wearers | companion µs/frame | trail µs/frame (particles) | aura µs/frame | all three µs/frame"
    );
    for n in [0usize, 1, 16, 64] {
        let mut follows = vec![Follow::at(DVec3::ZERO, 0.0); n];
        let mut emitters = vec![Emitter::default(); n];
        let mut budget = Budget::default();
        let mut poses = HashMap::new();
        let mut verts = Vec::with_capacity(64 * 512);
        let mut quads = Vec::with_capacity(512);
        let (mut tc, mut tt, mut ta, mut alive) = (0.0, 0.0, 0.0, 0);
        let camera = DVec3::new(0.0, 65.6, -6.0);
        for frame in 0..FRAMES {
            let ticks = frame as f32 / 3.0; // 60 frames a second
            // Everyone sprints round a circle (0.28 blocks a tick).
            let at = |i: usize| {
                let a = (ticks * 0.28 / 8.0) + i as f32;
                DVec3::new(
                    f64::from(a.cos()) * 8.0,
                    64.0,
                    f64::from(a.sin()) * 8.0 + i as f64 * 0.1,
                )
            };
            verts.clear();
            let start = Instant::now();
            for (i, f) in follows.iter_mut().enumerate() {
                let feet = at(i);
                let base = Mat4::from_translation((feet - camera).as_vec3() + Vec3::Y * 1.501)
                    * Mat4::from_scale(Vec3::new(-0.9375, -0.9375, 0.9375));
                let target = base.transform_point3(Vec3::from(c.anchor) / 16.0);
                f.step(camera + target.as_dvec3(), ticks, c.lag, 1.0 / 60.0);
                let head = base.transform_point3(Vec3::from(horizon_cosmetics::HEAD) / 16.0);
                let mut rel = (f.pos - camera).as_vec3();
                rel = follow::out_of_view(rel, Vec3::Z, Vec3::X);
                rel = follow::clear_of(rel, head, 1.0);
                let m = Mat4::from_translation(rel) * Mat4::from_rotation_y(f.yaw.to_radians());
                pose(&companion, m, ticks, &mut poses, &mut verts);
            }
            tc += start.elapsed().as_secs_f64();
            let start = Instant::now();
            quads.clear();
            for (i, e) in emitters.iter_mut().enumerate() {
                e.step(t, at(i), true, f64::from(ticks), &mut budget);
                quads.extend(e.quads(t, f64::from(ticks), camera, Vec3::X, Vec3::Y));
            }
            tt += start.elapsed().as_secs_f64();
            alive = alive.max(budget.alive);
            let start = Instant::now();
            for i in 0..n {
                let base = Mat4::from_translation((at(i) - camera).as_vec3())
                    * Mat4::from_scale(Vec3::new(-0.9375, -0.9375, 0.9375));
                pose(&aura, base, ticks, &mut poses, &mut verts);
            }
            ta += start.elapsed().as_secs_f64();
            black_box((&verts, &quads));
        }
        let us = |s: f64| s / f64::from(FRAMES) * 1e6;
        println!(
            "{n:>7} | {:>18.2} | {:>15.2} ({alive:>3}) | {:>13.2} | {:>18.2}",
            us(tc),
            us(tt),
            us(ta),
            us(tc + tt + ta)
        );
    }
}
