//! winit's `ApplicationHandler` for the game window: window and device events, shutting down,
//! and the idle step (`about_to_wait`).
use super::*;

impl Game<'_> {
    /// winit's `window_event`. The control socket and the pad menu call it too, with the
    /// input they make up.
    pub(super) fn handle_window_event(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        id: WindowId,
        mut event: WindowEvent,
        origin: input_trace::InputOrigin,
    ) {
        // A headless game has no window: its events (the frame, the control socket's input)
        // carry `WindowId::dummy()`.
        let ours = match &self.window {
            Some(w) => w.id() == id,
            None => self.options.headless,
        };
        if !ours {
            return;
        }
        follow_keymap(&mut event);
        // MacosUtil.setCtrlClickEmulatesRightClick: Control + left click is a right click.
        if let WindowEvent::MouseInput { state, button, .. } = &mut event
            && *button == MouseButton::Left
            && self.right_click_emulation.is_right_click(
                *state == ElementState::Pressed,
                self.modifiers.control_key(),
                cfg!(target_os = "macos") && self.settings.ctrl_click_emulates_right_click,
            )
        {
            *button = MouseButton::Right;
        }
        // Who caused it, before the event is handled: the trace wants the
        // window's own keys, never the ones the socket made up.
        self.trace_window_event(origin, &event);
        if let WindowEvent::KeyboardInput { event, .. } = &event
            && let PhysicalKey::Code(key) = event.physical_key
        {
            if event.state == ElementState::Pressed {
                self.physical_keys.insert(key);
                horizon_client::keybinds::learn_layout(event);
            } else {
                self.physical_keys.remove(&key);
            }
        }
        if matches!(
            &event,
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                ..
            }
        ) && !self.pad.menu.synthetic
        {
            self.pad.keyboard_took_over = true;
        }
        if let WindowEvent::MouseInput { state, button, .. } = &event {
            if *state == ElementState::Pressed {
                self.physical_buttons.insert(*button);
            } else {
                self.physical_buttons.remove(button);
            }
        }
        let mouse = if !self.pad_synthetic_input()
            && let WindowEvent::MouseInput { state, button, .. } = &event
        {
            Some((
                glfw_button(*button),
                *state == ElementState::Pressed,
                glfw_modifiers(self.modifiers),
            ))
        } else {
            None
        };
        if let Some((button, pressed, modifiers)) = mouse {
            let mut canceled = false;
            self.native_mouse_button_pre(button, pressed, modifiers, &mut canceled);
            if canceled {
                return;
            }
        }
        self.handle_window_event_inner(event_loop, event);
        if let Some((button, pressed, modifiers)) = mouse {
            self.native_mouse_button_post(button, pressed, modifiers);
        }
    }

    /// The loop ended: close the pads' output, save the world, write the perf report.
    pub(super) fn shut_down(&mut self) {
        self.end_screenshot_sequence(Err("cancelled: the game is closing".into()));
        self.pad_close_output();
        if let Err(e) = self.save_for_exit() {
            log::warn!("Horizon: failed to save the world on exit: {e}");
        }
        self.finish_perf();
    }
    /// winit's `about_to_wait`: what happens between frames (stop requests, the closing
    /// save, world entry, the control socket, pads), then the next frame.
    pub(super) fn idle(&mut self, event_loop: &dyn EventLoopCtl) {
        // A splitscreen player's game has nothing to save: P1's shuts the process down.
        if horizon_threads::stop_requests() > 0
            && !self.options.split_guest
            && self.pending_save.as_ref().is_none_or(|save| !save.quit)
        {
            // SIGTERM, SIGINT or SIGHUP: the shutdown hook's server halt, as a window close.
            if let Err(error) = self.begin_closing_save(true) {
                self.error = Some(error);
                event_loop.exit();
                return;
            }
        }
        if self.pending_save.is_some() {
            match self.poll_closing_save() {
                Ok(true) => event_loop.exit(),
                Ok(false) => {
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                    // Yield between polls even when disk I/O is slow.
                    event_loop.set_control_flow(ControlFlow::WaitUntil(
                        Instant::now() + Duration::from_millis(4),
                    ));
                }
                Err(error) => {
                    self.error = Some(error);
                    self.exit_on_error(event_loop);
                }
            }
            return;
        }
        if let Err(error) = horizon_client::spans::time("world_entry", || self.resume_world_entry())
        {
            self.error = Some(error);
            self.exit_on_error(event_loop);
            return;
        }
        // The world a backup made before its upgrade opens (EditWorldScreen's callback).
        if let Some(folder) = self.title.as_mut().and_then(|t| t.take_open_request())
            && let Err(error) = self.open_world(&folder, None, None, false, None)
        {
            self.error = Some(error);
            event_loop.exit();
            return;
        }
        // A Realm's address came back (GetServerDetailsTask hands over to ConnectScreen).
        if let Some(action) = self.title.as_mut().and_then(|t| t.take_connect_request())
            && let Err(error) = self.title_action(event_loop, action)
        {
            self.error = Some(error);
            event_loop.exit();
            return;
        }
        horizon_client::spans::time("server_packs", || self.tick_server_packs());
        horizon_client::spans::time("control", || self.poll_control(event_loop));
        if self.pending_save.is_some() {
            return;
        }
        horizon_client::spans::time("pads", || self.poll_pads(event_loop));
        self.pause_if_inactive();
        if self.options.frames.is_some() && self.started.elapsed() > self.options.timeout {
            self.error = Some("frame-limited run timed out".into());
            event_loop.exit();
            return;
        }
        if let Some(w) = &self.window {
            if self.options.offscreen {
                // Offscreen frames don't wait for the window to be drawn: an
                // occluded window (a locked macOS session) gets no redraws.
                let id = w.id();
                self.handle_window_event(
                    event_loop,
                    id,
                    WindowEvent::RedrawRequested,
                    input_trace::InputOrigin::Game,
                );
            } else {
                w.request_redraw();
            }
        } else if self.options.headless {
            // The headless loop paces the frames itself (`run_headless`).
            self.handle_window_event(
                event_loop,
                WindowId::dummy(),
                WindowEvent::RedrawRequested,
                input_trace::InputOrigin::Game,
            );
            return;
        }
        // Consume the last frame's result: a hidden compositor may stop sending
        // redraws altogether, leaving a successful result stale indefinitely.
        event_loop.set_control_flow(event_loop::redraw_flow(
            &mut self.frame_drawn,
            self.options.offscreen,
            Instant::now(),
        ));
    }
}

impl ApplicationHandler for Game<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        debug_keys::warm_debug_cpu();
        // The Window constructor: created fullscreen when options.txt or --fullscreen says so.
        let fullscreen = self.settings.fullscreen || self.options.fullscreen;
        let size = self.options.window_size.map(f64::from);
        self.fullscreen = horizon_client::window::FullscreenState::new(fullscreen);
        let attributes = horizon_client::window::with_horizon_icon(
            Window::default_attributes().with_title("Horizon"),
        );
        // Wayland names the window by its app id, which the horizon.desktop entry matches.
        #[cfg(target_os = "linux")]
        let attributes = winit::platform::wayland::WindowAttributesExtWayland::with_name(
            attributes, "horizon", "horizon",
        );
        // `--offscreen` renders into a texture: its window would only show white.
        let window_step = startup::step("window.create");
        let window = match event_loop.create_window(
            attributes
                .with_inner_size(winit::dpi::LogicalSize::new(size[0], size[1]))
                .with_fullscreen(fullscreen.then_some(winit::window::Fullscreen::Borderless(None)))
                .with_visible(!self.options.offscreen),
        ) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                self.error = Some(e.into());
                event_loop.exit();
                return;
            }
        };
        horizon_client::window::settle_created_fullscreen(&window, fullscreen);
        drop(window_step);
        horizon_client::telemetry::note_display(&window);
        match self.new_renderer(window.clone()) {
            Ok(r) => {
                self.renderer = Some(r);
                self.world_renderer_ready = true;
            }
            Err(e) => {
                self.error = Some(e);
                event_loop.exit();
                return;
            }
        }
        self.window = Some(window);
        self.ime_allowed = false;
        self.ime_area = None;
        self.apply_settings();
        self.previous = Instant::now();
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let key = if self.window.as_ref().is_some_and(|w| w.id() == id)
            && let WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } = &event
        {
            Some((event.clone(), glfw_modifiers(self.modifiers)))
        } else {
            None
        };
        self.handle_window_event(event_loop, id, event, input_trace::InputOrigin::User);
        if let Some((event, modifiers)) = key {
            self.native_input_key(&event, modifiers);
        }
    }
    fn device_event(&mut self, event_loop: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        // A composited view renders offscreen but its keyboard/mouse owner still
        // controls a visible native window. A headless offscreen run never does.
        if self.pending_save.is_none()
            && self.captured
            && self.focused
            && self.keyboard_mouse
            && (!self.options.offscreen || self.split_window)
            && self.screen == Screen::Playing
            && let DeviceEvent::MouseMotion { delta: (x, y) } = event
        {
            if x != 0.0 || y != 0.0 {
                self.pad_yield_input(event_loop);
            }
            // MouseHandler.onMove with the pointer grabbed; applied once per frame.
            self.note_input(
                input_trace::InputOrigin::User,
                input_trace::TraceKind::Motion { x, y },
            );
            self.mouse.on_move(0.0, 0.0, x, y, true, true);
            self.last_input = Instant::now();
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.idle(event_loop);
    }
    /// The application is ending. On macOS, Quit (Cmd+Q) ends it through
    /// applicationWillTerminate without a CloseRequested; vanilla's GLFW
    /// window turns it into a window close, so Minecraft.stop saves the world.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.shut_down();
    }
}
