use serde_json::{Value, json};
use std::io::{self, Read};
mod game;
#[path = "TOUCH_SOURCE"]
mod touch;
fn main() {
    let mut text = String::new();
    io::stdin().read_to_string(&mut text).unwrap();
    let c: Value = serde_json::from_str(&text).unwrap();
    let w = c["width"].as_f64().unwrap();
    let h = c["height"].as_f64().unwrap();
    let grabbed = c["grabbed"].as_bool().unwrap_or(false);
    let mut stick = touch::desktop::Joystick::default();
    stick.thumb = (w / 2., h / 2.);
    let mut pointer = touch::desktop::Pointer::new((w, h));
    pointer.speed = c["speed"].as_f64().unwrap_or(1.);
    pointer.scale = c["scale"].as_f64().unwrap_or(1.);
    pointer.resolution = c["resolution"].as_f64().unwrap_or(1.);
    let mut clock = touch::desktop::MotionClock::default();
    let mut output = vec![];
    let mut buttons = touch::desktop::Cursor::default();
    let mut fingers = touch::Touches::default();
    fingers.direct_look = true;
    fingers.capture_buttons = true;
    let mut game = game::Game::default();
    let layout = touch::layout(w as f32, h as f32);
    for e in c["events"].as_array().unwrap() {
        let op = e["op"].as_str().unwrap_or("tick");
        let x = e["x"].as_f64().unwrap_or(0.);
        let y = e["y"].as_f64().unwrap_or(0.);
        let mut events = vec![];
        if c["mode"] == "menu_surface" {
            buttons.set_virtual_mouse(c["virtual"].as_bool().unwrap());
            let phase = match op {
                "down" => 0,
                "up" => 2,
                _ => 1,
            };
            for action in buttons.event(
                e["id"].as_u64().unwrap(),
                phase,
                (x as f32, y as f32),
                (w as f32, h as f32),
                [[-99., -99., 0., 0.]; 2],
            ) {
                if let touch::desktop::Action::Move(x, y) = action {
                    events.push(json!(["move", x, y]));
                }
            }
        } else if c["mode"] == "gestures" {
            let recognized = touch::desktop::Recognized {
                kind: e["kind"].as_u64().unwrap() as u8,
                phase: e["phase"].as_u64().unwrap() as u8,
                slot: e["slot"].as_i64().unwrap() as i8,
                point: (x, y),
            };
            for (delay, input) in recognized.inputs(grabbed, c["mouse"].as_bool().unwrap()) {
                use touch::desktop::Physical;
                let input = match input {
                    Physical::Key(key, down) | Physical::CustomKey(key,down) => json!(["key", key, i32::from(down), 0]),
                    Physical::Mouse(button, down) | Physical::CustomMouse(button,down) => json!(["button", button, i32::from(down), 0]),
                    Physical::Scroll(x, y) => json!(["scroll", x, y]),
                    Physical::Keyboard => json!(["keyboard"]),
                    Physical::Cancel => json!(["cancel"]),
                    Physical::ToggleControls => json!(["toggle-controls"]),
                };
                events.push(json!([delay, input]));
            }
        } else if c["mode"] == "world_camera" {
            let id = e["id"].as_u64().unwrap();
            let out = match op {
                "down" => fingers.start(id, (x as f32, y as f32), 0., &[]),
                "up" => fingers.end(id),
                "cancel" => fingers.cancel(id),
                _ => fingers.moved(id, (x as f32, y as f32), &[]),
            };
            events = game.dispatch(out, c["scale"].as_f64().unwrap_or(1.) as f32);
        } else if c["mode"] == "world_stick" {
            let r = [0., 0., w as f32, h as f32];
            let layout = [(touch::Control::Joystick, r)];
            let out = match op {
                "down" => fingers.start(1, (x as f32, y as f32), 0., &layout),
                "end" => fingers.end(1),
                _ => fingers.moved(1, (x as f32, y as f32), &layout),
            };
            events = game.dispatch(out, 1.);
        } else if c["mode"] == "world_buttons" || c["mode"] == "menu_buttons" {
            let id = e["id"].as_u64().unwrap();
            let control = game::control(e["code"].as_i64().unwrap());
            let r = layout.iter().find(|(c, _)| *c == control).unwrap().1;
            let at = if e["outside"].as_bool().unwrap_or(false) {
                (r[0] + r[2] + 100., r[1] + r[3] + 100.)
            } else {
                (r[0] + r[2] / 2., r[1] + r[3] / 2.)
            };
            let phase = match op {
                "down" => 0,
                "move" => 1,
                _ => 2,
            };
            if c["mode"] == "world_buttons" {
                let out = match phase {
                    0 => fingers.start(id, at, 0., &layout),
                    1 => fingers.moved(id, at, &layout),
                    _ => fingers.end(id),
                };
                events = game.dispatch(out, 1.);
            } else {
                for action in buttons.event_controls(id, phase, at, (w as f32, h as f32), &layout) {
                    match action {
                        touch::desktop::Action::Key(control, down) => {
                            events.push(json!(["key", game::key(control), i32::from(down), 0]))
                        }
                        touch::desktop::Action::Button(right, down) => {
                            events.push(json!(["button", i32::from(right), i32::from(down), 0]))
                        }
                        touch::desktop::Action::Move(..) => {}
                    }
                }
            }
        } else if c["mode"] == "tick" {
            stick.velocity = (e["vx"].as_f64().unwrap(), e["vy"].as_f64().unwrap());
            if let Some(delta) = clock.tick(e["time"].as_f64().unwrap(), stick.velocity) {
                let p = pointer.point(3, delta, grabbed, c["virtual"].as_bool().unwrap_or(true));
                events.push(json!(["cursor", if grabbed { 3 } else { 2 }, p.0, p.1]));
            }
        } else if c["mode"] == "stick" {
            let keys = if op == "end" {
                stick.finish([0., 0., w as f32, h as f32], grabbed)
            } else {
                stick.sample([0., 0., w as f32, h as f32], (x as f32, y as f32), grabbed)
            };
            for (key, down) in keys {
                let key = match key {
                    touch::Control::Forward => 87,
                    touch::Control::Left => 65,
                    touch::Control::Back => 83,
                    _ => 68,
                };
                events.push(json!([
                    "key",
                    key,
                    i32::from(down),
                    i32::from(c["shift"].as_bool().unwrap_or(false))
                ]));
            }
        } else if c["mode"] == "cursor" {
            let phase = match op {
                "down" => 0,
                "up" => 1,
                "relative" => 3,
                _ => 2,
            };
            let p = pointer.point(
                phase,
                (x, y),
                grabbed,
                c["virtual"].as_bool().unwrap_or(true),
            );
            events.push(json!([
                "cursor",
                if !grabbed && c["virtual"].as_bool().unwrap_or(true) && phase == 3 {
                    2
                } else {
                    phase
                },
                p.0,
                p.1
            ]));
        } else {
            for code in e["codes"].as_array().unwrap() {
                let code = code.as_i64().unwrap();
                if ![-3, -4].contains(&code) {
                    continue;
                }
                let right = code == -4;
                let pos = if right { (90., 10.) } else { (10., 10.) };
                for event in buttons.event(
                    if right { 2 } else { 1 },
                    if op == "down" { 0 } else { 2 },
                    pos,
                    (100., 100.),
                    [[0., 0., 20., 20.], [80., 0., 20., 20.]],
                ) {
                    if let touch::desktop::Action::Button(right, down) = event {
                        events.push(json!(["button", i32::from(right), i32::from(down), 0]));
                    }
                }
            }
        }
        output.push(json!({"events":events,"velocity":[stick.velocity.0,stick.velocity.1],"thumb":[stick.thumb.0,stick.thumb.1]}));
    }
    println!("{}", serde_json::to_string(&output).unwrap());
}
