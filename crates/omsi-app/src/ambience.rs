//! Environment sounds: what is heard *around* the camera rather than out of a vehicle.
//!
//! OMSI keeps them in `Sounds\` next to the per-vehicle configurations: `rain_outside.wav`
//! is the rain as it sounds in the street (the bus plays its own `regen.wav` on the roof,
//! `[viewpoint] 2`, inside the cab), and `Sounds\Passengers\sound.cfg` holds the footstep
//! entry with its volume and its `[3d]` range - one metre, so a step is only heard from a
//! few metres away. The wet-road hiss (`Sounds\WetLane_1.wav`, `WetLane_2.wav`) belongs to
//! the vehicles and comes out of their own sound configurations once `StreetCond` is fed.

use glam::DVec3;
use omsi_audio::{AudioEngine, Clip, VoiceId, VoiceParams};
use std::path::Path;
use std::sync::Arc;

/// A footfall to be heard: where it happened and whether it is on a bus floor.
#[derive(Debug, Clone, Copy)]
pub struct Footfall {
    pub position: DVec3,
    pub inside: bool,
    /// On the floor of the player's own bus.
    pub own_bus: bool,
}

pub struct Ambience {
    /// `Sounds\rain_outside.wav`, the rain in the open.
    rain: Option<Arc<Clip>>,
    rain_voice: Option<VoiceId>,
    /// The step samples of `Sounds\Passengers\` and what the sound configuration there says
    /// about them (volume, `[3d]` range).
    steps: Vec<Arc<Clip>>,
    step_volume: f32,
    step_range: f32,
    /// Steps still allowed this second: a crowd getting off would otherwise fire a dozen
    /// voices a frame.
    step_budget: f32,
    rng: u64,
    /// A synthesised low engine rumble: not every bus's own sound configuration authors a
    /// distinct cabin idle loop, so the cab otherwise sounds as bare inside as out. Faded in
    /// only while the camera sits in the cabin and the engine runs.
    hum: Arc<Clip>,
    hum_voice: Option<VoiceId>,
    /// What was heard last, for `OMSI_DEBUG_SOUND`.
    pub last: String,
}

/// A couple of seconds of a soft, low engine rumble (a fundamental plus two harmonics,
/// looped with a short fade at the seam so it does not click), for [`Ambience::hum`].
fn synth_hum(sample_rate: u32) -> Arc<Clip> {
    let secs = 2.0f32;
    let n = (sample_rate as f32 * secs) as usize;
    let fade_len = ((sample_rate as f32 * 0.05) as usize).max(1);
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / sample_rate as f32;
        let tau = std::f32::consts::TAU;
        let mut s = (t * 55.0 * tau).sin() * 0.55
            + (t * 110.0 * tau).sin() * 0.25
            + (t * 27.5 * tau).sin() * 0.3;
        let fade = if i < fade_len {
            i as f32 / fade_len as f32
        } else if i >= n - fade_len {
            (n - i) as f32 / fade_len as f32
        } else {
            1.0
        };
        s *= fade;
        samples.push((s.clamp(-1.0, 1.0) * 3000.0) as i16);
    }
    Arc::new(Clip {
        sample_rate,
        channels: 1,
        samples,
    })
}

impl Ambience {
    /// Read the environment sounds of a content root. Missing files are not an error: a
    /// map pack without `Sounds\` simply has no rain of its own.
    pub fn load(engine: &AudioEngine, root: &Path) -> Ambience {
        let mut a = Ambience {
            rain: None,
            rain_voice: None,
            steps: Vec::new(),
            step_volume: 1.0,
            step_range: 1.0,
            step_budget: 0.0,
            rng: 0x5EED_1234_ABCD,
            hum: synth_hum(22050),
            hum_voice: None,
            last: String::new(),
        };
        if !engine.enabled {
            return a;
        }
        a.rain = engine.load_clip(&omsi_cfg::resolve_path(root, "Sounds\\rain_outside.wav"));
        let dir = omsi_cfg::resolve_path(root, "Sounds\\Passengers");
        // the configuration names one file; OMSI ships fourteen of them and a walking crowd
        // that repeats a single sample sounds like a machine, so the whole folder is the pool
        if let Ok(cfg) = omsi_vehicle::SoundCfg::load(&dir.join("sound.cfg")) {
            if let Some(step) = cfg
                .sounds
                .iter()
                .find(|s| s.triggers.iter().any(|t| t.eq_ignore_ascii_case("step")))
            {
                a.step_volume = step.volume.max(0.0);
                a.step_range = if step.range > 0.0 { step.range } else { 1.0 };
            }
        }
        let mut files: Vec<std::path::PathBuf> = omsi_cfg::vfs::read_dir_paths(&dir)
            .into_iter()
            .filter(|p| {
                let n = p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_ascii_lowercase())
                    .unwrap_or_default();
                // (not `Step_St_*`, steps on a stair: every step of the crowd sounded as if
                // it climbed one)
                n.starts_with("step") && n.ends_with(".wav") && !n.starts_with("step_st")
            })
            .collect();
        files.sort();
        a.steps = files.iter().filter_map(|p| engine.load_clip(p)).collect();
        log::info!(
            "environment sounds: rain {}, {} footstep samples in {}",
            if a.rain.is_some() { "yes" } else { "missing" },
            a.steps.len(),
            dir.display()
        );
        a
    }

    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    /// One frame. `precip` is the kind (1 rain, 2 snow) and the rate 0 … 1, `inside` says
    /// whether the camera sits in a vehicle (the rain is then muffled - the bus's own
    /// `regen.wav` takes over), `engine_running` whether the player's engine is running (for
    /// the cabin hum), `street_cond` the state of the road and `footfalls` the steps taken
    /// since the last frame.
    pub fn update(
        &mut self,
        engine: &AudioEngine,
        dt: f32,
        precip: (i32, f32),
        inside: bool,
        engine_running: bool,
        street_cond: f32,
        listener: DVec3,
        footfalls: &[Footfall],
    ) {
        if !engine.enabled {
            return;
        }
        self.rain(engine, precip, inside);
        self.hum(engine, inside, engine_running);
        self.footsteps(engine, dt, street_cond, listener, inside, footfalls);
    }

    /// The rain in the street: it only rains audibly, snow is silent. Heard at a quarter
    /// through the bodywork, and muffled on top of that, so that a shower is still there -
    /// duller, not just quieter - when you sit down in the cab.
    fn rain(&mut self, engine: &AudioEngine, precip: (i32, f32), inside: bool) {
        let Some(clip) = self.rain.clone() else {
            return;
        };
        let (kind, rate) = precip;
        let gain = if kind == 1 {
            (0.15 + 0.85 * rate.clamp(0.0, 1.0)) * if inside { 0.25 } else { 0.9 }
        } else {
            0.0
        };
        let params = VoiceParams {
            gain,
            pitch: 1.0,
            looping: true,
            position: None,
            doppler: true,
            range: 1.0,
            lowpass_hz: if inside { 400.0 } else { 0.0 },
            important: false,
        };
        match (self.rain_voice, gain > 0.001) {
            (Some(id), true) => {
                if engine.is_playing(id) {
                    engine.set_params(id, params);
                } else {
                    self.rain_voice = Some(engine.play(clip, params));
                }
            }
            (Some(id), false) => {
                engine.stop(id);
                self.rain_voice = None;
            }
            (None, true) => self.rain_voice = Some(engine.play(clip, params)),
            (None, false) => {}
        }
        self.last = format!("rain {gain:.2}");
    }

    /// The cabin's own idle rumble: only heard from inside, and only while the engine runs.
    fn hum(&mut self, engine: &AudioEngine, inside: bool, engine_running: bool) {
        let gain = if inside && engine_running { 0.35 } else { 0.0 };
        let params = VoiceParams {
            gain,
            pitch: 1.0,
            looping: true,
            position: None,
            doppler: true,
            range: 1.0,
            lowpass_hz: 300.0,
            important: false,
        };
        match (self.hum_voice, gain > 0.001) {
            (Some(id), true) => {
                if engine.is_playing(id) {
                    engine.set_params(id, params);
                } else {
                    self.hum_voice = Some(engine.play(self.hum.clone(), params));
                }
            }
            (Some(id), false) => {
                engine.stop(id);
                self.hum_voice = None;
            }
            (None, true) => self.hum_voice = Some(engine.play(self.hum.clone(), params)),
            (None, false) => {}
        }
    }

    /// Footsteps. A step is a 3D one-shot at the foot, with the `[3d]` range of the
    /// configuration (1 m), so it fades within a few metres - the pavement in front of the
    /// bus is alive, the crowd at the far end of the street is not. Snow swallows a step
    /// (quieter and duller), a wet pavement sharpens it a little.
    fn footsteps(
        &mut self,
        engine: &AudioEngine,
        dt: f32,
        street_cond: f32,
        listener: DVec3,
        listener_inside: bool,
        footfalls: &[Footfall],
    ) {
        if self.steps.is_empty() {
            return;
        }
        self.step_budget = (self.step_budget + dt * STEPS_PER_SECOND).min(STEPS_PER_SECOND);
        let snow = ((street_cond - 1.0) * 2.0).clamp(0.0, 1.0);
        let wet = street_cond.clamp(0.0, 1.0) * (1.0 - snow);
        let mut played = 0;
        for f in footfalls {
            if self.step_budget < 1.0 {
                break;
            }
            // (the samples are steps on a bus's floor, `Sounds\Passengers` - the passengers'
            // sound in OMSI; people in the street walked with them as if still aboard, #236)
            if !f.inside {
                continue;
            }
            let d = (f.position - listener).length();
            if d > self.step_range as f64 * 12.0 {
                continue;
            }
            self.step_budget -= 1.0;
            let k = (self.rand() * self.steps.len() as f32) as usize;
            let clip = self.steps[k.min(self.steps.len() - 1)].clone();
            // indoors the floor is a hard panel however deep the snow outside is
            let (gain, pitch) = if f.inside {
                (1.0, 1.0)
            } else {
                (1.0 - 0.55 * snow + 0.1 * wet, 1.0 - 0.12 * snow)
            };
            // a step on the other side of the bus's bodywork from the listener - the pavement
            // heard from the driver's seat, the saloon heard from the street - comes through
            // it: quieter and dull (it used to sound as if the people walked in the bus)
            let (through, lowpass) = if f.own_bus != listener_inside { (0.3, 600.0) } else { (1.0, 0.0) };
            let params = VoiceParams {
                gain: self.step_volume * gain * through * (0.8 + 0.4 * self.rand()),
                pitch: pitch * (0.94 + 0.12 * self.rand()),
                looping: false,
                position: Some(f.position.as_vec3()),
                doppler: true,
                range: self.step_range,
                lowpass_hz: lowpass,
                important: false,
            };
            engine.play(clip, params);
            played += 1;
        }
        if played > 0 {
            self.last = format!("{}, {played} steps (snow {snow:.2})", self.last);
        }
    }
}

/// At most this many footsteps a second are heard, however big the crowd.
const STEPS_PER_SECOND: f32 = 12.0;
