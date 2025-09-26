use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use slint::SharedString;
use rand::seq::SliceRandom;

slint::include_modules!();

// Simple audio engine using rodio + symphonia. Ported from iced app with minimal changes.
// ===== Equalizer implementation (10-band peaking filters) =====
#[derive(Clone, Copy)]
struct BiquadCoeffs { b0: f32, b1: f32, b2: f32, a1: f32, a2: f32 }
#[derive(Clone, Copy, Default)]
struct BiquadState { z1: f32, z2: f32 }
impl BiquadState {
    fn process(&mut self, x: f32, c: BiquadCoeffs) -> f32 {
        let y = c.b0 * x + self.z1;
        self.z1 = c.b1 * x - c.a1 * y + self.z2;
        self.z2 = c.b2 * x - c.a2 * y;
        y
    }
}
fn peaking_eq(sr: f32, f0: f32, q: f32, gain_db: f32) -> BiquadCoeffs {
    let a = 10f32.powf(gain_db / 40.0);
    let w0 = 2.0 * std::f32::consts::PI * (f0 / sr);
    let alpha = w0.sin() / (2.0 * q);
    let cosw = w0.cos();
    let b0 = 1.0 + alpha * a;
    let b1 = -2.0 * cosw;
    let b2 = 1.0 - alpha * a;
    let a0 = 1.0 + alpha / a;
    let a1 = -2.0 * cosw;
    let a2 = 1.0 - alpha / a;
    let inv_a0 = 1.0 / a0;
    BiquadCoeffs { b0: b0 * inv_a0, b1: b1 * inv_a0, b2: b2 * inv_a0, a1: a1 * inv_a0, a2: a2 * inv_a0 }
}

#[derive(Clone)]
struct Equalizer { gains_db: Arc<Mutex<[f32; 10]>> }
impl Default for Equalizer { fn default() -> Self { Self { gains_db: Arc::new(Mutex::new([0.0; 10])) } } }
impl Equalizer { fn set_gains_db(&self, gains: [f32; 10]) { if let Ok(mut g) = self.gains_db.lock() { *g = gains; } } fn snapshot(&self) -> [f32; 10] { self.gains_db.lock().map(|g| *g).unwrap_or([0.0;10]) } }

struct EqSource<S: rodio::Source<Item = f32>> {
    inner: S,
    coeffs: [BiquadCoeffs; 10],
    l: [BiquadState; 10],
    r: [BiquadState; 10],
    next_left: bool,
}
impl<S: rodio::Source<Item = f32>> EqSource<S> {
    fn new(inner: S, gains_db: [f32; 10]) -> Self {
        let sr = inner.sample_rate() as f32;
        let freqs = [31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
        let q = 1.0;
        let mut coeffs = [BiquadCoeffs { b0:1.0, b1:0.0, b2:0.0, a1:0.0, a2:0.0 }; 10];
        for i in 0..10 { coeffs[i] = peaking_eq(sr, freqs[i], q, gains_db[i]); }
        Self { inner, coeffs, l: [BiquadState::default(); 10], r: [BiquadState::default(); 10], next_left: true }
    }
}
impl<S: rodio::Source<Item = f32>> Iterator for EqSource<S> { type Item = f32; fn next(&mut self) -> Option<Self::Item> { let mut x = self.inner.next()?; if self.next_left { for i in 0..10 { x = self.l[i].process(x, self.coeffs[i]); } } else { for i in 0..10 { x = self.r[i].process(x, self.coeffs[i]); } } self.next_left = !self.next_left; Some(x) } }
impl<S: rodio::Source<Item = f32>> rodio::Source for EqSource<S> { fn channels(&self) -> u16 { self.inner.channels() } fn sample_rate(&self) -> u32 { self.inner.sample_rate() } fn current_span_len(&self) -> Option<usize> { self.inner.current_span_len() } fn total_duration(&self) -> Option<Duration> { self.inner.total_duration() } }

// ===== Audio Engine =====
struct AudioEngine {
    stream: rodio::stream::OutputStream,
    sink: Option<rodio::Sink>,
    current_path: Option<PathBuf>,
    duration: Option<Duration>,
    start_instant: Option<Instant>,
    paused_at: Option<Duration>,
    position_offset: Duration,
    eq: Equalizer,
}

impl AudioEngine {
    fn new() -> Result<Self, String> {
        let stream = rodio::OutputStreamBuilder::open_default_stream()
            .map_err(|e| format!("Audio output error: {e}"))?;
        Ok(Self {
            stream,
            sink: None,
            current_path: None,
            duration: None,
            start_instant: None,
            paused_at: None,
            position_offset: Duration::ZERO,
            eq: Equalizer::default(),
        })
    }

    fn stop(&mut self) {
        if let Some(sink) = self.sink.take() { sink.stop(); }
        self.current_path = None;
        self.duration = None;
        self.start_instant = None;
        self.paused_at = None;
        self.position_offset = Duration::ZERO;
    }

    fn play_from(&mut self, path: &Path, position: Duration, resume_paused: bool) -> Result<(), String> {
        use rodio::Source as _;
        if let Some(sink) = self.sink.take() { sink.stop(); }

        let file = std::fs::File::open(path).map_err(|e| format!("Failed to open file: {e}"))?;
    let decoder = rodio::Decoder::try_from(file).map_err(|e| format!("Failed to decode audio: {e}"))?;
        let same_track = self.current_path.as_ref().is_some_and(|p| p == path);
        if !same_track || self.duration.is_none() {
            self.duration = decoder.total_duration().or_else(|| probe_duration_with_symphonia(path));
        }

    let source = decoder.skip_duration(position);
    // Apply EQ to f32 samples (Decoder outputs f32 in rodio 0.21)
    let gains = self.eq.snapshot();
    let source = EqSource::new(source, gains);
        let sink = rodio::Sink::connect_new(&self.stream.mixer());
        sink.append(source);
        self.sink = Some(sink);
        self.current_path = Some(path.to_path_buf());
        self.position_offset = position;
        self.paused_at = None;
        self.start_instant = Some(Instant::now());

        if resume_paused { if let Some(s) = &self.sink { s.pause(); } }
        Ok(())
    }

    fn play_file(&mut self, path: &Path) -> Result<(), String> { self.play_from(path, Duration::ZERO, false) }
    fn pause(&mut self) { if let Some(s) = &self.sink { if !s.is_paused() { s.pause(); self.paused_at = Some(self.current_position()); self.start_instant = None; } } }
    fn resume(&mut self) { if let Some(s) = &self.sink { if s.is_paused() { s.play(); if let Some(p) = self.paused_at.take() { self.position_offset = p; } self.start_instant = Some(Instant::now()); } } }
    fn seek_to(&mut self, position: Duration) -> Result<(), String> {
        let clamped = if let Some(d) = self.duration { position.min(d) } else { position };
        if let Some(path) = self.current_path.clone() {
            let was_paused = self.sink.as_ref().is_some_and(|s| s.is_paused());
            if (self.current_position().as_secs_f32() - clamped.as_secs_f32()).abs() < 0.01 { return Ok(()); }
            self.play_from(&path, clamped, was_paused)
        } else { Ok(()) }
    }
    fn is_playing(&self) -> bool { self.sink.as_ref().map(|s| !s.is_paused() && !s.empty()).unwrap_or(false) }
    fn total_duration(&self) -> Option<Duration> { self.duration }
    fn current_position(&self) -> Duration {
        if let Some(paused) = self.paused_at { paused }
        else if let Some(start) = self.start_instant { self.position_offset + start.elapsed() }
        else { self.position_offset }
    }
}

fn probe_duration_with_symphonia(path: &Path) -> Option<Duration> {
    use symphonia::core::formats::FormatOptions as SymFormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions as SymMetadataOptions;
    use symphonia::core::probe::Hint as SymHint;
    use symphonia::default::get_probe as sym_get_probe;
    use symphonia::core::codecs::DecoderOptions as SymDecoderOptions;
    use symphonia::default::get_codecs as sym_get_codecs;

    let mut hint = SymHint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) { hint.with_extension(ext); }
    let file = std::fs::File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let probed = sym_get_probe().format(&hint, mss, &SymFormatOptions::default(), &SymMetadataOptions::default()).ok()?;
    let mut format = probed.format;
    let track = format.default_track().cloned().or_else(|| format.tracks().iter().find(|t| t.codec_params.sample_rate.is_some()).cloned())?;
    let params = &track.codec_params;
    if let (Some(sr), Some(n_frames)) = (params.sample_rate, params.n_frames) { return Some(Duration::from_secs_f64(n_frames as f64 / sr as f64)); }
    let mut decoder = sym_get_codecs().make(params, &SymDecoderOptions::default()).ok()?;
    let mut total_frames: u64 = 0;
    let mut sr_opt = params.sample_rate;
    let track_id = track.id;
    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != track_id { continue; }
        if let Ok(audio_buf) = decoder.decode(&packet) {
            total_frames += audio_buf.frames() as u64;
            let rate = audio_buf.spec().rate;
            if sr_opt.is_none() { sr_opt = Some(rate); }
        }
    }
    let sr = sr_opt?;
    if total_frames > 0 { return Some(Duration::from_secs_f64(total_frames as f64 / sr as f64)); }
    None
}

#[derive(Clone)]
struct SongItem { title: String, path: PathBuf }

fn format_time(dur: Duration) -> String { let secs = dur.as_secs(); format!("{:02}:{:02}", secs / 60, secs % 60) }

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let ui = AppWindow::new()?;

    // For mobile, scanning arbitrary folders is platform-specific. As a simple approach,
    // look for an "music" folder within the app's working directory or bundled assets.
    let music_dir = std::env::var("AUDIO_PLAYER_MUSIC_DIR").ok().map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok().map(|p| p.join("music")));

    let songs: Vec<SongItem> = music_dir.as_ref()
        .and_then(|dir| std::fs::read_dir(dir).ok())
        .map(|entries| {
            let mut v: Vec<SongItem> = entries.filter_map(|e| e.ok()).filter_map(|e| {
                let p = e.path();
                if p.is_file() {
                    if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
                        const EXTS: &[&str] = &["mp3","flac","wav","ogg","opus","aac","m4a","alac","aiff","aif"]; 
                        if EXTS.iter().any(|x| x.eq_ignore_ascii_case(ext)) {
                            let title = p.file_name().and_then(|n| n.to_str()).unwrap_or("Unknown").to_string();
                            return Some(SongItem{ title, path: p });
                        }
                    }
                }
                None
            }).collect();
            v.sort_by(|a,b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
            v
        })
        .unwrap_or_default();

    let filtered_indices = Arc::new(Mutex::new((0..songs.len()).collect::<Vec<usize>>()));
    let shuffle_order = Arc::new(Mutex::new(Vec::<usize>::new()));
    let repeat_one = Arc::new(Mutex::new(false));
    let shuffle = Arc::new(Mutex::new(false));
    let eq_gains = Arc::new(Mutex::new([0.0f32; 10]));

    let model_songs = songs.iter().map(|s| Song{ title: SharedString::from(s.title.clone())}).collect::<Vec<_>>();
    ui.set_songs(slint::ModelRc::new(slint::VecModel::from(model_songs)));

    let engine = Arc::new(Mutex::new(AudioEngine::new().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?));
    let selected = Arc::new(Mutex::new(None::<usize>));
    let search = Arc::new(Mutex::new(String::new()));

    // Handlers
    {
        let engine = engine.clone();
        let songs = songs.clone();
        let selected = selected.clone();
        let ui_handle = ui.as_weak();
        ui.on_request_select(move |index| {
            let mut sel = selected.lock().unwrap();
            *sel = Some(index as usize);
            if let Some(ui) = ui_handle.upgrade() { ui.set_selected_index(index); }
            // Toggle pause/resume if already playing this track
            if let Ok(mut eng) = engine.lock() {
                if let Some(cur_idx) = *sel {
                    if let Some(cur_path) = &eng.current_path { if songs.get(cur_idx).map(|s| &s.path) == Some(cur_path) {
                        if eng.is_playing() { eng.pause(); } else { eng.resume(); }
                        if let Some(ui) = ui_handle.upgrade() { ui.set_status_text(SharedString::from("Toggled")); }
                        return;
                    }}
                    if let Some(item) = songs.get(cur_idx) {
                        if let Err(e) = eng.play_file(&item.path) {
                            if let Some(ui) = ui_handle.upgrade() { ui.set_status_text(SharedString::from(format!("{e}"))); }
                        } else {
                            if let Some(ui) = ui_handle.upgrade() { ui.set_status_text(SharedString::from(format!("Playing: {}", item.title))); }
                        }
                    }
                }
            }
        });
    }

    {
        let engine = engine.clone();
        let songs = songs.clone();
        let selected = selected.clone();
        let ui_handle = ui.as_weak();
        let filtered_indices = filtered_indices.clone();
        ui.on_request_play_pause(move || {
            if let Ok(mut eng) = engine.lock() {
                if eng.sink.as_ref().map(|s| s.empty()).unwrap_or(true) {
                    if let Some(idx) = *selected.lock().unwrap() {
                        if let Some(item) = songs.get(idx) { let _ = eng.play_file(&item.path); }
                    } else if let Some(&first) = filtered_indices.lock().unwrap().first() {
                        if let Some(item) = songs.get(first) { let _ = eng.play_file(&item.path); }
                    }
                } else {
                    if eng.is_playing() { eng.pause(); } else { eng.resume(); }
                }
                if let Some(ui) = ui_handle.upgrade() { ui.set_is_playing(eng.is_playing()); }
            }
        });
    }

    {
        let engine = engine.clone();
        let songs = songs.clone();
        let selected = selected.clone();
        let ui_handle = ui.as_weak();
        let filtered_indices = filtered_indices.clone();
        let shuffle_c = shuffle.clone();
        let shuffle_order_c = shuffle_order.clone();
        ui.on_request_prev(move || {
            let fi = filtered_indices.lock().unwrap().clone();
            let cur = {
                let s = selected.lock().unwrap();
                (*s).or_else(|| fi.first().copied())
            };
            if let Some(cur_idx) = cur {
                if let Ok(mut eng) = engine.lock() {
                    if eng.current_position() > Duration::from_secs(3) {
                        let _ = eng.seek_to(Duration::ZERO);
                    } else {
                        let idx = if *shuffle_c.lock().unwrap() {
                            // in shuffle mode, pick previous within shuffled list
                            let so = shuffle_order_c.lock().unwrap().clone();
                            let pos = so.iter().position(|&x| x == cur_idx).and_then(|p| p.checked_sub(1));
                            pos.map(|p| so[p]).or_else(|| so.last().copied())
                        } else {
                            fi.iter().position(|&x| x == cur_idx).and_then(|p| p.checked_sub(1)).map(|p| fi[p])
                        };
                        if let Some(idx) = idx { if let Some(item) = songs.get(idx) { let _ = eng.play_file(&item.path); } if let Some(ui) = ui_handle.upgrade() { ui.set_selected_index(idx as i32); } }
                    }
                    if let Some(ui) = ui_handle.upgrade() { ui.set_is_playing(eng.is_playing()); }
                }
            }
        });
    }

    {
        let engine = engine.clone();
        let songs = songs.clone();
        let selected = selected.clone();
        let ui_handle = ui.as_weak();
        let filtered_indices = filtered_indices.clone();
        let shuffle_c2 = shuffle.clone();
        let shuffle_order_c2 = shuffle_order.clone();
        ui.on_request_next(move || {
            let fi = filtered_indices.lock().unwrap().clone();
            let cur = {
                let s = selected.lock().unwrap();
                (*s).or_else(|| fi.first().copied())
            };
            if let Some(cur_idx) = cur {
                let idx_opt = if *shuffle_c2.lock().unwrap() {
                    let so = shuffle_order_c2.lock().unwrap().clone();
                    so.iter().position(|&x| x == cur_idx).and_then(|p| so.get(p+1)).copied().or_else(|| so.first().copied())
                } else {
                    fi.iter().position(|&x| x == cur_idx).and_then(|p| fi.get(p+1)).copied()
                };
                if let Some(idx) = idx_opt { if let Ok(mut eng) = engine.lock() { if let Some(item) = songs.get(idx) { let _ = eng.play_file(&item.path); } if let Some(ui) = ui_handle.upgrade() { ui.set_selected_index(idx as i32); ui.set_is_playing(eng.is_playing()); } } }
            }
        });
    }

    {
        let engine = engine.clone();
        let ui_handle = ui.as_weak();
        ui.on_request_stop(move || {
            if let Ok(mut eng) = engine.lock() { eng.stop(); }
            if let Some(ui) = ui_handle.upgrade() { ui.set_is_playing(false); ui.set_time_text(SharedString::new()); }
        });
    }

    {
        let engine = engine.clone();
        let ui_handle = ui.as_weak();
        ui.on_request_seek(move |value| {
            if let Ok(mut eng) = engine.lock() {
                if let Some(total) = eng.total_duration() {
                    let position = Duration::from_secs_f32(total.as_secs_f32() * value as f32);
                    let _ = eng.seek_to(position);
                    if let Some(ui) = ui_handle.upgrade() {
                        let text = format!("{} / {}", format_time(eng.current_position()), format_time(total));
                        ui.set_time_text(SharedString::from(text));
                        ui.set_progress(value);
                    }
                }
            }
        });
    }

    {
        let search = search.clone();
        let filtered_indices_arc = filtered_indices.clone();
        let ui_handle = ui.as_weak();
        let songs = songs.clone();
        ui.on_search_changed(move |text| {
            {
                let mut s = search.lock().unwrap();
                *s = text.to_string();
            }
            let q = search.lock().unwrap().to_lowercase();
            let mut fi = filtered_indices_arc.lock().unwrap();
            fi.clear();
            for (i, item) in songs.iter().enumerate() {
                if q.is_empty() || item.title.to_lowercase().contains(&q) { fi.push(i); }
            }
            if let Some(ui) = ui_handle.upgrade() {
                let items = fi.iter().map(|&i| Song{ title: SharedString::from(songs[i].title.clone()) }).collect::<Vec<_>>();
                ui.set_songs(slint::ModelRc::new(slint::VecModel::from(items)));
            }
        });
    }

    // Periodic timer to update progress and auto-advance at end
    {
        let engine = engine.clone();
        let ui_handle = ui.as_weak();
        let songs = songs.clone();
        let selected = selected.clone();
        let filtered_indices = filtered_indices.clone();
    let repeat_one = repeat_one.clone();
    let shuffle = shuffle.clone();
    let shuffle_order = shuffle_order.clone();
        let timer = Box::leak(Box::new(slint::Timer::default()));
        timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(200), move || {
            if let Ok(mut eng) = engine.lock() {
                if let Some(total) = eng.total_duration() {
                    let total_secs = total.as_secs_f32().max(0.001);
                    let ratio = (eng.current_position().as_secs_f32() / total_secs).clamp(0.0, 1.0);
                    if let Some(ui) = ui_handle.upgrade() {
                        let text = format!("{} / {}", format_time(eng.current_position()), format_time(total));
                        ui.set_time_text(SharedString::from(text));
                        ui.set_progress(ratio as f32);
                        ui.set_is_playing(eng.is_playing());
                    }
                }
                // Auto-advance
                if eng.sink.as_ref().map(|s| !s.is_paused() && s.empty()).unwrap_or(false) {
                    let fi = filtered_indices.lock().unwrap().clone();
                    let cur_idx = selected.lock().unwrap().or_else(|| fi.first().copied());
                    if let Some(cur_idx) = cur_idx {
                        let next_idx_opt = if *repeat_one.lock().unwrap() {
                            Some(cur_idx)
                        } else if *shuffle.lock().unwrap() {
                            let so = shuffle_order.lock().unwrap().clone();
                            so.iter().position(|&x| x == cur_idx).and_then(|p| so.get(p+1)).copied().or_else(|| so.first().copied())
                        } else {
                            fi.iter().position(|&x| x == cur_idx).and_then(|p| fi.get(p+1)).copied()
                        };
                        if let Some(next_idx) = next_idx_opt {
                            if let Some(item) = songs.get(next_idx) { let _ = eng.play_file(&item.path); }
                            if let Some(ui) = ui_handle.upgrade() { ui.set_selected_index(next_idx as i32); }
                        }
                    }
                }
            }
        });
    }

    // Toggle Repeat / Shuffle / EQ band changes
    {
        let repeat_one_flag = repeat_one.clone();
        let ui_handle = ui.as_weak();
        ui.on_toggle_repeat(move || {
            let mut f = repeat_one_flag.lock().unwrap();
            *f = !*f;
            if let Some(ui) = ui_handle.upgrade() { ui.set_repeat_one(*f); }
        });
    }
    {
        let shuffle_flag = shuffle.clone();
        let filtered_indices = filtered_indices.clone();
        let shuffle_order_arc = shuffle_order.clone();
        let ui_handle = ui.as_weak();
        ui.on_toggle_shuffle(move || {
            let mut s = shuffle_flag.lock().unwrap();
            *s = !*s;
            if *s {
                let mut order = filtered_indices.lock().unwrap().clone();
                order.shuffle(&mut rand::rng());
                *shuffle_order_arc.lock().unwrap() = order;
            }
            if let Some(ui) = ui_handle.upgrade() { ui.set_shuffle(*s); }
        });
    }
    {
        let ui_handle = ui.as_weak();
        ui.on_toggle_eq(move || {
            if let Some(ui) = ui_handle.upgrade() { ui.set_eq_visible(!ui.get_eq_visible()); }
        });
    }
    {
        let eq_gains = eq_gains.clone();
        let engine = engine.clone();
        ui.on_eq_band_changed(move |index, value| {
            if index >= 0 && index < 10 { let idx = index as usize; let mut gains = eq_gains.lock().unwrap(); gains[idx] = (value - 0.5) * 24.0; }
            // To apply new EQ, restart at current position if a track is loaded
            if let Ok(mut eng) = engine.lock() {
                if let Some(path) = eng.current_path.clone() {
                    let pos = eng.current_position();
                    let paused = eng.sink.as_ref().map(|s| s.is_paused()).unwrap_or(false);
                    // Update engine EQ gains
                    if let Ok(g) = eq_gains.lock() { eng.eq.set_gains_db(*g); }
                    let _ = eng.play_from(&path, pos, paused);
                } else {
                    if let Ok(g) = eq_gains.lock() { eng.eq.set_gains_db(*g); }
                }
            }
        });
    }

    ui.run()?;
    Ok(())
}
