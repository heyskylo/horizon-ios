//! Main-thread, real Game and Metal reproduction of unpresented menu geometry.
use super::super::*;
use horizon_client::{
    gamepad::{Button, PadState},
    widgets,
};
use std::{
    io::{Cursor, Write},
    path::Path,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    event::{DeviceId, Touch, TouchPhase},
    event_loop::{ActiveEventLoop, EventLoop},
    window::Window,
};

pub fn run() -> Result<()> {
    let root = PathBuf::from(
        std::env::var_os("HORIZON_TEST_RESOURCES").ok_or("HORIZON_TEST_RESOURCES required")?,
    );
    let output = PathBuf::from(
        std::env::var_os("HORIZON_PRESENTATION_PROBE_OUTPUT")
            .ok_or("HORIZON_PRESENTATION_PROBE_OUTPUT required")?,
    );
    for key in ["HORIZON_HOME", "XDG_DATA_HOME", "XDG_CONFIG_HOME"] {
        if std::env::var_os(key).is_none() {
            return Err(format!("isolated {key} required").into());
        }
    }
    std::fs::create_dir_all(&output)?;
    let loaded = load_resources(
        root.to_owned(),
        None,
        &root,
        &Settings::default(),
        Instant::now(),
        false,
        false,
        &Progress::new(),
        &mut |_, _| unreachable!(),
    )?;
    let mut options = Options::parse(std::iter::empty())?.ok_or("options")?;
    options.resources = root.to_owned();
    options.offscreen = true;
    options.headless = true;
    let mut settings = Settings::default();
    settings.gui_scale = 1;
    let game = Game::new(
        &loaded.resources,
        loaded.pack,
        loaded.gui.ok_or("graphical UI resources missing")?,
        options,
        settings,
        Audio::silent(&root),
        None,
    )?;
    let mut app = ProbeApp {
        game,
        output,
        result: None,
        started: false,
        stage: SurfaceStage::MainReady,
        serial: 0,
        deadline: None,
        next_frame: Instant::now(),
        external: None,
    };
    EventLoop::new()?.run_app(&mut app)?;
    app.result.ok_or("probe did not resume")?
}
#[derive(Clone, Copy)]
enum SurfaceStage {
    MainReady,
    ExternalReady,
    MainAgain,
}
struct ProbeApp<'a> {
    game: Game<'a>,
    output: PathBuf,
    result: Option<Result<()>>,
    started: bool,
    stage: SurfaceStage,
    serial: u64,
    deadline: Option<Instant>,
    next_frame: Instant,
    external: Option<Arc<Window>>,
}
fn caught(f: impl FnOnce() -> Result<()>) -> Result<()> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|payload| {
        let message = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .unwrap_or("probe assertion failed");
        Err(message.to_owned().into())
    })
}
impl ProbeApp<'_> {
    fn finish(&mut self, event_loop: &ActiveEventLoop, result: Result<()>) {
        self.result = Some(result);
        // The fixture owns both windows. Close them even after a failed
        // assertion; no surface or hidden native window escapes the probe.
        if let Some(window) = &self.game.window {
            window.set_visible(false);
        }
        if let Some(window) = &self.external {
            window.set_visible(false);
        }
        self.game.renderer.take();
        self.external.take();
        self.game.window.take();
        event_loop.exit();
    }
    fn request_frame(&self) {
        let window = match self.stage {
            SurfaceStage::ExternalReady => self.external.as_ref(),
            _ => self.game.window.as_ref(),
        };
        if let Some(window) = window {
            window.request_redraw();
        }
    }
    fn surface_frame(&mut self, event_loop: &ActiveEventLoop) -> Result<bool> {
        // Exactly one render per native redraw. AppKit regains control between
        // surface creation, ordering a window, and acquiring its drawable.
        frame(&mut self.game, &mut self.serial)?;
        if !self.game.renderer.as_ref().unwrap().presentation_current() {
            return Ok(false);
        }
        match self.stage {
            SurfaceStage::MainReady => {
                eprintln!("presentation_probe main surface presented from native RedrawRequested");
                let external = Arc::new(
                    event_loop.create_window(
                        Window::default_attributes()
                            .with_title("Horizon external target probe (temporary)")
                            .with_visible(true)
                            .with_inner_size(winit::dpi::PhysicalSize::new(640, 480)),
                    )?,
                );
                let renderer = self.game.renderer.as_mut().unwrap();
                let generation = renderer.presentation_generation();
                renderer.set_target(Some((71, external.clone())), (640, 480))?;
                assert!(renderer.presentation_generation() > generation);
                assert!(!renderer.presentation_current());
                self.external = Some(external);
                let id = self.game.window.as_ref().unwrap().id();
                self.game.handle_window_event(
                    event_loop,
                    id,
                    WindowEvent::Resized(winit::dpi::PhysicalSize::new(333, 222)),
                    input_trace::InputOrigin::User,
                );
                let renderer = self.game.renderer.as_ref().unwrap();
                assert_eq!([renderer.config.width, renderer.config.height], [640, 480]);
                self.stage = SurfaceStage::ExternalReady;
            }
            SurfaceStage::ExternalReady => {
                eprintln!(
                    "presentation_probe external surface presented from native RedrawRequested"
                );
                let renderer = self.game.renderer.as_mut().unwrap();
                let generation = renderer.presentation_generation();
                renderer.set_target(None, (640, 480))?;
                assert!(renderer.presentation_generation() > generation);
                assert!(!renderer.presentation_current());
                if let Some(window) = &self.external {
                    window.set_visible(false);
                }
                self.stage = SurfaceStage::MainAgain;
            }
            SurfaceStage::MainAgain => {
                eprintln!(
                    "presentation_probe real macOS target identity invalidation and phone-resize routing passed; not a physical iOS AirPlay test"
                );
                return Ok(true);
            }
        }
        self.request_frame();
        Ok(false)
    }
}
impl ApplicationHandler for ProbeApp<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.started {
            return;
        }
        self.started = true;
        let result = caught(|| {
            self.serial = exercise(event_loop, &mut self.game, &self.output)?;
            self.game.renderer.take();
            self.game.options.offscreen = false;
            let window = self.game.window.as_ref().unwrap().clone();
            window.set_title("Horizon presentation target probe (temporary)");
            window.set_visible(true);
            self.game.renderer = Some(self.game.new_renderer(window)?);
            self.game.renderer.as_mut().unwrap().resize(640, 480);
            self.deadline = Some(Instant::now() + Duration::from_secs(60));
            self.next_frame = Instant::now();
            self.request_frame();
            Ok(())
        });
        if let Err(error) = result {
            self.finish(event_loop, Err(error));
        }
        // Return to AppKit before any real-surface acquisition.
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.result.is_some() {
            return;
        }
        let Some(deadline) = self.deadline else {
            return;
        };
        let now = Instant::now();
        if now >= deadline {
            self.finish(
                event_loop,
                Err("native surface presentation timed out".into()),
            );
            return;
        }
        if now >= self.next_frame {
            self.request_frame();
            self.next_frame = now + Duration::from_millis(16);
        }
        event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
            self.next_frame.min(deadline),
        ));
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.result.is_some() || self.deadline.is_none() {
            return;
        }
        let ours = self.game.window.as_ref().is_some_and(|w| w.id() == id)
            || self.external.as_ref().is_some_and(|w| w.id() == id);
        if !ours {
            return;
        }
        if matches!(event, WindowEvent::CloseRequested) {
            self.finish(
                event_loop,
                Err("probe window closed before completion".into()),
            );
        } else if matches!(event, WindowEvent::RedrawRequested) {
            let mut done = false;
            let result = caught(|| {
                done = self.surface_frame(event_loop)?;
                Ok(())
            });
            match result {
                Err(error) => self.finish(event_loop, Err(error)),
                Ok(()) if done => self.finish(event_loop, Ok(())),
                Ok(()) => {}
            }
        }
    }
}
fn frame(game: &mut Game<'_>, serial: &mut u64) -> Result<bool> {
    let r = game.renderer.as_ref().unwrap();
    let mut batch = HudBatch::new([r.config.width, r.config.height]);
    game.draw_pause_menu(&mut batch);
    *serial += 1;
    widgets::end_frame(*serial);
    let drawn = game
        .renderer
        .as_mut()
        .unwrap()
        .render(&loading_frame(&batch))?;
    game.renderer.as_mut().unwrap().wait_in_flight();
    Ok(drawn)
}
fn ready(game: &mut Game<'_>, serial: &mut u64) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        frame(game, serial)?;
        if game.renderer.as_ref().unwrap().presentation_current() {
            return Ok(());
        }
        if Instant::now() > deadline {
            return Err("presentation timed out".into());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn options_target() -> widgets::Target {
    let label = horizon_client::lang::get("menu.options");
    let widget = widgets::last()
        .into_iter()
        .find(|w| w.enabled && w.label == label)
        .expect("actual Options button registered");
    widgets::Target {
        rect: [widget.x, widget.y, widget.w, widget.h],
        kind: widgets::Kind::Button,
    }
}
fn exercise(event_loop: &ActiveEventLoop, game: &mut Game<'_>, out: &Path) -> Result<u64> {
    assert!(
        !game.presentation_stale(),
        "rendererless control must remain usable"
    );
    game.title = None;
    game.screen = Screen::Paused;
    game.focused = true;
    let window = Arc::new(
        event_loop.create_window(
            Window::default_attributes()
                .with_visible(false)
                .with_inner_size(winit::dpi::PhysicalSize::new(640, 480)),
        )?,
    );
    game.renderer = Some(game.new_renderer(window.clone())?);
    game.window = Some(window);
    horizon_client::hud::GUI_SCALE.store(1, Ordering::Relaxed);
    game.renderer.as_mut().unwrap().resize(640, 480);
    // Original, asset-free shader. The actual pack post owns resize retirement.
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (path, text) in [
        (
            "shaders/final.vsh",
            "#version 120\nvoid main(){gl_Position=ftransform();}",
        ),
        (
            "shaders/final.fsh",
            "#version 120\nvoid main(){gl_FragColor=vec4(0.0,0.4,0.0,1.0);}",
        ),
    ] {
        zip.start_file(path, zip::write::SimpleFileOptions::default())?;
        zip.write_all(text.as_bytes())?;
    }
    let pack =
        horizon_shaders::Pack::from_zip_bytes("presentation fixture", zip.finish()?.into_inner())?;
    let post = horizon_shaders::load_post(
        &pack,
        "",
        &Default::default(),
        &horizon_shaders::Macros::standard(),
        &Default::default(),
    );
    game.renderer
        .as_mut()
        .unwrap()
        .set_shader_pack(0, Some(post), &[])
        .map_err(|e| format!("shader refused: {e:?}"))?;
    let mut serial = 0;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        frame(game, &mut serial)?;
        if let Some((_, programs, notes)) = game.renderer.as_mut().unwrap().take_shader_report() {
            assert!(
                programs.iter().any(|p| p == "final"),
                "shader did not load: {notes:?}"
            );
            break;
        }
        if Instant::now() > deadline {
            return Err("shader compile timed out".into());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    for input in ["mouse", "touch", "pad"] {
        for bypass in [true, false] {
            game.presentation_test_bypass = bypass;
            game.screen = Screen::Paused;
            game.title = None;
            game.pad.feed = Some(PadState::default());
            game.renderer.as_mut().unwrap().resize(640, 480);
            ready(game, &mut serial)?;
            let old = options_target();
            let a = out.join(format!("{input}-{bypass}-A.png"));
            game.renderer.as_mut().unwrap().screenshot =
                Some(horizon_client::gfx::ScreenshotRequest {
                    path: a.clone(),
                    size: None,
                    scaled: false,
                });
            ready(game, &mut serial)?;
            assert!(std::fs::metadata(&a)?.len() > 100, "real A pixels saved");
            let gate = game.renderer.as_mut().unwrap().hold_resize_ack_for_test();
            game.renderer.as_mut().unwrap().resize(1000, 600);
            assert!(!game.renderer.as_ref().unwrap().presentation_current());
            game.screen = Screen::Playing;
            game.keys.press(KeyCode::KeyW, Source::Keyboard);
            game.sync_presentation_input(event_loop);
            assert!(
                game.keys.held_by(&KeyCode::KeyW, Source::Keyboard),
                "ordinary resize must preserve world keyboard movement"
            );
            game.keys.release(&KeyCode::KeyW, Source::Keyboard);
            game.screen = Screen::Paused;
            let b = out.join(format!("{input}-{bypass}-B.png"));
            game.renderer.as_mut().unwrap().screenshot =
                Some(horizon_client::gfx::ScreenshotRequest {
                    path: b.clone(),
                    size: None,
                    scaled: false,
                });
            assert!(!frame(game, &mut serial)?, "resize must remain pending");
            let target = options_target();
            let centre = (
                target.rect[0] + target.rect[2] / 2.,
                target.rect[1] + target.rect[3] / 2.,
            );
            assert!(
                !horizon_client::ui::hovered(
                    centre,
                    old.rect[0],
                    old.rect[1],
                    old.rect[2],
                    old.rect[3]
                ),
                "B point must not be old Options"
            );
            let deadline = Instant::now() + Duration::from_secs(10);
            while !gate.queue_retired() {
                assert!(!frame(game, &mut serial)?);
                if Instant::now() > deadline {
                    return Err("Metal callback timed out".into());
                }
            }
            assert!(
                !b.exists(),
                "pending render must not save a fabricated B image"
            );
            activate(game, event_loop, input, target)?;
            assert_eq!(
                game.screen == Screen::Options,
                bypass,
                "{input}: private negative control must reproduce unseen activation"
            );
            game.screen = Screen::Paused;
            if input == "pad" && !bypass {
                let mut held = PadState::default();
                held.set(Button::South, true);
                held.left = [0.0, 1.0];
                game.pad.feed = Some(held);
                game.poll_pads(event_loop);
            }
            gate.release();
            ready(game, &mut serial)?;
            assert!(game.renderer.as_ref().unwrap().presentation_current());
            assert!(std::fs::metadata(&b)?.len() > 100);
            game.presentation_test_bypass = false;
            game.sync_presentation_input(event_loop);
            if input == "pad" && !bypass {
                game.pad.menu.focus = Some(options_target());
                game.poll_pads(event_loop);
                assert!(
                    game.screen == Screen::Paused,
                    "held pad must not reactivate when display resumes"
                );
                game.pad.feed = Some(PadState::default());
                game.poll_pads(event_loop);
                assert!(
                    game.screen == Screen::Paused,
                    "release must not click a cancelled press"
                );
            }
            activate(game, event_loop, input, options_target())?;
            assert!(
                game.screen == Screen::Options,
                "{input}: fresh activation after presentation"
            );
            eprintln!(
                "presentation_probe {input} bypass={bypass} pending={} ready=activated",
                if bypass {
                    "activated (negative control)"
                } else {
                    "blocked"
                }
            );
        }
    }
    game.screen = Screen::Paused;
    game.presentation_test_bypass = false;
    ready(game, &mut serial)?;
    let generation = game.renderer.as_ref().unwrap().presentation_generation();
    game.settings.gui_scale = 2;
    game.apply_settings();
    assert!(game.renderer.as_ref().unwrap().presentation_generation() > generation);
    assert!(!game.renderer.as_ref().unwrap().presentation_current());
    ready(game, &mut serial)?;
    let generation = game.renderer.as_ref().unwrap().presentation_generation();
    game.apply_settings();
    assert_eq!(
        game.renderer.as_ref().unwrap().presentation_generation(),
        generation,
        "identical GUI setting must not disarm again"
    );
    game.renderer
        .as_mut()
        .unwrap()
        .set_world_post(0.5, false, false, 1.0);
    assert!(!game.renderer.as_ref().unwrap().presentation_current());
    ready(game, &mut serial)?;
    eprintln!("presentation_probe GUI-scale and render-scale invalidate before new input");
    // A screenshot can render a new layout without presenting it to the
    // player. It must not grant that layout ownership of live menu input.
    game.renderer.as_mut().unwrap().mark_gui_geometry_changed();
    game.renderer.as_mut().unwrap().screenshot_result = None;
    let screenshot = out.join("scaled-capture.png");
    game.renderer.as_mut().unwrap().screenshot_quality = 2;
    game.renderer.as_mut().unwrap().screenshot = Some(horizon_client::gfx::ScreenshotRequest {
        path: screenshot,
        size: None,
        scaled: true,
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        frame(game, &mut serial)?;
        if let Some(result) = game.renderer.as_mut().unwrap().screenshot_result.take() {
            result.map_err(|e| format!("scaled capture: {e}"))?;
            assert!(
                !game.renderer.as_ref().unwrap().presentation_current(),
                "screenshot must not acknowledge unseen menu geometry"
            );
            break;
        }
        if Instant::now() > deadline {
            return Err("scaled screenshot timed out".into());
        }
    }
    ready(game, &mut serial)?;
    Ok(serial)
}

fn activate(
    game: &mut Game<'_>,
    event_loop: &ActiveEventLoop,
    kind: &str,
    target: widgets::Target,
) -> Result<()> {
    let centre = (
        target.rect[0] + target.rect[2] / 2.,
        target.rect[1] + target.rect[3] / 2.,
    );
    let id = game.window.as_ref().unwrap().id();
    let origin = input_trace::InputOrigin::User;
    match kind {
        "mouse" => {
            game.handle_window_event(
                event_loop,
                id,
                WindowEvent::CursorMoved {
                    device_id: DeviceId::dummy(),
                    position: winit::dpi::PhysicalPosition::new(
                        f64::from(centre.0),
                        f64::from(centre.1),
                    ),
                },
                origin,
            );
            for state in [ElementState::Pressed, ElementState::Released] {
                game.handle_window_event(
                    event_loop,
                    id,
                    WindowEvent::MouseInput {
                        device_id: DeviceId::dummy(),
                        state,
                        button: MouseButton::Left,
                    },
                    origin,
                );
            }
        }
        "touch" => {
            for phase in [TouchPhase::Started, TouchPhase::Ended] {
                game.touch_event(
                    event_loop,
                    Touch {
                        device_id: DeviceId::dummy(),
                        phase,
                        location: winit::dpi::PhysicalPosition::new(
                            f64::from(centre.0),
                            f64::from(centre.1),
                        ),
                        force: None,
                        id: 27,
                    },
                )?;
            }
            game.touch_frame(event_loop);
            game.touch_frame(event_loop);
            game.touch_frame(event_loop);
        }
        "pad" => {
            game.pad.menu.focus = Some(target);
            let mut state = PadState::default();
            state.set(Button::South, true);
            game.pad.feed = Some(state);
            game.poll_pads(event_loop);
            game.pad.feed = Some(PadState::default());
            game.poll_pads(event_loop);
        }
        _ => unreachable!(),
    }
    Ok(())
}
