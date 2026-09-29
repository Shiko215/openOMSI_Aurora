//! The window's events: winit's `ApplicationHandler` for `App`.

/// Mirror pictures drawn per second at most, all mirrors together (see the redraw).
const MIRROR_RATE: f32 = 75.0;
/// The least a mirror is redrawn a second (see the mirrors in `window_event`).
const MIRROR_MIN_HZ: f32 = 8.0;

use super::*;

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.resumed_impl(event_loop);
    }

    /// A phone put the app into the background: its window's surface goes (made again on
    /// `resumed`), the fingers and the held keys are let go.
    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.surface = None;
        self.touch.drop_gpu();
        self.keys.clear();
        if let Some(p) = self.player.as_mut() {
            p.axes.release_all();
        }
        self.save_last_situation();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.finish_session();
                crate::platform::exit(event_loop);
            }
            WindowEvent::Resized(size) => {
                if let (Some(s), Some(r)) = (self.surface.as_mut(), self.renderer.as_ref()) {
                    s.resize(r, size.width, size.height);
                }
            }
            WindowEvent::Focused(false) => {
                // No key-up reaches us for whatever was held when focus left (alt-tab, a
                // click outside the window, an OS dialog popping up): without this, a held
                // modifier got "stuck" and made the next plain key press look like it was
                // held with that modifier - Shift got stuck this way once, and a plain `W`
                // (throttle in the wasd preset) was then read as Shift+W, OMSI's own wiper
                // key, toggling the wipers on every press instead of driving.
                self.keys.clear();
                if let Some(p) = self.player.as_mut() {
                    p.axes.release_all();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                // '/' opens the chat's input box wherever the keyboard has it (the key
                // itself is then swallowed by the chat)
                if event.state == ElementState::Pressed
                    && event.text.as_deref() == Some("/")
                    && self.lan.is_some()
                    && !lan::chat_open(&self.remotes)
                {
                    self.remotes.chat.open();
                    if let PhysicalKey::Code(code) = event.physical_key {
                        lan::chat_swallow(&mut self.remotes, code);
                    }
                    return;
                }
                // what is typed into an open LAN chat line (the key itself goes on to on_key)
                if let (Some(text), true, true) = (
                    event.text.as_deref(),
                    event.state == ElementState::Pressed,
                    lan::chat_open(&self.remotes),
                ) {
                    lan::chat_type(&mut self.remotes, text);
                }
                // a phone's back key is Escape (the game menu, out of the city map ...)
                let physical = match event.physical_key {
                    PhysicalKey::Code(KeyCode::BrowserBack) => PhysicalKey::Code(KeyCode::Escape),
                    k => k,
                };
                if let PhysicalKey::Code(code) = physical {
                    self.on_key(
                        event_loop,
                        code,
                        event.state == ElementState::Pressed,
                        event.repeat,
                    );
                }
            }
            WindowEvent::MouseInput {
                state,
                button: winit::event::MouseButton::Right,
                ..
            } => {
                if self.navigator.as_ref().map(|n| n.map_open()).unwrap_or(false) {
                    return;
                }
                self.mouse_look = state == ElementState::Pressed;
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let amount = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y,
                    winit::event::MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                self.wheel(amount);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.on_cursor(position.x as f32, position.y as f32);
            }
            WindowEvent::MouseInput {
                state,
                button: winit::event::MouseButton::Left,
                ..
            } => self.left_button(event_loop, state == ElementState::Pressed),
            // a finger (a phone; see touch.rs)
            WindowEvent::Touch(t) => self.on_touch(event_loop, t),
            WindowEvent::RedrawRequested => {
                // OMSI's autosave of the last situation: every five minutes of play
                if !self.paused && self.player.is_some() && self.clock.run_time - self.autosave_t >= 300.0 {
                    self.autosave_t = self.clock.run_time;
                    self.save_last_situation();
                }
                // the time of day a script set last frame (the nearer way round the clock)
                if let Some(t) = self.pending_time.take() {
                    let d = (t - self.clock.time + 43_200.0).rem_euclid(86_400.0) - 43_200.0;
                    self.shift_clock(d);
                }
                // the graphics device is gone (a driver reset, an external card unplugged):
                // nothing can be drawn again - end the session the ordinary way, so that the
                // summary, the personnel file and the LAN goodbye are not lost
                if let Some(why) = self.renderer.as_ref().and_then(|r| r.device_lost()) {
                    log::error!("ending the session: the graphics device was lost ({why})");
                    crate::platform::exit(event_loop);
                    return;
                }
                // in the own bus's cab: at the wheel, a passenger's view, or sitting in a
                // seat of it after getting up (its inside is drawn and heard from inside)
                self.in_cab = matches!(self.view.as_str(), "driver" | "pax")
                    || (self.view == "foot" && self.foot_bus() == Some(crate::humans::BusId::Player));
                // standing or sitting in another player's bus: that bus is drawn and heard
                // from inside (its interior meshes, not the outside ones over them)
                self.inside_remote = match self.foot_bus() {
                    Some(crate::humans::BusId::Ai(x)) if self.view == "foot" => crate::humans::remote_bus_player(x),
                    _ => None,
                };
                let now = Instant::now();
                let raw_dt = (now - self.last).as_secs_f32();
                let profiling = omsi_cfg::env::var_os("OMSI_PROFILE").is_some();
                if self.total_frames > 60 {
                    if raw_dt > 0.05 {
                        self.spikes += 1;
                        if profiling {
                            // where the slow frame went: the stages that took more than 2 ms,
                            // and what no stage accounts for (waiting for the window, the
                            // system, other processes)
                            let mut parts: Vec<(&'static str, f64)> = self
                                .profile
                                .iter()
                                .map(|(k, v)| {
                                    (*k, v - self.profile_prev.get(k).copied().unwrap_or(0.0))
                                })
                                .collect();
                            let staged: f64 = parts
                                .iter()
                                .filter(|(k, _)| !k.contains('.'))
                                .map(|p| p.1)
                                .sum();
                            parts.retain(|p| p.1 > 0.002);
                            parts.sort_by(|a, b| b.1.total_cmp(&a.1));
                            let list: Vec<String> = parts
                                .iter()
                                .map(|(k, v)| format!("{k} {:.0}", v * 1000.0))
                                .collect();
                            log::info!(
                                "stutter: frame {} took {:.0} ms ({}; outside the stages {:.0} ms)",
                                self.total_frames,
                                raw_dt * 1000.0,
                                list.join(", "),
                                (raw_dt as f64 - staged).max(0.0) * 1000.0
                            );
                        }
                    }
                    self.worst_ms = self.worst_ms.max(raw_dt * 1000.0);
                    // The frame-rate governor: a graphics chip that cannot hold 45 frames a
                    // second (an older laptop's, at the window's full size with the enhanced
                    // picture) gets the 3D picture drawn smaller, down to 0.6 of the window,
                    // and back up once there is room. Judged every two seconds; a render
                    // scale the player set, OMSI_FIXED_SCALE or a frame limit below 50 keep
                    // it where it is.
                    self.governor.0 += raw_dt;
                    self.governor.1 += 1;
                    if self.governor.0 >= 2.0 {
                        let fps = self.governor.1 as f32 / self.governor.0;
                        self.governor = (0.0, 0);
                        let free = self.settings.render_scale <= 0.0
                            && (self.settings.max_fps == 0 || self.settings.max_fps >= 50)
                            && omsi_cfg::env::var_os("OMSI_FIXED_SCALE").is_none();
                        if let (Some(r), true) = (self.renderer.as_ref(), free) {
                            let s = r.dynamic_scale();
                            let next = if fps < 45.0 {
                                s - 0.1
                            } else if fps > 57.0 {
                                s + 0.05
                            } else {
                                s
                            };
                            r.set_dynamic_scale(next);
                            if (r.dynamic_scale() - s).abs() > 1e-3 {
                                log::info!("frame rate {fps:.0} fps: the 3D picture is drawn at {:.0} % of the window now", r.dynamic_scale() * 100.0);
                            }
                        }
                    }
                }
                if profiling {
                    self.profile_prev.clone_from(&self.profile);
                }
                let dt = raw_dt.min(0.1);
                self.last = now;
                self.run_input_script(event_loop);
                if let Some(m) = self.menu.as_ref() {
                    if let Some(limit) = self.args.exit_after {
                        if self.started.elapsed().as_secs_f32() > limit {
                            log::info!(
                                "menu: {} maps, {} vehicles",
                                m.maps.len(),
                                m.vehicles.len()
                            );
                            crate::platform::exit(event_loop);
                        }
                    }
                    let lines = m.lines();
                    if let (Some(hud), Some(s), Some(r), Some(scene), Some(win)) = (
                        self.hud.as_mut(),
                        self.surface.as_ref(),
                        self.renderer.as_mut(),
                        self.scene.as_mut(),
                        self.window.as_ref(),
                    ) {
                        hud.update(r, scene, &lines);
                        if let wgpu::CurrentSurfaceTexture::Success(frame)
                        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) =
                            s.surface.get_current_texture()
                        {
                            let view = frame.texture.create_view(&Default::default());
                            let cam = Camera {
                                position: DVec3::ZERO,
                                yaw: 0.0,
                                pitch: 0.0,
                                fov_deg: 60.0,
                                near: 0.5,
                                far: 100.0,
                            };
                            let lighting = omsi_render::Lighting {
                                sky_color: glam::Vec3::new(0.08, 0.10, 0.14),
                                ..Default::default()
                            };
                            r.render(
                                scene,
                                &view,
                                s.config.width,
                                s.config.height,
                                &cam,
                                &lighting,
                            );
                            win.pre_present_notify();
                            frame.present();
                        }
                    }
                    return;
                }
                if !self.drive_start(event_loop) {
                    return;
                }
                let __t = Instant::now();
                self.drive_streaming();
                *self.profile.entry("streaming").or_default() += __t.elapsed().as_secs_f64();
                let __t = Instant::now();
                if let (Some(t), Some(w), Some(r), Some(scene)) = (
                    self.traffic.as_mut(),
                    self.world.as_ref(),
                    self.renderer.as_ref(),
                    self.scene.as_mut(),
                ) {
                    let center = self
                        .player
                        .as_ref()
                        .map(|p| p.vehicle.position)
                        .or(self.camera.as_ref().map(|c| c.position))
                        .unwrap_or(DVec3::ZERO);
                    let aspect = self
                        .surface
                        .as_ref()
                        .map(|s| s.config.width as f64 / s.config.height.max(1) as f64)
                        .unwrap_or(16.0 / 9.0);
                    let fog = self
                        .weather
                        .as_ref()
                        .map(|w| w.fog.0 as f64)
                        .unwrap_or(50000.0);
                    traffic_inputs(
                        t,
                        self.camera.as_ref(),
                        aspect,
                        fog,
                        &self.clock,
                        self.humans.as_ref(),
                        self.player.as_ref(),
                        &r.options,
                    );
                    self.populate_t -= dt;
                    if self.populate_t <= 0.0 && !self.paused {
                        // come back quickly while there is a backlog of departures to put out
                        self.populate_t = if self
                            .schedule
                            .as_ref()
                            .map(|s| s.pending() > 0)
                            .unwrap_or(false)
                        {
                            0.1
                        } else {
                            2.0
                        };
                        let __t5 = Instant::now();
                        let view = self.camera.as_ref().map(|c| c.forward().as_dvec3());
                        t.populate_seen(w, r, scene, center, view);
                        *self.profile.entry("traffic.populate").or_default() +=
                            __t5.elapsed().as_secs_f64();
                        t.keep_clear = self
                            .player
                            .as_ref()
                            .map(|p| traffic::vehicle_bodies(&p.vehicle))
                            .unwrap_or_default();
                        t.keep_clear.extend(
                            self.remotes
                                .remotes
                                .values()
                                .flat_map(|r| traffic::vehicle_bodies(r.vehicle())),
                        );
                        if let Some(s) = self.schedule.as_mut() {
                            let window = if self.first_populate {
                                20.0 * 60.0
                            } else {
                                2.5
                            };
                            let __t6 = Instant::now();
                            s.tick(w, t, r, scene, t.day_time, window);
                            *self.profile.entry("traffic.schedule").or_default() +=
                                __t6.elapsed().as_secs_f64();
                        }
                        self.first_populate = false;
                    }
                    // the AI's lights (and a bus's saloon lamps, which its scripts switch with
                    // them): by the time of day, and by day in fog, rain, snow or under a
                    // closed cloud cover as drivers do
                    let gloomy = self.weather.as_ref().map(|w| {
                        let (kind, rate) = precip_of(w);
                        w.fog.0 < 600.0 || (kind != 0 && rate > 0.05) || w.clouds.0.trim().to_ascii_lowercase().starts_with("overcast")
                    }).unwrap_or(false);
                    t.night = omsi_sim::Daylight::compute(&self.clock, self.envir.as_ref())
                        .lamps_on
                        || gloomy;
                    let __t2 = Instant::now();
                    t.others = lan_outlines(&self.remotes);
                    t.others.extend(own_outlines(self.player.as_ref(), &self.placed));
                    if !self.paused {
                        t.player_priority = self.player.as_ref().and_then(|p| p.vehicle.var("TrafficPriority")).is_some_and(|v| v > 0.5);
                        t.tick(dt, self.player.as_ref().map(|p| player_outline(p)));
                        if let Some(w) = self.world.as_ref() {
                            w.set_switches(&t.switch_requests());
                            w.set_signals(&t.signal_aspects(&w.signal_routes));
                        }
                    }
                    *self.profile.entry("traffic.tick").or_default() +=
                        __t2.elapsed().as_secs_f64();
                    for (k, v) in ["traffic.tick.lanes", "traffic.tick.plan", "traffic.tick.ai"]
                        .into_iter()
                        .zip(t.tick_split)
                    {
                        *self.profile.entry(k).or_default() += v;
                    }
                    if let Some(p) = self.player.as_mut() {
                        // the options' [no_collision_vehToVeh]: the bus drives through the traffic
                        p.vehicle.dynamic_boxes = if self.settings.collision_vehicles { t.boxes(p.vehicle.position, 80.0) } else { Vec::new() };
                    }
                    let __t3 = Instant::now();
                    if let Some(a) = self.audio.as_ref() {
                        let street = self
                            .weather
                            .as_ref()
                            .map(|w| street_condition(w, self.wetness))
                            .unwrap_or(0.0);
                        let muffled = self.in_cab;
                        t.update_audio(a, center, street, muffled);
                    }
                    *self.profile.entry("traffic.audio").or_default() +=
                        __t3.elapsed().as_secs_f64();
                    let __t4 = Instant::now();
                    t.camera = self.camera.as_ref().map(|c| c.position);
                    t.sync(w, r, scene);
                    *self.profile.entry("traffic.sync").or_default() +=
                        __t4.elapsed().as_secs_f64();
                }
                *self.profile.entry("traffic").or_default() += __t.elapsed().as_secs_f64();
                // The player's vehicle moves before the passengers are placed: they sit in
                // the bus frame, and placing them on the pose of the frame before made everyone
                // aboard tremble at speed (a quarter of a metre behind the seat, every frame).
                let __t = Instant::now();
                self.drag_frame();
                // the tutorial's pages, once the world is there
                if self.world.is_some() {
                    if let Some(n) = self.args.tutorial.take() {
                        self.tutorial = crate::tutorial::Tutorial::load(&self.args.root, n, &self.settings.language);
                    }
                }
                // the game controllers: their axes this frame, their buttons' key actions
                let ctl = self.controllers.get_or_insert_with(|| crate::controllers::Controllers::new(&self.args.root));
                ctl.deadzone = self.settings.ctrl_deadzone;
                let analog = ctl.poll();
                let actions = std::mem::take(&mut ctl.actions);
                // the bus's force feedback (OMSI's FF_Vib_Amp)
                let driving = self.player.as_ref().filter(|_| self.view == "driver");
                ctl.feedback(driving.and_then(|p| p.vehicle.var("FF_Vib_Amp")).unwrap_or(0.0), driving.and_then(|p| p.vehicle.var("FF_Vib_Period")).unwrap_or(0.0));
                // OMSI's mouse control: the cursor's place across steers, above the middle
                // of the window is the throttle, below it the brake
                let mut analog = analog;
                if let (true, Some(s)) = (self.mouse_drive && self.view == "driver" && !self.mouse_look && self.game_menu.is_none(), self.surface.as_ref()) {
                    let (w, h) = (s.config.width as f32, s.config.height as f32);
                    let dx = (self.cursor.0 - w * 0.5) / (w * 0.4);
                    let dy = (h * 0.5 - self.cursor.1) / (h * 0.4);
                    analog.steering = Some(dx.clamp(-1.0, 1.0));
                    let dead = |v: f32| ((v.abs() - 0.05) / 0.95).clamp(0.0, 1.0);
                    analog.throttle = Some(if dy > 0.0 { dead(dy) } else { 0.0 });
                    analog.brake = Some(if dy < 0.0 { dead(dy) } else { 0.0 });
                }
                if let Some(p) = self.player.as_mut() {
                    p.analog = analog;
                    if self.game_menu.is_none() {
                        for (name, down) in actions {
                            p.action(&name, down);
                        }
                    }
                }
                // the on-screen wheel and pedals (a phone)
                self.touch_frame(dt);
                if let (Some(p), Some(r), Some(scene)) = (
                    self.player.as_mut(),
                    self.renderer.as_ref(),
                    self.scene.as_mut(),
                ) {
                    if !self.paused {
                        p.tick(
                            dt,
                            self.audio.as_ref(),
                            self.in_cab,
                        );
                        p.move_head(dt, self.settings.head_movement);
                        if let Some(w) = self.world.as_ref() {
                            crate::rail_drive::frame(p, self.traffic.as_ref().map(|t| &t.net), w, dt);
                        }
                    }
                    // a script that set the time of day (`(S.S.Time)`) moves the game's clock
                    if let Some(t) = p.vehicle.host.time_written.take() {
                        self.pending_time = Some(t);
                    }
                    // the situation's further vehicles stand and run their scripts, and the
                    // player's bus meets them
                    let mut placed_boxes = Vec::new();
                    for q in self.placed.iter_mut() {
                        if !self.paused {
                            q.vehicle.update(dt);
                        }
                        q.sync_transforms(r, scene, false);
                        let f = crate::lan::footprint_of(&q.vehicle, [2.5, 11.5, 3.0, 0.0, 0.0, 1.5]);
                        placed_boxes.push(omsi_sim::collision::Obb {
                            center: glam::DVec2::new(f.x, f.y),
                            half: glam::DVec2::new(f.width as f64 * 0.5, f.length as f64 * 0.5),
                            heading: (f.heading as f64).to_radians(),
                            z0: f.z,
                            z1: f.z + 3.0,
                            velocity: glam::DVec2::ZERO,
                            mass: 12_000.0,
                            pole: None,
                            id: -1,
                        });
                    }
                    if !self.placed.is_empty() {
                        // (the traffic writes the list afresh every frame; without it, this does)
                        if self.traffic.is_none() {
                            p.vehicle.dynamic_boxes.clear();
                        }
                        p.vehicle.dynamic_boxes.extend(placed_boxes);
                    }
                    if let Some(w) = self.world.as_ref() {
                        lay_down_poles(w, r, scene, &mut p.vehicle);
                    }
                    static EVERY: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
                    if let Some(every) = *EVERY.get_or_init(|| {
                        omsi_cfg::env::var("OMSI_DEBUG_PHYSICS")
                            .ok()
                            .and_then(|v| v.parse::<f32>().ok())
                            .filter(|v| *v > 0.0)
                    }) {
                        static LAST: std::sync::atomic::AtomicU32 =
                            std::sync::atomic::AtomicU32::new(u32::MAX);
                        let t = self.started.elapsed().as_secs_f32();
                        let bucket = (t / every) as u32;
                        if LAST.swap(bucket, std::sync::atomic::Ordering::Relaxed) != bucket {
                            log_physics(&p.vehicle, t);
                        }
                    }
                    let inside = self.in_cab;
                    p.sync_transforms(r, scene, inside);
                    // from the driver's seat the figure stays in the mirrors
                    // (from the driver's seat only the mirrors show him)
                    // (out of the seat: nobody at the wheel)
                    p.sync_driver(r, scene, dt, self.settings.driver && self.on_foot.is_none(), self.view == "driver");
                    if self.view != "free" && self.view != "foot" {
                        if let Some(cam) = self.camera.as_ref() {
                            let mut cam = p.camera_look(&self.view, cam, self.look, self.orbit);
                            if self.view == "outside" {
                                if let Some(w) = self.world.as_ref() {
                                    cam = p.camera_clipped(cam, w, self.orbit, dt);
                                }
                            } else {
                                p.arm.reset();
                            }
                            self.camera = Some(cam);
                        }
                    }
                    let __th = Instant::now();
                    // (the cursor's aim into the cab: again when the cursor or the view
                    // turned, else every few frames for switches that moved under it - a ray
                    // through every cockpit mesh every frame was a tenth of the frame)
                    let key = self.camera.as_ref().map(|c| (self.cursor.0.round() as i32, self.cursor.1.round() as i32, (c.yaw * 4.0).round() as i32, (c.pitch * 4.0).round() as i32));
                    if key != self.hover_key || self.total_frames % 6 == 0 {
                        self.hover_key = key;
                        self.update_hover();
                    }
                    *self.profile.entry("player.hover").or_default() +=
                        __th.elapsed().as_secs_f64();
                    if let (Some(a), Some(cam)) = (self.audio.as_ref(), self.camera.as_ref()) {
                        let (reverb_time, reverb_mix) = self.world.as_ref().map(|w| w.reverb_at(cam.position)).unwrap_or((0.0, 0.0));
                        a.set_listener(omsi_audio::Listener {
                            position: cam.position.as_vec3(),
                            forward: cam.forward(),
                            right: cam.right(),
                            // (silent while paused: the engine's loops would go on)
                            // (the settings' volume: it had been 0.6 whatever the slider said)
                            master: if self.paused { 0.0 } else { self.settings.volume.clamp(0.0, 1.0) },
                            reverb_time,
                            reverb_mix,
                        });
                    }
                }
                // on foot without a bus of one's own: the vehicles one placed still stand, run
                // their scripts and are drawn where they are (the player's frame did it)
                if let (None, Some(r), Some(scene)) = (self.player.as_ref(), self.renderer.as_ref(), self.scene.as_mut()) {
                    for q in self.placed.iter_mut() {
                        if !self.paused {
                            q.vehicle.update(dt);
                        }
                        q.sync_transforms(r, scene, false);
                    }
                }
                if let Some(a) = self.audio.as_ref() {
                    match self.player.as_ref() {
                        Some(p) => {
                            let inside = self.in_cab;
                            if let Some(m) = self.radio.update(a, &p.vehicle, inside) {
                                self.service_msg = Some((m, 6.0));
                            }
                        }
                        None => self.radio.stop(a),
                    }
                }
                *self.profile.entry("player").or_default() += __t.elapsed().as_secs_f64();
                let __t = Instant::now();
                self.tick_lan(dt);
                // (a stage of its own: a joining player's bus is loaded here, and that frame
                // was counted as the people's)
                *self.profile.entry("lan").or_default() += __t.elapsed().as_secs_f64();
                // the player on foot, and the other players walking about
                self.tick_on_foot(if self.paused { 0.0 } else { dt });
                self.sync_remote_walkers();
                let __t = Instant::now();
                if let (Some(h), Some(w), Some(r), Some(scene)) = (
                    self.humans.as_mut(),
                    self.world.as_ref(),
                    self.renderer.as_ref(),
                    self.scene.as_mut(),
                ) {
                    let center = self
                        .player
                        .as_ref()
                        .map(|p| p.vehicle.position)
                        .or(self.camera.as_ref().map(|c| c.position))
                        .unwrap_or(DVec3::ZERO);
                    // (the riders leave a bus the driver has walked away from)
                    h.driver_away = self.on_foot.as_ref().is_some_and(|f| {
                        let own = Some(crate::humans::BusId::Player);
                        f.seat.map(|s| s.0) != own && f.inside.map(|i| i.0) != own
                    });
                    h.density = w
                        .global
                        .passenger_density((self.clock.time / 3600.0) as f32)
                        // (OMSI's `AIPassFactor`, the passengers setting in per cent)
                        * self.settings.pax_density;
                    h.time_of_day = self.clock.time;
                    h.delay = self.duty.as_ref().map(|d| d.delay(self.clock.time)).unwrap_or(0.0);
                    self.humans_populate_t -= dt;
                    if self.humans_populate_t <= 0.0 && !self.paused {
                        self.humans_populate_t = 2.0;
                        h.populate(w, r, scene, center);
                    }
                    if let (Some(cam), Some(s)) = (self.camera.as_ref(), self.surface.as_ref()) {
                        h.eye = Some(humans::Eye::of(
                            cam,
                            s.config.width as f32 / s.config.height.max(1) as f32,
                        ));
                    }
                    // (the other LAN players' buses, for their riders to sit in)
                    h.set_remote_buses(self.remotes.remotes.iter().map(|(id, r)| (*id, r.vehicle())));
                    // (and the vehicles the player placed and left, with their riders)
                    h.set_placed_buses(self.placed.iter().map(|q| (q.uid, &q.vehicle)));
                    let took = h.tick(
                        if self.paused { 0.0 } else { dt },
                        w,
                        self.player.as_ref().map(|p| &p.vehicle),
                        self.traffic.as_ref(),
                        r,
                        scene,
                    );
                    // validators used: the bus's `ev_Stamper` sound
                    for bus in h.take_stamped() {
                        match bus {
                            None => {
                                if let Some(p) = self.player.as_mut() {
                                    p.vehicle.host.fired_triggers.push("ev_Stamper".into());
                                }
                            }
                            Some(id) => {
                                if let Some(c) = self.traffic.as_mut().and_then(|t| t.cars.iter_mut().find(|c| c.id == id)) {
                                    c.vehicle.host.fired_triggers.push("ev_Stamper".into());
                                }
                            }
                        }
                    }
                    if let Some(t) = self.traffic.as_mut() {
                        for (id, secs) in h.take_holds() {
                            t.hold_boarding(id, secs);
                        }
                        for (id, entry, exit) in h.take_ai_requests() {
                            t.set_pax_requests(id, &entry, &exit);
                        }
                    }
                    if let Some(m) = h.take_message() {
                        self.service_msg = Some((m, 6.0));
                    }
                    if let Some(p) = self.player.as_mut() {
                        if took {
                            p.vehicle.set_var("GivenTicket", -1.0);
                        }
                        h.give_ticket = std::mem::take(&mut p.give_ticket);
                        h.give_change_all = std::mem::take(&mut p.give_change);
                        if std::mem::take(&mut p.take_change) {
                            h.take_change_tray();
                        }
                        if std::mem::take(&mut h.stop_request) {
                            p.vehicle.trigger("door_haltewunsch");
                        }
                        if std::mem::take(&mut h.door_request) {
                            p.vehicle.trigger("door_aussenoeffner");
                        }
                        h.write_pax_vars(&mut p.vehicle);
                        p.vehicle.host.humans_on_path_link = h.path_link_counts();
                        p.vehicle.host.humans_on_seat = h.seat_counts();
                        let coins: Vec<usize> = std::mem::take(&mut p.vehicle.host.change_coins);
                        h.give_change(w, r, scene, &coins);
                        h.sync_money(r, scene, &p.vehicle);
                    }
                    h.sync(r, scene, center);
                }
                *self.profile.entry("humans").or_default() += __t.elapsed().as_secs_f64();
                self.foot_after_humans();
                if let (Some(d), Some(p), false) = (self.duty.as_mut(), self.player.as_mut(), self.paused) {
                    if let Some((arrival, departure)) = d.update(&mut p.vehicle, self.clock.time) {
                        self.career.stop_served(arrival, departure);
                        self.service_msg = Some((crate::career::Career::stop_feedback(arrival, departure).into(), 6.0));
                    }
                    if d.take_trip_change() && p.duty_typed {
                        let (trip, stop) = d.trip_for_ibis();
                        p.set_duty_destination(trip, stop);
                    }
                }
                if let Some(p) = self.player.as_mut() {
                    let riders = self.humans.as_ref().map(|h| h.riding()).unwrap_or(0);
                    // the engine's own variables of the bus (see `update_engine_vars`)
                    p.vehicle.host.humans_count = riders as f32;
                    p.vehicle.host.schedule_active = if self.duty.is_some() { 1.0 } else { 0.0 };
                    let crash = std::mem::take(&mut p.vehicle.last_crash);
                    // (a frame after the session was written must not start another one)
                    if !self.exiting && !self.paused {
                        self.career.tick(dt, &p.vehicle, riders);
                    }
                    if crash > 0.0 {
                        self.career.crashed(crash, p.vehicle.physics.velocity_kmh() / 3.6);
                        self.service_msg = Some((format!("Crash: {:.0} kJ", crash / 1000.0), 6.0));
                    }
                }
                if !self.paused {
                    crate::admin::guard_fall(self, dt);
                }
                self.placing_frame();
                // the host sends every edit of the map again now and then (players join)
                if self.lan.as_ref().map(|l| l.role == omsi_net::Role::Host).unwrap_or(false) {
                    self.editor_sync_t -= dt;
                    if self.editor_sync_t <= 0.0 {
                        self.editor_sync_t = 10.0;
                        self.editor_broadcast(true);
                    }
                }
                // --on-foot: the bus the start put down goes, the player stands beside it
                if self.args.on_foot && self.world.is_some() {
                    self.args.on_foot = false;
                    if self.player.is_some() {
                        self.remove_driven_vehicle();
                    } else if let Some(c) = self.camera.as_ref() {
                        // (no bus came: where the camera stands, on the ground)
                        let p = c.position;
                        let z = self.world.as_ref().and_then(|w| w.walk_height(p.x, p.y)).unwrap_or(p.z - 1.7);
                        let yaw = c.yaw as f64;
                        self.start_on_foot(glam::DVec3::new(p.x, p.y, z), yaw);
                    }
                    self.service_msg = Some(("On foot: Esc menu, Place a vehicle..., then G at its driver's door to drive it".into(), 8.0));
                }
                // the plugins' frame, with the bus's scripts done
                let plugins = self.plugins.get_or_insert_with(crate::plugins::load);
                if !plugins.is_empty() && !self.paused {
                    let mut io = crate::plugins::Io { vehicle: self.player.as_mut().map(|p| &mut p.vehicle), dt, message: None };
                    plugins.frame(&mut io);
                    if let Some(m) = io.message {
                        self.service_msg = Some(m);
                    }
                }
                // OMSI_WATCH_VARS=a,b: every change of those variables of the player's bus
                if let (Some(p), Ok(list)) = (self.player.as_ref(), omsi_cfg::env::var("OMSI_WATCH_VARS")) {
                    thread_local!(static LAST: std::cell::RefCell<std::collections::HashMap<String, f32>> = Default::default());
                    LAST.with(|last| {
                        let mut last = last.borrow_mut();
                        for n in list.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                            let v = p.vehicle.var(n).unwrap_or(f32::NAN);
                            if last.get(n).is_none_or(|&o| o.to_bits() != v.to_bits()) {
                                log::info!("watch: {n} = {v} at {:.2} s", self.clock.time);
                                last.insert(n.to_string(), v);
                            }
                        }
                    });
                }
                if let (Some(h), Some(p)) = (self.humans.as_mut(), self.player.as_ref()) {
                    self.career.tickets = (h.tickets_sold as i32, h.ticket_cash as f64);
                    self.career.boarded = h.boarded as i32;
                    self.career.served = h.served as i32;
                    self.career.stepped_in = h.stepped_in as i32;
                    self.career.content = h.content as i32;
                    self.career.ticket_requests = h.ticket_requests as i32;
                    self.career.ticket_points = h.ticket_points as i32;
                    // the options' [no_collision_pedastrians]: nobody is knocked down
                    let hurt = if self.settings.collision_pedestrians { h.run_over(&p.vehicle) } else { 0 };
                    if hurt > 0 {
                        self.career.crashes[1] += hurt as i32;
                        self.service_msg = Some(("Pedestrian knocked down!".into(), 6.0));
                    }
                }
                // looking around and zooming work in every view, not only the free camera
                if self.player.is_some() && self.view != "free" {
                    // looking around with the keyboard: Alt + I/J/K/L (the plain letters
                    // belong to the bus - L is the headlights in Inputs/keyboard.cfg)
                    let step = 60.0 * dt;
                    let alt = self.keys.contains(&KeyCode::AltLeft)
                        || self.keys.contains(&KeyCode::AltRight);
                    if alt && self.keys.contains(&KeyCode::KeyJ) {
                        self.look.0 -= step;
                    }
                    if alt && self.keys.contains(&KeyCode::KeyL) {
                        self.look.0 += step;
                    }
                    if alt && self.keys.contains(&KeyCode::KeyI) {
                        self.look.1 = (self.look.1 + step * 0.7).min(85.0);
                    }
                    if alt && self.keys.contains(&KeyCode::KeyK) {
                        self.look.1 = (self.look.1 - step * 0.7).max(-85.0);
                    }
                    if self.view != "outside" {
                        self.look.0 = self.look.0.clamp(-140.0, 140.0);
                    }
                    // W/S and the wheel pull the outside camera in and out
                    if self.view == "outside" {
                        if self.keys.contains(&KeyCode::Equal)
                            || self.keys.contains(&KeyCode::NumpadAdd)
                        {
                            self.orbit = (self.orbit - 12.0 * dt).max(ORBIT_MIN);
                        }
                        if self.keys.contains(&KeyCode::Minus)
                            || self.keys.contains(&KeyCode::NumpadSubtract)
                        {
                            self.orbit = (self.orbit + 12.0 * dt).min(ORBIT_MAX);
                        }
                    }
                    if self.keys.contains(&KeyCode::Home) {
                        self.look = (0.0, 0.0);
                        self.orbit = ORBIT_DEFAULT;
                    }
                }
                if self.view != "free" {
                    self.ego = false;
                }
                if let (Some(cam), true) = (
                    self.camera.as_mut(),
                    self.view == "free" || self.player.is_none(),
                ) {
                    let mut v = Vec3::ZERO;
                    let f = cam.forward();
                    let r = cam.right();
                    if self.keys.contains(&KeyCode::KeyW) {
                        v += f;
                    }
                    if self.keys.contains(&KeyCode::KeyS) {
                        v -= f;
                    }
                    if self.keys.contains(&KeyCode::KeyD) {
                        v += r;
                    }
                    if self.keys.contains(&KeyCode::KeyA) {
                        v -= r;
                    }
                    if self.keys.contains(&KeyCode::KeyE) || self.keys.contains(&KeyCode::Space) {
                        v += Vec3::Z;
                    }
                    if self.keys.contains(&KeyCode::KeyQ) {
                        v -= Vec3::Z;
                    }
                    let boost = if self.keys.contains(&KeyCode::ShiftLeft) {
                        5.0
                    } else {
                        1.0
                    };
                    if self.ego {
                        // walking: along the ground at eye height, 1.4 m/s (running 4.5)
                        let flat = Vec3::new(v.x, v.y, 0.0).normalize_or_zero();
                        let pace = if boost > 1.0 { 4.5 } else { 1.4 };
                        cam.position += (flat * pace * dt).as_dvec3();
                        if let Some(g) = self.world.as_ref().and_then(|w| w.walk_height(cam.position.x, cam.position.y)) {
                            cam.position.z = g + 1.7;
                        }
                    } else {
                        cam.position += (v.normalize_or_zero() * self.speed * boost * dt).as_dvec3();
                    }
                    if self.keys.contains(&KeyCode::ArrowLeft) {
                        cam.yaw -= 60.0 * dt;
                    }
                    if self.keys.contains(&KeyCode::ArrowRight) {
                        cam.yaw += 60.0 * dt;
                    }
                    if self.keys.contains(&KeyCode::ArrowUp) {
                        cam.pitch = (cam.pitch + 40.0 * dt).min(89.0);
                    }
                    if self.keys.contains(&KeyCode::ArrowDown) {
                        cam.pitch = (cam.pitch - 40.0 * dt).max(-89.0);
                    }
                }
                if !self.paused {
                    // (the time speed: the settings', or the session's in LAN play)
                    let speed = self.time_speed();
                    self.clock.advance(dt * speed as f32);
                    if let Some(t) = self.traffic.as_mut() {
                        t.time_scale = speed;
                    }
                }
                let daylight = omsi_sim::Daylight::compute(&self.clock, self.envir.as_ref());
                if self.lamps_on != Some(daylight.lamps_on) {
                    self.lamps_on = Some(daylight.lamps_on);
                    if let (Some(w), Some(r), Some(scene)) = (
                        self.world.as_ref(),
                        self.renderer.as_ref(),
                        self.scene.as_mut(),
                    ) {
                        w.set_lamps(r, scene, daylight.lamps_on);
                    }
                }
                // the lit windows of the houses by their [NightMapMode] timetable (once a
                // second: tiles come and go, and the hours pass)
                if self.total_frames % 60 == 0 {
                    if let (Some(w), Some(r), Some(scene)) = (
                        self.world.as_ref(),
                        self.renderer.as_ref(),
                        self.scene.as_mut(),
                    ) {
                        w.update_night_modes(r, scene, &self.clock, daylight.brightness);
                    }
                    self.follow_date();
                }
                if let Some(p) = self.player.as_mut() {
                    p.vehicle.set_var("Envir_Brightness", daylight.brightness);
                    p.vehicle.host.sun_alt = daylight.altitude_deg;
                    if let Some(w) = &self.weather {
                        apply_weather(&mut p.vehicle, w, self.wetness);
                    }
                }
                let __t = Instant::now();
                // the lamps' cones in fog and falling rain or snow, and new light pictures
                if let Some(wt) = &self.weather {
                    lights::set_cone_strength(wt.fog.0, precip_of(wt).1, daylight.night);
                }
                if let Some(r) = self.renderer.as_mut() {
                    lights::upload_corona_textures(r);
                }
                // the tile light maps around the camera, for the roads' night light
                let __ta = Instant::now();
                if let (Some(w), Some(r), Some(cam)) = (self.world.as_ref(), self.renderer.as_ref(), self.camera.as_ref()) {
                    w.update_light_map_atlas(r, cam.position);
                }
                *self.profile.entry("lights.atlas").or_default() += __ta.elapsed().as_secs_f64();
                if let (Some(w), Some(scene), Some(cam)) = (
                    self.world.as_ref(),
                    self.scene.as_mut(),
                    self.camera.as_ref(),
                ) {
                    let mut vehicles: Vec<&omsi_sim::VehicleInstance> = Vec::new();
                    if let Some(p) = self.player.as_ref() {
                        vehicles.push(&p.vehicle);
                    }
                    if let Some(t) = self.traffic.as_ref() {
                        vehicles.extend(t.cars.iter().map(|c| &c.vehicle));
                    }
                    vehicles.extend(self.remotes.remotes.values().map(|r| r.vehicle()));
                    let __tc = Instant::now();
                    lights::collect(w, scene, &daylight, cam.position, &vehicles);
                    *self.profile.entry("lights.collect").or_default() += __tc.elapsed().as_secs_f64();
                    // the object editor's pick: a magenta glow over it
                    if let Some(id) = self.editor.as_ref().and_then(|e| e.selected) {
                        let at = w.edit_objects.lock().get(&id).map(|o| o.pos);
                        let moved = w.object_edits.lock().get(&id).map(|e| e.moved).unwrap_or_default();
                        if let Some(p) = at {
                            scene.coronas.push(omsi_render::Corona {
                                position: p + moved + glam::DVec3::Z * 3.0,
                                size: 0.6,
                                color: [1.0, 0.1, 0.9],
                                brightness: 2.0,
                                ..Default::default()
                            });
                        }
                    }
                    if let Some(wt) = &self.weather {
                        let (kind, rate) = precip_of(wt);
                        self.rain.set(kind, rate);
                        // [wind] direction (deg) speed (m/s)
                        let wind = Vec3::new(
                            wt.wind.0.to_radians().sin() * wt.wind.1,
                            wt.wind.0.to_radians().cos() * wt.wind.1,
                            0.0,
                        );
                        // every bus one may ride in keeps the weather out: the own, another
                        // player's, a timetable bus
                        let boxed = |v: &omsi_sim::VehicleInstance| v.ty.def.bounding_box.map(|bb| (v.position, v.heading, bb));
                        let mut buses: Vec<(glam::DVec3, f64, [f32; 6])> = self.player.as_ref().and_then(|p| boxed(&p.vehicle)).into_iter().collect();
                        buses.extend(self.remotes.remotes.values().filter_map(|rv| boxed(rv.vehicle())));
                        if let Some(t) = self.traffic.as_ref() {
                            buses.extend(t.cars.iter().filter(|c| c.is_bus() && (c.vehicle.position - cam.position).length() < 40.0).filter_map(|c| boxed(&c.vehicle)));
                        }
                        let __tr = Instant::now();
                        self.rain.tick(if self.paused { 0.0 } else { dt }, cam.position, wind, scene, &buses);
                        *self.profile.entry("lights.rain").or_default() += __tr.elapsed().as_secs_f64();
                        // wheel splashes through the puddles enhanced.wgsl paints on wet roads
                        if kind == 1 {
                            if let Some(p) = self.player.as_ref() {
                                let wheels = puddles::wheel_contacts(&p.vehicle);
                                let speed = p.vehicle.physics.velocity_kmh().abs() / 3.6;
                                let wetness = self.wetness;
                                scene.coronas.extend(self.splashes.update(
                                    dt,
                                    &wheels,
                                    speed,
                                    &|x, y| {
                                        puddles::puddle_coverage(x, y, w.wet_road_at(x, y, wetness))
                                    },
                                ));
                            }
                        }
                        // the rain heard in the street and the footsteps on the pavement
                        if let (Some(amb), Some(a)) = (self.ambience.as_mut(), self.audio.as_ref())
                        {
                            let steps = self
                                .humans
                                .as_mut()
                                .map(|h| h.take_footfalls())
                                .unwrap_or_default();
                            // what the passengers say, where they stand
                            for line in self.humans.as_mut().map(|h| h.take_voice_lines()).unwrap_or_default() {
                                if let Some(clip) = a.load_clip(&line.path) {
                                    a.play(
                                        clip,
                                        omsi_audio::mixer::VoiceParams {
                                            gain: 1.0,
                                            pitch: 1.0,
                                            looping: false,
                                            position: Some(line.position.as_vec3()),
                                            range: 3.0,
                                            lowpass_hz: 0.0,
                                        },
                                    );
                                }
                            }
                            let inside = self.in_cab;
                            let engine_running = self
                                .player
                                .as_ref()
                                .map(|p| omsi_sim::startup::engine_running(&p.vehicle))
                                .unwrap_or(false);
                            let __tm = Instant::now();
                            amb.update(
                                a,
                                dt,
                                (kind, rate),
                                inside,
                                engine_running,
                                street_condition(wt, self.wetness),
                                cam.position,
                                &steps,
                            );
                            *self.profile.entry("lights.ambience").or_default() += __tm.elapsed().as_secs_f64();
                            if let Some(every) = debug_sound_every() {
                                static LAST: std::sync::atomic::AtomicU32 =
                                    std::sync::atomic::AtomicU32::new(u32::MAX);
                                let bucket = (self.clock.time / every as f64) as u32;
                                if LAST.swap(bucket, std::sync::atomic::Ordering::Relaxed) != bucket
                                {
                                    log::info!("sound: environment - {} (precip {kind} {rate:.2}, StreetCond {:.2}, {} voices)", amb.last, street_condition(wt, self.wetness), a.voice_count());
                                }
                            }
                        }
                    }
                }
                *self.profile.entry("lights+rain").or_default() += __t.elapsed().as_secs_f64();
                let __t = Instant::now();
                if let (Some(w), Some(r), Some(scene), Some(cam)) = (
                    self.world.as_ref(),
                    self.renderer.as_ref(),
                    self.scene.as_mut(),
                    self.camera.as_ref(),
                ) {
                    let traffic = self.traffic.as_ref();
                    let phase = |c: usize, li: usize| {
                        traffic.map(|t| t.light_vars(c, li)).unwrap_or((-1.0, 0.0))
                    };
                    let __tb = Instant::now();
                    match self.schedule.as_mut() {
                        Some(s) => s.update_boards(
                            w,
                            traffic,
                            self.duty.as_ref(),
                            self.player
                                .as_ref()
                                .and_then(|p| p.vehicle.host.hof.as_deref()),
                            &self.clock,
                        ),
                        None => w.timetable_boards.lock().clock = Some(self.clock.clone()),
                    }
                    *self.profile.entry("scripted.boards").or_default() += __tb.elapsed().as_secs_f64();
                    w.update_scripted(
                        r,
                        scene,
                        dt,
                        cam.position,
                        daylight.lamps_on,
                        &phase,
                        self.audio.as_ref(),
                        self.in_cab,
                    );
                }
                *self.profile.entry("scripted").or_default() += __t.elapsed().as_secs_f64();
                // (the game menu's lines, for the interface below)
                let menu_lines = if self.game_menu.is_some() { self.game_menu_items() } else { Vec::new() };
                // HUD
                if let (Some(hud), Some(r), Some(scene)) = (
                    self.hud.as_mut(),
                    self.renderer.as_ref(),
                    self.scene.as_mut(),
                ) {
                    // No block of text over the picture (the clock, the bus, the trip and the
                    // key reminder were the old HUD in OMSI's bitmap font; the navigator shows
                    // the trip, the launcher the keys): only what the driver has to act on,
                    // in the interface font, top left.
                    let mut lines: Vec<String> = Vec::new();
                    // why the bus is not moving, whenever the throttle is pressed and nothing
                    // happens: the things a driver checks first
                    if let Some(p) = self.player.as_ref() {
                        lines.extend(standing_reasons(&p.vehicle));
                    }
                    // what is under the cursor, in the player's language (the scripts only
                    // know internal, mostly German names)
                    let names = describe::names(&self.args.root, &self.settings.language);
                    // next to the cursor (`ui`), when the setting asks for it
                    let tooltip = self.hover.as_ref().map(|h| names.control(h));
                    // the object editor's keys, while it is on (one quiet line)
                    if self.editor.is_some() {
                        lines.push("Object editor: click picks · drag moves · wheel turns (Shift lifts) · Del · C copy · V variant · Backspace undo · Ctrl+S save · Esc".into());
                    }
                    if let Some((msg, left)) = self.service_msg.as_mut() {
                        *left -= dt;
                        if *left > 0.0 {
                            lines.push(msg.clone());
                        }
                    }
                    self.service_msg = self.service_msg.take().filter(|(_, l)| *l > 0.0);
                    if let Some(lan) = self.lan.as_ref() {
                        lines.extend(lan::hud_lines(lan, &self.remotes, self.player.as_ref()));
                    }
                    if let Some(h) = self.humans.as_ref() {
                        if let Some(hint) = h.hint() {
                            lines.push(hint);
                        } else if let Some((name, value)) = &h.request {
                            lines.push(format!("Passenger wants: {name}  {value:.2}"));
                        }
                        if let Some((paid, value)) = h.paid {
                            lines.push(format!(
                                "paid: {paid:.2}  (change {:.2})",
                                (paid - value).max(0.0)
                            ));
                        }
                        if let Some(owed) = h.change_due {
                            lines.push(format!("Change due: {owed:.2}"));
                        }
                    }
                    let __t = Instant::now();
                    hud.update(r, scene, &[]);
                    let notes = lines;
                    if let (Some(nav), Some(p), Some(s)) = (self.navigator.as_mut(), self.player.as_ref(), self.surface.as_ref()) {
                        if let Some(w) = self.world.as_ref() {
                            nav.start_map(w.clone());
                        }
                        // stops beyond the loaded tiles: their places from the navigator's map
                        if let (Some(places), Some(d)) = (nav.places(), self.duty.as_mut()) {
                            if !self.duty_places {
                                self.duty_places = true;
                                d.learn_places(places);
                            }
                        }
                        let (line, terminus, stops, trip) = navigator::duty_parts(self.duty.as_ref());
                        match (trip, self.schedule.as_ref(), self.traffic.as_ref(), self.world.as_ref()) {
                            (Some((key, name)), Some(sch), _, _) if nav.map_net().is_some() => {
                                if nav.wants_route(&key, 0) {
                                    let lanes = sch.trip_route_in(nav.map_net().unwrap(), &name);
                                    let g = nav.global_version + (1 << 40);
                                    nav.set_route(&key, lanes, true, g);
                                }
                            }
                            (Some((key, name)), Some(sch), Some(t), Some(w)) => {
                                if nav.wants_route(&key, t.lanes_generation) {
                                    let (lanes, complete) = sch.trip_route(w, t, &name);
                                    nav.set_route(&key, lanes, complete, t.lanes_generation);
                                }
                            }
                            _ => nav.clear_route(),
                        }
                        let frame = navigator::NavFrame {
                            traffic: self.traffic.as_ref(),
                            bus: p.vehicle.position,
                            heading: p.vehicle.heading,
                            speed_kmh: p.vehicle.physics.velocity_kmh(),
                            line,
                            terminus,
                            stops,
                            delay: self.duty.as_ref().map(|_| p.vehicle.host.tt_delay as f64),
                            passengers: self.humans.as_ref().map(|h| h.riding()),
                            time: self.clock.time,
                            weekday: self.clock.weekday(),
                            language: &self.settings.language,
                            screen: (s.config.width as f32, s.config.height as f32),
                            dt,
                        };
                        nav.frame(r, scene, &frame);
                        // OMSI 2's dynamic route arrows over the junctions ahead
                        if nav.arrows {
                            if let Some(w) = self.world.as_ref() {
                                let spots = nav.arrow_spots(self.traffic.as_ref().map(|t| &t.net), 350.0);
                                self.route_arrows.tick(dt, w, r, scene, &spots);
                            }
                        }
                    }
                    if let (Some(ui), Some(s)) = (self.ui.as_mut(), self.surface.as_ref()) {
                        let scale = self.window.as_ref().map(|w| w.scale_factor() as f32).unwrap_or(1.0);
                        let (w, h) = (s.config.width as f32, s.config.height as f32);
                        self.remotes.chat.disabled = !self.settings.chat;
                        let chat = (self.lan.is_some() && self.settings.chat).then(|| ui::ChatView {
                            lines: &self.remotes.chat.lines,
                            typing: self.remotes.chat.typing.as_deref(),
                            error: self.remotes.chat.error(),
                        });
                        ui.chat.hidden = self.remotes.chat.hidden;
                        let tags = if self.settings.name_tags {
                            self.camera.as_ref().map(|c| lan::name_tags(&self.remotes, c, w, h)).unwrap_or_default()
                        } else {
                            Vec::new()
                        };
                        // the vehicle chooser shows its window of vehicles in the menu's place
                        // (as `chooser_window` has it, from the fields: `scene` is borrowed)
                        let chooser_list = self.admin_list.as_ref().unwrap_or(&self.vehicle_list);
                        let (chooser_items, chooser_sel): (Vec<(&str, &str)>, Option<usize>) = match self.chooser {
                            Some(sel) => {
                                let n = chooser_list.len();
                                let start = sel.saturating_sub(7).min(n.saturating_sub(15));
                                let items = chooser_list[start..(start + 15).min(n)].iter().map(|(name, path)| (path.as_str(), name.as_str())).collect();
                                (items, Some(sel - start))
                            }
                            None => (Vec::new(), None),
                        };
                        let frame = ui::Frame {
                            scale,
                            width: w,
                            height: h,
                            cursor: self.cursor,
                            tooltip: tooltip.filter(|_| self.settings.tooltips && !self.dragging),
                            notes: &notes,
                            fps: self.settings.show_fps.then_some(self.fps),
                            paused: self.paused,
                            menu: match chooser_sel {
                                Some(k) => Some((k, &chooser_items[..])),
                                None => self.game_menu.map(|k| (k, &menu_lines[..])),
                            },
                            timetable: self.timetable.then(|| timetable_rows(self.duty.as_ref(), self.player.as_ref().map(|p| p.vehicle.host.tt_delay as f64))).flatten(),
                            info: self.info_bar.then(|| info_line(&self.clock, self.player.as_ref(), self.duty.as_ref())),
                            tutorial: self.tutorial.as_ref().filter(|t| !t.hidden).and_then(|t| t.page().map(|p| (p.title.as_str(), p.text.as_str(), p.image.as_deref(), t.at, t.pages.len()))),
                            chat,
                            tags,
                        };
                        ui.draw(r, scene, &frame, dt);
                    }
                    *self.profile.entry("hud").or_default() += __t.elapsed().as_secs_f64();
                }

                let mut lighting = match self.weather.as_ref() {
                    Some(w) => {
                        self.wetness = road_wetness(precip_of(w).1, dt as f64, self.wetness);
                        weather_lighting(
                            &daylight,
                            w,
                            // (on over midnight, for the clouds' drift)
                            self.clock.time + self.clock.day_of_year as f64 * 86400.0,
                            self.wetness,
                            self.settings.shadows,
                        )
                    }
                    None => lights::lighting_from(&daylight, 50000.0),
                };
                lighting.wetness = omsi_cfg::env::var("OMSI_WETNESS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(self.wetness);
                lighting.inside = match self.inside_remote.and_then(|id| self.remotes.remotes.get(&id)) {
                    // (in another player's bus: its box is the one the camera is in)
                    Some(rv) => rv.vehicle().ty.def.bounding_box.map(|bb| (rv.vehicle().position, rv.vehicle().heading, bb)),
                    None => self.player.as_ref().and_then(|p| {
                        p.vehicle
                            .ty
                            .def
                            .bounding_box
                            .map(|bb| (p.vehicle.position, p.vehicle.heading, bb))
                    }),
                };
                lighting.detail = self.settings.detail_textures;
                let mut finish = false;
                let mut reconfigure = false;
                let shot = self.shot.take();
                if let Some(s) = self.surface.as_ref() {
                    let (w, h) = (s.config.width, s.config.height);
                    self.touch_prepare(w, h);
                }
                if let (Some(s), Some(r), Some(scene), Some(cam), Some(win)) = (
                    self.surface.as_ref(),
                    self.renderer.as_mut(),
                    self.scene.as_mut(),
                    self.camera.as_ref(),
                    self.window.as_ref(),
                ) {
                    // `shot <file>` from the input script: the scene the window is showing,
                    // from its camera and lighting, into a PNG - the only way to look at
                    // what an automated window run draws (also when the window is hidden,
                    // so it does not depend on a frame being acquired)
                    if let Some(path) = shot {
                        match r.render_to_image(
                            scene,
                            s.config.width,
                            s.config.height,
                            cam,
                            &lighting,
                        ) {
                            Ok(mut px) => match {
                                // (with the on-screen controls, when there are)
                                if let Some(over) = self.touch.picture(r, s.config.width, s.config.height) {
                                    crate::touch::composite(&mut px, &over);
                                }
                                image::save_buffer(
                                &path,
                                &px,
                                s.config.width,
                                s.config.height,
                                image::ColorType::Rgba8,
                            ) } {
                                Ok(()) => log::info!(
                                    "input script: window picture written to {}",
                                    path.display()
                                ),
                                Err(e) => log::warn!(
                                    "input script: {} could not be written: {e}",
                                    path.display()
                                ),
                            },
                            Err(e) => log::warn!(
                                "input script: the window picture could not be rendered: {e}"
                            ),
                        }
                    }
                    // A window that is hidden (another app covers it, another Space) gets
                    // no frames on macOS. OMSI_RENDER_OCCLUDED=1 draws them into a texture
                    // of the window's size anyway and waits for the GPU as a present would,
                    // so frame times can be measured with the window out of sight.
                    let __t = Instant::now();
                    // OMSI_HIDE_WINDOW=from,to: treat the window as hidden between these
                    // seconds of the session (the frame is acquired and dropped unshown), to
                    // check the hidden-window path without covering the window by hand
                    let hide_test = omsi_cfg::env::var("OMSI_HIDE_WINDOW").ok().and_then(|v| {
                        let mut it = v.split(',').filter_map(|x| x.trim().parse::<f32>().ok());
                        Some((it.next()?, it.next()?))
                    });
                    let hidden_now = hide_test
                        .map(|(a, b)| (a..b).contains(&self.started.elapsed().as_secs_f32()))
                        .unwrap_or(false);
                    let acquired = match s.surface.get_current_texture() {
                        wgpu::CurrentSurfaceTexture::Success(_)
                        | wgpu::CurrentSurfaceTexture::Suboptimal(_)
                            if hidden_now =>
                        {
                            wgpu::CurrentSurfaceTexture::Occluded
                        }
                        other => other,
                    };
                    let (frame, stand_in) = match acquired {
                        wgpu::CurrentSurfaceTexture::Success(frame)
                        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => (Some(frame), None),
                        wgpu::CurrentSurfaceTexture::Occluded
                            if omsi_cfg::env::var_os("OMSI_RENDER_OCCLUDED").is_some() =>
                        {
                            let (w, h) = (s.config.width, s.config.height);
                            if self
                                .stand_in
                                .as_ref()
                                .map(|t| (t.width(), t.height()) != (w, h))
                                .unwrap_or(true)
                            {
                                self.stand_in =
                                    Some(r.device.create_texture(&wgpu::TextureDescriptor {
                                        label: Some("hidden window"),
                                        size: wgpu::Extent3d {
                                            width: w,
                                            height: h,
                                            depth_or_array_layers: 1,
                                        },
                                        mip_level_count: 1,
                                        sample_count: 1,
                                        dimension: wgpu::TextureDimension::D2,
                                        format: r.format(),
                                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                                        view_formats: &[],
                                    }));
                            }
                            (
                                None,
                                self.stand_in
                                    .as_ref()
                                    .map(|t| t.create_view(&Default::default())),
                            )
                        }
                        wgpu::CurrentSurfaceTexture::Outdated
                        | wgpu::CurrentSurfaceTexture::Lost => {
                            reconfigure = true;
                            (None, None)
                        }
                        _ => (None, None),
                    };
                    *self.profile.entry("acquire").or_default() += __t.elapsed().as_secs_f64();
                    if frame.is_none() {
                        self.hidden_frames += 1;
                    }
                    let view = frame
                        .as_ref()
                        .map(|f| f.texture.create_view(&Default::default()))
                        .or(stand_in);
                    if let Some(view) = view {
                        let __t = Instant::now();
                        // One mirror a turn, in turn, at most MIRROR_RATE pictures a second in
                        // all: a mirror costs half the main picture's CPU time, and at 140 fps
                        // five mirrors were each redrawn 28 times a second, a small picture
                        // that nobody can tell from 15.
                        // Every mirror at least MIRROR_MIN_HZ, though: with eight of them (the
                        // Procity) at 25 fps each was redrawn three times a second, and the
                        // street jerked past in them - up to two a frame then (each costs a
                        // few milliseconds of the frame).
                        let mirrors = self.player.as_ref().map(|p| p.vehicle.ty.def.cameras_reflexion.len()).unwrap_or(0) as f32;
                        let rate = MIRROR_RATE.max(mirrors * MIRROR_MIN_HZ);
                        self.mirror_budget = (self.mirror_budget + raw_dt.min(0.1) * rate).min(2.5);
                        let mut drawn = 0;
                        // (in the cab, and from outside too while the bus is near: its
                        // mirrors are seen from the pavement and stood frozen)
                        let near = self.player.as_ref().zip(self.camera.as_ref()).is_some_and(|(p, c)| (p.vehicle.position - c.position).length() < 12.0);
                        while (self.in_cab || near) && self.mirror_budget >= 1.0 && drawn < 2 {
                            let (Some(w), Some(p)) = (self.world.as_ref(), self.player.as_ref()) else { break };
                            self.mirror_budget -= 1.0;
                            drawn += 1;
                            self.mirror_turn = self.mirror_turn.wrapping_add(1);
                            render_mirrors(
                                r,
                                scene,
                                w,
                                p,
                                &lighting,
                                Some(self.mirror_turn),
                            );
                        }
                        *self.profile.entry("mirrors").or_default() += __t.elapsed().as_secs_f64();
                        let __t = Instant::now();
                        r.render(
                            scene,
                            &view,
                            s.config.width,
                            s.config.height,
                            cam,
                            &lighting,
                        );
                        // the on-screen controls over the picture (a phone)
                        self.touch.render(r, &view, s.config.width, s.config.height);
                        *self.profile.entry("render").or_default() += __t.elapsed().as_secs_f64();
                        if omsi_cfg::env::var_os("OMSI_PROFILE_GPU").is_some() {
                            // wait for the GPU here, so that its time shows as a stage of its own
                            let __t = Instant::now();
                            let _ = r.device.poll(wgpu::PollType::Wait {
                                submission_index: None,
                                timeout: None,
                            });
                            *self.profile.entry("gpu").or_default() += __t.elapsed().as_secs_f64();
                        }
                        let __t = Instant::now();
                        match frame {
                            Some(frame) => {
                                win.pre_present_notify();
                                frame.present();
                            }
                            None => {
                                let _ = r.device.poll(wgpu::PollType::Wait {
                                    submission_index: None,
                                    timeout: None,
                                });
                            }
                        }
                        *self.profile.entry("present").or_default() += __t.elapsed().as_secs_f64();
                    } else {
                        // Nothing to draw into (a hidden window): the simulation goes on at
                        // a display's pace instead of spinning a core a thousand times a
                        // second. What it uploaded (traffic instances, streamed tiles,
                        // people, the navigator) waits in wgpu's staging buffers until the
                        // next submit, so submit nothing to let them go: without it a hidden
                        // window on Ahlheim grew by 100 MB a second (5.6 GB after 55 s).
                        let __t = Instant::now();
                        r.queue.submit(std::iter::empty::<wgpu::CommandBuffer>());
                        let _ = r.device.poll(wgpu::PollType::Poll);
                        *self.profile.entry("present").or_default() += __t.elapsed().as_secs_f64();
                        if let Some(rest) =
                            std::time::Duration::from_millis(16).checked_sub(now.elapsed())
                        {
                            std::thread::sleep(rest);
                        }
                    }
                    // max_fps (the original's [maxFPS]; OMSI_MAX_FPS for a test): the rest of
                    // the frame's time is slept, not spun, so a limit gives the CPU back
                    // (and keeps a laptop cool enough not to slow itself down)
                    let max_fps = omsi_cfg::env::var("OMSI_MAX_FPS")
                        .ok()
                        .and_then(|v| v.parse::<u32>().ok())
                        .unwrap_or(self.settings.max_fps);
                    // 0 = the screen's refresh rate: frames the screen never shows only heat the
                    // machine (with V-sync off and no limit an M4 drew 300 frames a second in the
                    // depot and ran hot); 1000 and more = no limit at all
                    let max_fps = if max_fps == 0 {
                        self.window.as_ref().and_then(|w| w.current_monitor()).and_then(|m| m.refresh_rate_millihertz()).map(|mhz| (mhz as f64 / 1000.0).round() as u32).filter(|r| *r >= 30).unwrap_or(120)
                    } else if max_fps >= 1000 {
                        0
                    } else {
                        max_fps
                    };
                    if max_fps > 0 {
                        let __t = Instant::now();
                        if let Some(rest) = std::time::Duration::from_secs_f64(1.0 / max_fps as f64)
                            .checked_sub(now.elapsed())
                        {
                            std::thread::sleep(rest);
                        }
                        *self.profile.entry("limiter").or_default() += __t.elapsed().as_secs_f64();
                    }
                    self.frames += 1;
                    let profiling = omsi_cfg::env::var_os("OMSI_PROFILE").is_some();
                    if profiling
                        && self.cpu_mark.is_none()
                        && self.started.elapsed().as_secs_f32() > 15.0
                    {
                        self.cpu_mark =
                            process_cpu_seconds().map(|c| (c, Instant::now(), self.total_frames));
                    }
                    if let (Some(limit), false) = (self.args.exit_after, self.exiting) {
                        if self.started.elapsed().as_secs_f32() > limit {
                            self.exiting = true;
                            log::info!("exit after {limit} s: {} frames total ({} with the window hidden{}), {:.1} fps average, {} frames over 50 ms, worst {:.0} ms", self.total_frames, self.hidden_frames, if omsi_cfg::env::var_os("OMSI_RENDER_OCCLUDED").is_some() { ", drawn off-screen" } else { ", not drawn" }, self.total_frames as f32 / self.started.elapsed().as_secs_f32(), self.spikes, self.worst_ms);
                            if let (Some(st), Some(w)) =
                                (self.streamer.as_ref(), self.world.as_ref())
                            {
                                log::info!("tile streaming: {} tiles loaded now, {} loaded and {} unloaded in all, {:.1} s preparing on the worker, slowest upload {:.0} ms, streaming over 16 ms in {} frames (worst {:.0} ms); {} objects + {} trees, {} rows, {} attached ({} without parent), {} unresolved", w.loaded_tiles().len(), st.loaded_total, st.unloaded_total, st.prepare_secs, st.worst_upload_ms, st.slow_frames, st.worst_frame_ms, st.stats.objects, st.stats.trees, st.stats.rows, st.stats.attached, st.stats.unattached, st.stats.failed_objects);
                                st.stats.log_ground();
                            }
                            if omsi_cfg::env::var_os("OMSI_PROFILE").is_some() {
                                let n = self.total_frames.max(1) as f64;
                                for (k, v) in &self.profile {
                                    log::info!("profile {k:10}: {:.1} ms/frame", v / n * 1000.0);
                                }
                                if let Some(h) = self.humans.as_ref() {
                                    log::info!(
                                        "profile people: {} ({})",
                                        h.people.len(),
                                        h.summary()
                                    );
                                }
                                for (k, v) in r.stats.borrow().iter() {
                                    log::info!(
                                        "profile render.{k:10}: {:.2} ms/frame",
                                        v / n * 1000.0
                                    );
                                }
                                for (k, v) in r.counts.borrow().iter() {
                                    log::info!("profile count {k}: {:.0} a frame", v / n);
                                }
                                for (pass, ms, frames) in r.gpu_pass_times() {
                                    log::info!("profile gpu pass {pass:12}: {ms:.2} ms ({frames} frames measured)");
                                }
                                if let (Some((c0, t0, f0)), Some(c1)) =
                                    (self.cpu_mark, process_cpu_seconds())
                                {
                                    let frames = self.total_frames.saturating_sub(f0).max(1) as f64;
                                    log::info!("profile: since 15 s {:.1} ms wall and {:.1} ms CPU (all threads) per frame, {:.1} cores busy", t0.elapsed().as_secs_f64() / frames * 1000.0, (c1 - c0) / frames * 1000.0, (c1 - c0) / t0.elapsed().as_secs_f64().max(1e-3));
                                }
                                let (sw, sh) = r.scene_size(s.config.width, s.config.height);
                                log::info!(
                                    "profile: window {}x{}, scene drawn at {sw}x{sh}, {}x MSAA",
                                    s.config.width,
                                    s.config.height,
                                    r.options.msaa
                                );
                            }
                            finish = true;
                            crate::platform::exit(event_loop);
                        }
                    }
                    self.total_frames += 1;
                    if self.fps_t.elapsed().as_secs_f32() >= 1.0 {
                        let speed = self
                            .player
                            .as_ref()
                            .map(|p| format!(" - {:.0} km/h", p.vehicle.physics.velocity_kmh()))
                            .unwrap_or_default();
                        self.fps = self.frames as f32;
                        win.set_title(&format!(
                            "openOMSI - {} fps{speed} - {:.0},{:.0},{:.0} yaw {:.0}",
                            self.frames,
                            cam.position.x,
                            cam.position.y,
                            cam.position.z,
                            cam.yaw.rem_euclid(360.0)
                        ));
                        self.frames = 0;
                        self.fps_t = Instant::now();
                    }
                    win.request_redraw();
                }
                if reconfigure {
                    // the drawable went away under us (display change, lost surface)
                    if let (Some(s), Some(r), Some(win)) = (
                        self.surface.as_mut(),
                        self.renderer.as_ref(),
                        self.window.as_ref(),
                    ) {
                        let size = win.inner_size();
                        s.resize(r, size.width, size.height);
                    }
                }
                if finish {
                    self.finish_session();
                }
            }
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: winit::event::DeviceId,
        event: DeviceEvent,
    ) {
        if let DeviceEvent::MouseMotion { delta } = event {
            if self.mouse_look {
                self.look_by(delta.0 as f32 * 0.15, delta.1 as f32 * 0.15);
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// The only user event: a quit signal arrived (see quit.rs).
    fn user_event(&mut self, event_loop: &ActiveEventLoop, _event: ()) {
        if let Some(sig) = quit::requested() {
            log::info!("{} received: ending the session", quit::signal_name(sig));
            self.finish_session();
            crate::platform::exit(event_loop);
        }
    }

    /// Every way out ends here (Escape, the window's close button, Cmd+Q, --exit-after, a
    /// quit signal): the session is written and the LAN peers hear that we left, before
    /// anything else is torn down (Cmd+Q ends the process without returning from the loop).
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.finish_session();
        if let Some(lan) = self.lan.take() {
            // dropping the session says goodbye (BYE) to the host or the players
            drop(lan);
            log::info!("LAN: left the session");
        }
        // the tunnel's cloudflared and the WebSocket gateway go with the game (kept in a
        // static, which Rust never drops: cloudflared outlived every session, holding the
        // port and a public tunnel open)
        crate::lan::close_public_gateway();
        // and the launcher's LAN status file goes (Cmd+Q never returns to main's guard)
        drop(lan::StatusFileGuard);
        log::info!("game ends");
    }
}

impl App {
    /// The mouse wheel (or a pinch of two fingers): `amount` notches, up positive.
    pub(crate) fn wheel(&mut self, amount: f32) {
        // the object editor: the wheel turns (Shift: lifts) the object
        if self.game_menu.is_none() && self.editor_wheel(amount) {
            return;
        }
        // placing a vehicle: the wheel turns it
        if self.placing.is_some() && self.game_menu.is_none() {
            self.placing_wheel(amount);
            return;
        }
        // the game menu and its lists scroll with the wheel
        if self.game_menu.is_some() {
            self.menu_wheel(amount);
            return;
        }
        // the city map takes the wheel while it is open
        if let Some(n) = self.navigator.as_mut().filter(|n| n.map_open()) {
            n.map_wheel(amount, self.cursor.0, self.cursor.1);
            return;
        }
        // the wheel over the chat (or while typing) scrolls its history
        if let Some(ui) = self.ui.as_mut() {
            if self.lan.is_some() && (ui.chat.hovered || lan::chat_open(&self.remotes)) {
                ui.chat.wheel(self.remotes.chat.lines.len(), amount);
                return;
            }
        }
        // The wheel over a cockpit switch turns it: the same <event>_drag the
        // original fires while the mouse is dragged, with the notch as the
        // movement. Knobs, the sun blind and the ignition key are far easier to
        // set that way than by holding the button down and moving the mouse.
        if self.hover.is_some() && self.view != "free" {
            if let (Some(p), Some(cam), Some(s)) = (
                self.player.as_mut(),
                self.camera.as_ref(),
                self.surface.as_ref(),
            ) {
                let (o, d) = cursor_ray(
                    cam,
                    self.cursor.0,
                    self.cursor.1,
                    s.config.width as f32,
                    s.config.height as f32,
                );
                let spread = pixel_angle(cam, s.config.height as f32) * 6.0;
                if p.pick(o, d, spread).is_some() {
                    // a notch is worth a good push of the mouse: the scripts divide
                    // the movement by 10 (the ignition key), 200 (the parking brake)
                    // or 500 (the driver's window), so a few pixels would do nothing
                    p.wheel(o, d, spread, -amount * 40.0);
                    return;
                }
            }
        }
        if self.view == "outside" && self.player.is_some() {
            self.orbit = (self.orbit - amount * 1.5).clamp(ORBIT_MIN, ORBIT_MAX);
        } else if let Some(cam) = self.camera.as_mut() {
            let f = cam.forward();
            cam.position += (f * amount * 4.0).as_dvec3();
        }
    }

    /// The left mouse button (or a finger's tap) where the cursor is.
    pub(crate) fn left_button(&mut self, event_loop: &ActiveEventLoop, pressed: bool) {
        let state = if pressed { ElementState::Pressed } else { ElementState::Released };
        // placing a vehicle: a click sets it down
        if self.placing.is_some() && self.game_menu.is_none() {
            if state == ElementState::Pressed {
                self.placing_click();
            }
            return;
        }
        // the game menu takes the clicks while it is open
        if self.game_menu.is_some() {
            if state == ElementState::Pressed {
                let hit = self.ui.as_ref().and_then(|u| {
                    u.menu_rects.iter().position(|r| self.cursor.0 >= r[0] && self.cursor.0 <= r[2] && self.cursor.1 >= r[1] && self.cursor.1 <= r[3])
                });
                if let Some(k) = hit {
                    let k = k + self.ui.as_ref().map(|u| u.menu_start).unwrap_or(0);
                    if self.chooser.is_none() {
                        self.game_menu = Some(k);
                    }
                    self.menu_choose(event_loop, k);
                }
            }
            return;
        }
        self.on_left(state == ElementState::Pressed)
    }
}

/// OMSI's timetable window: the current trip's stops with their times, the ones served
/// greyed, the next one marked.
fn timetable_rows(duty: Option<&crate::schedule::PlayerDuty>, delay: Option<f64>) -> Option<(String, Vec<(String, String, u8)>)> {
    let d = duty?;
    let trip = d.trips.get(d.trip_index)?;
    let hm = |t: f64| format!("{:02}:{:02}", ((t / 3600.0) as i64).rem_euclid(24), ((t % 3600.0) / 60.0) as i64);
    let delay = delay.unwrap_or(0.0);
    let title = format!(
        "{} › {}   {}{}:{:02}",
        if trip.line.trim().is_empty() { d.line.trim() } else { trip.line.trim() },
        trip.terminus.trim(),
        if delay < 0.0 { "−" } else { "+" },
        (delay.abs() / 60.0) as i64,
        (delay.abs() % 60.0) as i64
    );
    let rows = trip
        .stops
        .iter()
        .enumerate()
        .filter(|(_, s)| s.stops)
        .map(|(k, s)| (s.name.trim().to_string(), hm(s.arr), if k < d.next_stop { 0 } else if k == d.next_stop { 1 } else { 2 }))
        .collect();
    Some((title, rows))
}

/// OMSI's information bar: the time, the speed, and the trip with its next stop and delay.
fn info_line(clock: &omsi_sim::SimClock, player: Option<&Player>, duty: Option<&crate::schedule::PlayerDuty>) -> String {
    let t = clock.time;
    let mut parts = vec![format!("{:02}:{:02}:{:02}", ((t / 3600.0) as i64).rem_euclid(24), ((t % 3600.0) / 60.0) as i64, (t % 60.0) as i64)];
    if let Some(p) = player {
        parts.push(format!("{:.0} km/h", p.vehicle.physics.velocity_kmh().abs()));
        // the tank as the bus's script says it (OMSI's RL_TankContent: tank_percent)
        if let Some(tank) = p.vehicle.var("tank_percent").filter(|v| v.is_finite()) {
            parts.push(format!("tank {:.0} %", (tank * 100.0).round()));
        }
        if let Some(d) = duty {
            if let Some(trip) = d.trips.get(d.trip_index) {
                let line = if trip.line.trim().is_empty() { d.line.trim() } else { trip.line.trim() };
                parts.push(format!("{line} › {}", trip.terminus.trim()));
                if let Some(s) = trip.stops.get(d.next_stop) {
                    parts.push(format!("next: {}", s.name.trim()));
                }
                let delay = p.vehicle.host.tt_delay;
                parts.push(format!("{}{}:{:02}", if delay < 0.0 { "−" } else { "+" }, (delay.abs() / 60.0) as i64, (delay.abs() % 60.0) as i64));
            }
        }
    }
    parts.join("   ·   ")
}
