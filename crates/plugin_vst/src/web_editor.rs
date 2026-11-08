//! Editor Gráfico Nativo WebView2 (Microsoft Edge Chromium) para VST3 y CLAP.
//!
//! Incrusta la interfaz HTML5/CSS3/JS directamente dentro de la ventana nativa (HWND)
//! que entrega el DAW (Ableton Live, FL Studio, Reaper, Bitwig) o la app standalone.
//!
//! 100% Serverless: Cero servidores HTTP, cero WebSockets, cero puertos TCP abiertos.
//! Todos los assets se cargan en memoria vía protocolo virtual nativo `tepui://app/`.

use std::any::Any;
use std::borrow::Cow;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use ai_bridge::AiWorker;
use crossbeam_channel::Sender;
use nih_plug::prelude::{Editor, GuiContext, Param, ParentWindowHandle};
use serde::{Deserialize, Serialize};
use std::rc::Rc;
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::http::{header::CONTENT_TYPE, Request, Response, StatusCode};
use wry::{PageLoadEvent, Rect, WebView, WebViewBuilder};

use crate::params::{LiveParamStore, SamplerParams};
use crate::GuiMidiEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuiParamId {
    Attack,
    Decay,
    Sustain,
    Release,
    Cutoff,
    Resonance,
    FilterMode,
    Gain,
    SpaceMix,
    RootNote,
    LfoRate,
    LfoDepth,
    LfoTarget,
    AllRefresh,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WebGuiEvent {
    SetParam { id: GuiParamId, normalized: f32 },
}

#[derive(Debug, Serialize)]
struct InitMessage<'a> {
    #[serde(rename = "type")]
    msg_type: &'a str,
    params: ParamValues,
    normalized: ParamNormalized,
    waveform: Vec<f32>,
    sample_info: SampleInfo,
    region: RegionInfo,
    odd_metrics: OddMetrics,
    is_generating: bool,
    param_revision: u32,
    peak_l: f32,
    peak_r: f32,
    active_voices: u32,
    playhead_norm: f32,
    favorites: Vec<String>,
    user_presets: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct TelemetryMessage {
    peak_l: f32,
    peak_r: f32,
    active_voices: u32,
    playhead_norm: f32,
    param_revision: u32,
    is_generating: bool,
    params: ParamValues,
    normalized: ParamNormalized,
    sample_info: SampleInfo,
    region: RegionInfo,
    odd_metrics: OddMetrics,
}

#[derive(Debug, Serialize)]
struct ParamValues {
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
    cutoff: f32,
    resonance: f32,
    filter_mode: i32,
    gain: f32,
    space_mix: f32,
    root_note: i32,
    lfo_rate: f32,
    lfo_depth: f32,
    lfo_target: i32,
    lfo_shape: i32,
    arp_mode: i32,
    mod_matrix_mask: u32,
    wt_pos: f32,
    wt_warp: f32,
    unison_voices: u8,
    unison_detune: f32,
    sub_level: f32,
    fm_from_b: f32,
    osc_b_level: f32,
    drive_amount: f32,
    ott_amount: f32,
    filter_keytrack: f32,
    arp_step_accents: [f32; 16],
    macros: [f32; 4],
    glide_mode: u8,
    env2_target: u8,
    env2_depth: f32,
    env_curve: f32,
    arp_rate_div: u8,
    arp_step_count: u8,
    granular_mode: u8,
    wt_warp_mode: u8,
    fx_mode: u8,
    arp_step_ratchets: [u8; 16],
    arp_step_octaves: [i8; 16],
    midi_cc_map: Vec<(u8, i32)>,
}

#[derive(Debug, Serialize)]
struct ParamNormalized {
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
    cutoff: f32,
    resonance: f32,
    gain: f32,
    space_mix: f32,
    root_note: f32,
    lfo_rate: f32,
    lfo_depth: f32,
}

#[derive(Debug, Serialize)]
struct SampleInfo {
    name: String,
    root_note: u8,
    sample_rate: f32,
    num_samples: usize,
    duration_secs: f32,
}

#[derive(Debug, Serialize)]
struct RegionInfo {
    trim_start_norm: f32,
    trim_end_norm: f32,
    loop_enabled: bool,
    loop_start_norm: f32,
    loop_end_norm: f32,
}

#[derive(Debug, Serialize)]
struct OddMetrics {
    dsp_blocks_processed: u32,
    dsp_limiter_activations: u32,
    dsp_nan_sanitized_count: u32,
    persistence_restores: u32,
    patch_io_count: u32,
    gui_param_sync_count: u32,
    midi_cc_events_processed: u32,
    arp_steps_triggered: u32,
    is_recording_master: bool,
    recorded_master_frames: u32,
    active_keyzones_mask: u8,
    lfo_mod_value: f32,
    zone_names: [Option<String>; 3],
    last_generation_duration_us: u32,
    generation_count: u32,
    synth_backend: String,
    active_prompt: String,
    ui_preset_loads: u32,
    ui_matrix_switches: u32,
    ui_resize_events: u32,
    ui_fav_toggles: u32,
    user_presets_saved: u32,
    mod_matrix_active_routes: u32,
    mod_matrix_dsp_frames: u32,
    npu_cache_hits: u32,
    npu_cache_misses: u32,
    last_velocity: f32,
    velocity_events: u32,
    pitch_bend_semitones: f32,
    mod_wheel: f32,
    sustain_pedal: bool,
    wav_zone_drops: u32,
    ai_presets_synthesized: u32,
    piano_roll_auto_trims: u32,
    last_detected_pitch_hz: f32,
    last_trimmed_samples: u32,
    cloud_tier: String,
    cloud_credits_remaining: u32,
    cloud_kv_cache_hits: u32,
    cloud_daily_neurons: u32,
    cloud_budget_guard_trips: u32,
    vital_serum_dsp_frames: u32,
    custom_wt_draws: u32,
    custom_wt_active: bool,
    hardware_midi_events: u32,
    glide_transitions: u32,
    mpe_poly_events: u32,
    env2_dsp_frames: u32,
    arp_ratchet_hits: u32,
    granular_dsp_frames: u32,
    advanced_filter_frames: u32,
    midi_learn_armed_param: i32,
    midi_learn_bindings_count: u32,
    last_learned_cc: i32,
    wav_drag_exports_count: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersistedPresetCatalogStore {
    #[serde(default)]
    pub favorites: Vec<String>,
    #[serde(default)]
    pub user_presets: Vec<serde_json::Value>,
}

fn preset_catalog_store_path() -> std::path::PathBuf {
    let base = std::env::var("APPDATA")
        .or_else(|_| std::env::var("LOCALAPPDATA"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let dir = base.join("Tepui").join("Presets");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("preset_catalog_state.json")
}

pub fn load_persisted_preset_store() -> PersistedPresetCatalogStore {
    let path = preset_catalog_store_path();
    if let Ok(bytes) = std::fs::read(&path) {
        if let Ok(store) = serde_json::from_slice::<PersistedPresetCatalogStore>(&bytes) {
            return store;
        }
    }
    PersistedPresetCatalogStore::default()
}

pub fn save_persisted_preset_store(store: &PersistedPresetCatalogStore) {
    let path = preset_catalog_store_path();
    if let Ok(json) = serde_json::to_vec_pretty(store) {
        let _ = std::fs::write(&path, json);
    }
}

pub fn toggle_persisted_favorite(preset_id: &str) -> bool {
    let clean = preset_id.trim();
    if clean.is_empty() {
        return false;
    }
    let mut store = load_persisted_preset_store();
    let is_now_fav = if let Some(pos) = store.favorites.iter().position(|x| x == clean) {
        store.favorites.remove(pos);
        false
    } else {
        store.favorites.push(clean.to_string());
        true
    };
    save_persisted_preset_store(&store);
    is_now_fav
}

pub fn append_persisted_user_preset(preset_val: serde_json::Value) {
    let mut store = load_persisted_preset_store();
    store.user_presets.insert(0, preset_val);
    if store.user_presets.len() > 250 {
        store.user_presets.truncate(250);
    }
    save_persisted_preset_store(&store);
}

pub use base_plugin_webview::NativeParentWindow;
pub struct WebViewEditor {
    size_w: Arc<AtomicU32>,
    size_h: Arc<AtomicU32>,
    scale_factor: AtomicU32,
    gui_midi_tx: Sender<GuiMidiEvent>,
    params: Arc<SamplerParams>,
    live_params: Arc<LiveParamStore>,
    ai_worker: Arc<AiWorker>,
    sample_rate: Arc<AtomicU32>,
    gui_param_tx: Option<Sender<WebGuiEvent>>,
}

impl WebViewEditor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        width: u32,
        height: u32,
        gui_midi_tx: Sender<GuiMidiEvent>,
        params: Arc<SamplerParams>,
        live_params: Arc<LiveParamStore>,
        ai_worker: Arc<AiWorker>,
        sample_rate: Arc<AtomicU32>,
        gui_param_tx: Option<Sender<WebGuiEvent>>,
    ) -> Self {
        Self {
            size_w: Arc::new(AtomicU32::new(width)),
            size_h: Arc::new(AtomicU32::new(height)),
            scale_factor: AtomicU32::new(1.0_f32.to_bits()),
            gui_midi_tx,
            params,
            live_params,
            ai_worker,
            sample_rate,
            gui_param_tx,
        }
    }
}

#[cfg(windows)]
pub use base_plugin_webview::win32_pump;
struct WebEditorHandle {
    _webview: Rc<WebView>,
    _web_context: wry::WebContext,
    #[cfg(windows)]
    parent_hwnd: win32_pump::HWND,
}

impl Drop for WebEditorHandle {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            win32_pump::remove_active_webview(self.parent_hwnd);
            win32_pump::KillTimer(self.parent_hwnd, win32_pump::TIMER_ID);
        }
    }
}

// Safety: el handle es liberado exclusivamente en el hilo GUI del DAW al cerrar la ventana.
unsafe impl Send for WebEditorHandle {}

fn load_asset_bytes(filename: &str, embedded: &'static [u8]) -> Cow<'static, [u8]> {
    if std::env::var("TEPUI_DEV_UI").is_ok() {
        let candidates = [
            format!("crates/plugin_vst/ui/{}", filename),
            format!("ui/{}", filename),
        ];
        for path in &candidates {
            if std::path::Path::new(path).exists() {
                if let Ok(bytes) = std::fs::read(path) {
                    return Cow::Owned(bytes);
                }
            }
        }
    }
    Cow::Borrowed(embedded)
}

pub fn extract_waveform_points(sample: &synth_core::SampleBuffer) -> Vec<f32> {
    if sample.is_empty() {
        return Vec::new();
    }
    let samples = &sample.left;
    let num_points = 360.min(samples.len());
    let step = (samples.len() as f32 / num_points as f32).max(1.0);

    let mut points = Vec::with_capacity(num_points);
    for i in 0..num_points {
        let idx = ((i as f32 * step) as usize).min(samples.len() - 1);
        points.push(samples[idx].clamp(-1.0, 1.0));
    }
    points
}

pub fn build_init_state_json(
    params: &SamplerParams,
    live_params: &LiveParamStore,
    ai_worker: &AiWorker,
) -> String {
    let sample = ai_worker.get_sample();
    if let Some(cloud_spec) = ai_worker.take_pending_cloud_patch_spec() {
        apply_patch_spec_to_live_params(&cloud_spec, live_params, false);
    }
    let waveform = extract_waveform_points(&sample);
    let effective_root = live_params.get_root_note().clamp(0, 127);
    let sample_info = SampleInfo {
        name: sample.name.clone(),
        root_note: effective_root as u8,
        sample_rate: sample.sample_rate,
        num_samples: sample.len(),
        duration_secs: sample.duration_seconds(),
    };
    let region = RegionInfo {
        trim_start_norm: live_params.get_trim_start_norm(),
        trim_end_norm: live_params.get_trim_end_norm(),
        loop_enabled: live_params
            .get_loop_enabled_override()
            .unwrap_or(sample.loop_enabled),
        loop_start_norm: live_params.get_loop_start_norm(),
        loop_end_norm: live_params.get_loop_end_norm(),
    };
    let odd_metrics = OddMetrics {
        dsp_blocks_processed: live_params.get_dsp_blocks(),
        dsp_limiter_activations: live_params.get_limiter_activations(),
        dsp_nan_sanitized_count: live_params.get_nan_sanitized(),
        persistence_restores: live_params.get_persistence_restores(),
        patch_io_count: live_params.get_patch_io_count(),
        gui_param_sync_count: live_params.get_gui_param_sync(),
        midi_cc_events_processed: live_params.get_midi_cc_events(),
        arp_steps_triggered: live_params.get_arp_steps_triggered(),
        is_recording_master: live_params.recorder.is_recording(),
        recorded_master_frames: live_params.recorder.recorded_frames(),
        active_keyzones_mask: ai_worker.active_zones_mask(),
        lfo_mod_value: live_params.get_lfo_mod_value(),
        zone_names: ai_worker.zone_names(),
        last_generation_duration_us: ai_worker.last_generation_duration_us(),
        generation_count: ai_worker.generation_count(),
        synth_backend: ai_worker.last_backend(),
        active_prompt: ai_worker.active_prompt(),
        ui_preset_loads: live_params.get_ui_preset_loads(),
        ui_matrix_switches: live_params.get_ui_matrix_switches(),
        ui_resize_events: live_params.get_ui_resize_events(),
        ui_fav_toggles: live_params.get_ui_fav_toggles(),
        user_presets_saved: live_params.get_user_presets_saved(),
        mod_matrix_active_routes: live_params.get_mod_matrix_mask().count_ones(),
        mod_matrix_dsp_frames: live_params.get_mod_matrix_dsp_frames(),
        npu_cache_hits: ai_worker.cache_hits(),
        npu_cache_misses: ai_worker.cache_misses(),
        last_velocity: live_params.get_last_velocity(),
        velocity_events: live_params.get_velocity_events(),
        pitch_bend_semitones: live_params.get_pitch_bend_semitones(),
        mod_wheel: live_params.get_mod_wheel(),
        sustain_pedal: live_params.get_sustain_pedal(),
        wav_zone_drops: live_params.get_wav_zone_drops(),
        ai_presets_synthesized: ai_worker.ai_presets_synthesized(),
        piano_roll_auto_trims: ai_worker.piano_roll_auto_trims(),
        last_detected_pitch_hz: ai_worker.last_detected_pitch_hz(),
        last_trimmed_samples: ai_worker.last_trimmed_samples(),
        cloud_tier: ai_worker.cloud_tier(),
        cloud_credits_remaining: ai_worker.cloud_credits_remaining(),
        cloud_kv_cache_hits: ai_worker.cloud_kv_cache_hits(),
        cloud_daily_neurons: ai_worker.cloud_daily_neurons(),
        cloud_budget_guard_trips: ai_worker.cloud_budget_guard_trips(),
        vital_serum_dsp_frames: live_params.get_vital_serum_dsp_frames(),
        custom_wt_draws: live_params.get_custom_wavetable_draws(),
        custom_wt_active: live_params.is_custom_wavetable_active(),
        hardware_midi_events: crate::audio_devices::hardware_midi_events_count(),
        glide_transitions: live_params.get_glide_transitions(),
        mpe_poly_events: live_params.get_mpe_poly_events(),
        env2_dsp_frames: live_params.get_env2_dsp_frames(),
        arp_ratchet_hits: live_params.get_arp_ratchet_hits(),
        granular_dsp_frames: live_params.get_granular_dsp_frames(),
        advanced_filter_frames: live_params.get_advanced_filter_frames(),
        midi_learn_armed_param: live_params.get_midi_learn_armed_param(),
        midi_learn_bindings_count: live_params.get_midi_learn_bindings_count(),
        last_learned_cc: live_params.get_last_learned_cc(),
        wav_drag_exports_count: live_params.get_wav_drag_exports(),
    };
    let preset_store = load_persisted_preset_store();

    let init_msg = InitMessage {
        msg_type: "init",
        params: ParamValues {
            attack: live_params.get_attack(),
            decay: live_params.get_decay(),
            sustain: live_params.get_sustain(),
            release: live_params.get_release(),
            cutoff: live_params.get_cutoff(),
            resonance: live_params.get_resonance(),
            filter_mode: live_params.get_filter_mode().to_index() as i32,
            gain: live_params.get_gain(),
            space_mix: live_params.get_space_mix(),
            root_note: effective_root,
            lfo_rate: live_params.get_lfo_rate(),
            lfo_depth: live_params.get_lfo_depth(),
            lfo_target: live_params.get_lfo_target().as_i32(),
            lfo_shape: live_params.get_lfo_shape().as_i32(),
            arp_mode: live_params.get_arp_mode().as_i32(),
            mod_matrix_mask: live_params.get_mod_matrix_mask(),
            wt_pos: live_params.get_wt_pos(),
            wt_warp: live_params.get_wt_warp(),
            unison_voices: live_params.get_unison_voices(),
            unison_detune: live_params.get_unison_detune(),
            sub_level: live_params.get_sub_level(),
            fm_from_b: live_params.get_fm_from_b(),
            osc_b_level: live_params.get_osc_b_level(),
            drive_amount: live_params.get_drive_amount(),
            ott_amount: live_params.get_ott_amount(),
            filter_keytrack: live_params.get_filter_keytrack(),
            arp_step_accents: live_params.get_arp_step_accents(),
            macros: live_params.get_macros(),
            glide_mode: live_params.get_glide_mode(),
            env2_target: live_params.get_env2_target(),
            env2_depth: live_params.get_env2_depth(),
            env_curve: live_params.get_env_curve(),
            arp_rate_div: live_params.get_arp_rate_div(),
            arp_step_count: live_params.get_arp_step_count(),
            granular_mode: live_params.get_granular_mode(),
            wt_warp_mode: live_params.get_wt_warp_mode(),
            fx_mode: live_params.get_fx_mode(),
            arp_step_ratchets: live_params.get_arp_step_ratchets(),
            arp_step_octaves: live_params.get_arp_step_octaves(),
            midi_cc_map: live_params.export_midi_cc_map(),
        },
        normalized: ParamNormalized {
            attack: params.attack.preview_normalized(live_params.get_attack()),
            decay: params.decay.preview_normalized(live_params.get_decay()),
            sustain: params.sustain.preview_normalized(live_params.get_sustain()),
            release: params.release.preview_normalized(live_params.get_release()),
            cutoff: params.cutoff.preview_normalized(live_params.get_cutoff()),
            resonance: params
                .resonance
                .preview_normalized(live_params.get_resonance()),
            gain: params.gain.preview_normalized(live_params.get_gain()),
            space_mix: params
                .space_mix
                .preview_normalized(live_params.get_space_mix()),
            root_note: (effective_root as f32 / 127.0).clamp(0.0, 1.0),
            lfo_rate: params
                .lfo_rate
                .preview_normalized(live_params.get_lfo_rate()),
            lfo_depth: params
                .lfo_depth
                .preview_normalized(live_params.get_lfo_depth()),
        },
        waveform,
        sample_info,
        region,
        odd_metrics,
        is_generating: ai_worker.is_generating(),
        param_revision: live_params.get_revision(),
        peak_l: live_params.get_peak_l(),
        peak_r: live_params.get_peak_r(),
        active_voices: live_params.get_active_voices(),
        playhead_norm: live_params.get_playhead_norm(),
        favorites: preset_store.favorites,
        user_presets: preset_store.user_presets,
    };

    serde_json::to_string(&init_msg).unwrap_or_else(|_| "{}".to_string())
}

pub fn build_telemetry_json(
    params: &SamplerParams,
    live_params: &LiveParamStore,
    ai_worker: &AiWorker,
) -> String {
    let sample = ai_worker.get_sample();
    if let Some(cloud_spec) = ai_worker.take_pending_cloud_patch_spec() {
        apply_patch_spec_to_live_params(&cloud_spec, live_params, false);
    }
    let effective_root = live_params.get_root_note().clamp(0, 127);
    let msg = TelemetryMessage {
        peak_l: live_params.get_peak_l(),
        peak_r: live_params.get_peak_r(),
        active_voices: live_params.get_active_voices(),
        playhead_norm: live_params.get_playhead_norm(),
        param_revision: live_params.get_revision(),
        is_generating: ai_worker.is_generating(),
        params: ParamValues {
            attack: live_params.get_attack(),
            decay: live_params.get_decay(),
            sustain: live_params.get_sustain(),
            release: live_params.get_release(),
            cutoff: live_params.get_cutoff(),
            resonance: live_params.get_resonance(),
            filter_mode: live_params.get_filter_mode().to_index() as i32,
            gain: live_params.get_gain(),
            space_mix: live_params.get_space_mix(),
            root_note: effective_root,
            lfo_rate: live_params.get_lfo_rate(),
            lfo_depth: live_params.get_lfo_depth(),
            lfo_target: live_params.get_lfo_target().as_i32(),
            lfo_shape: live_params.get_lfo_shape().as_i32(),
            arp_mode: live_params.get_arp_mode().as_i32(),
            mod_matrix_mask: live_params.get_mod_matrix_mask(),
            wt_pos: live_params.get_wt_pos(),
            wt_warp: live_params.get_wt_warp(),
            unison_voices: live_params.get_unison_voices(),
            unison_detune: live_params.get_unison_detune(),
            sub_level: live_params.get_sub_level(),
            fm_from_b: live_params.get_fm_from_b(),
            osc_b_level: live_params.get_osc_b_level(),
            drive_amount: live_params.get_drive_amount(),
            ott_amount: live_params.get_ott_amount(),
            filter_keytrack: live_params.get_filter_keytrack(),
            arp_step_accents: live_params.get_arp_step_accents(),
            macros: live_params.get_macros(),
            glide_mode: live_params.get_glide_mode(),
            env2_target: live_params.get_env2_target(),
            env2_depth: live_params.get_env2_depth(),
            env_curve: live_params.get_env_curve(),
            arp_rate_div: live_params.get_arp_rate_div(),
            arp_step_count: live_params.get_arp_step_count(),
            granular_mode: live_params.get_granular_mode(),
            wt_warp_mode: live_params.get_wt_warp_mode(),
            fx_mode: live_params.get_fx_mode(),
            arp_step_ratchets: live_params.get_arp_step_ratchets(),
            arp_step_octaves: live_params.get_arp_step_octaves(),
            midi_cc_map: live_params.export_midi_cc_map(),
        },
        normalized: ParamNormalized {
            attack: params.attack.preview_normalized(live_params.get_attack()),
            decay: params.decay.preview_normalized(live_params.get_decay()),
            sustain: params.sustain.preview_normalized(live_params.get_sustain()),
            release: params.release.preview_normalized(live_params.get_release()),
            cutoff: params.cutoff.preview_normalized(live_params.get_cutoff()),
            resonance: params
                .resonance
                .preview_normalized(live_params.get_resonance()),
            gain: params.gain.preview_normalized(live_params.get_gain()),
            space_mix: params
                .space_mix
                .preview_normalized(live_params.get_space_mix()),
            root_note: (effective_root as f32 / 127.0).clamp(0.0, 1.0),
            lfo_rate: params
                .lfo_rate
                .preview_normalized(live_params.get_lfo_rate()),
            lfo_depth: params
                .lfo_depth
                .preview_normalized(live_params.get_lfo_depth()),
        },
        sample_info: SampleInfo {
            name: sample.name.clone(),
            root_note: effective_root as u8,
            sample_rate: sample.sample_rate,
            num_samples: sample.len(),
            duration_secs: sample.duration_seconds(),
        },
        region: RegionInfo {
            trim_start_norm: live_params.get_trim_start_norm(),
            trim_end_norm: live_params.get_trim_end_norm(),
            loop_enabled: live_params
                .get_loop_enabled_override()
                .unwrap_or(sample.loop_enabled),
            loop_start_norm: live_params.get_loop_start_norm(),
            loop_end_norm: live_params.get_loop_end_norm(),
        },
        odd_metrics: OddMetrics {
            dsp_blocks_processed: live_params.get_dsp_blocks(),
            dsp_limiter_activations: live_params.get_limiter_activations(),
            dsp_nan_sanitized_count: live_params.get_nan_sanitized(),
            persistence_restores: live_params.get_persistence_restores(),
            patch_io_count: live_params.get_patch_io_count(),
            gui_param_sync_count: live_params.get_gui_param_sync(),
            midi_cc_events_processed: live_params.get_midi_cc_events(),
            arp_steps_triggered: live_params.get_arp_steps_triggered(),
            is_recording_master: live_params.recorder.is_recording(),
            recorded_master_frames: live_params.recorder.recorded_frames(),
            active_keyzones_mask: ai_worker.active_zones_mask(),
            lfo_mod_value: live_params.get_lfo_mod_value(),
            zone_names: ai_worker.zone_names(),
            last_generation_duration_us: ai_worker.last_generation_duration_us(),
            generation_count: ai_worker.generation_count(),
            synth_backend: ai_worker.last_backend(),
            active_prompt: ai_worker.active_prompt(),
            ui_preset_loads: live_params.get_ui_preset_loads(),
            ui_matrix_switches: live_params.get_ui_matrix_switches(),
            ui_resize_events: live_params.get_ui_resize_events(),
            ui_fav_toggles: live_params.get_ui_fav_toggles(),
            user_presets_saved: live_params.get_user_presets_saved(),
            mod_matrix_active_routes: live_params.get_mod_matrix_mask().count_ones(),
            mod_matrix_dsp_frames: live_params.get_mod_matrix_dsp_frames(),
            npu_cache_hits: ai_worker.cache_hits(),
            npu_cache_misses: ai_worker.cache_misses(),
            last_velocity: live_params.get_last_velocity(),
            velocity_events: live_params.get_velocity_events(),
            pitch_bend_semitones: live_params.get_pitch_bend_semitones(),
            mod_wheel: live_params.get_mod_wheel(),
            sustain_pedal: live_params.get_sustain_pedal(),
            wav_zone_drops: live_params.get_wav_zone_drops(),
            ai_presets_synthesized: ai_worker.ai_presets_synthesized(),
            piano_roll_auto_trims: ai_worker.piano_roll_auto_trims(),
            last_detected_pitch_hz: ai_worker.last_detected_pitch_hz(),
            last_trimmed_samples: ai_worker.last_trimmed_samples(),
            cloud_tier: ai_worker.cloud_tier(),
            cloud_credits_remaining: ai_worker.cloud_credits_remaining(),
            cloud_kv_cache_hits: ai_worker.cloud_kv_cache_hits(),
            cloud_daily_neurons: ai_worker.cloud_daily_neurons(),
            cloud_budget_guard_trips: ai_worker.cloud_budget_guard_trips(),
            vital_serum_dsp_frames: live_params.get_vital_serum_dsp_frames(),
            custom_wt_draws: live_params.get_custom_wavetable_draws(),
            custom_wt_active: live_params.is_custom_wavetable_active(),
            hardware_midi_events: crate::audio_devices::hardware_midi_events_count(),
            glide_transitions: live_params.get_glide_transitions(),
            mpe_poly_events: live_params.get_mpe_poly_events(),
            env2_dsp_frames: live_params.get_env2_dsp_frames(),
            arp_ratchet_hits: live_params.get_arp_ratchet_hits(),
            granular_dsp_frames: live_params.get_granular_dsp_frames(),
            advanced_filter_frames: live_params.get_advanced_filter_frames(),
            midi_learn_armed_param: live_params.get_midi_learn_armed_param(),
            midi_learn_bindings_count: live_params.get_midi_learn_bindings_count(),
            last_learned_cc: live_params.get_last_learned_cc(),
            wav_drag_exports_count: live_params.get_wav_drag_exports(),
        },
    };
    serde_json::to_string(&msg).unwrap_or_else(|_| "{}".to_string())
}

fn extract_sample_name_from_query(query: Option<&str>) -> Option<String> {
    let q = query?;
    for pair in q.split('&') {
        if let Some(val) = pair.strip_prefix("name=") {
            let decoded = val
                .replace("%20", " ")
                .replace('+', " ")
                .replace("%28", "(")
                .replace("%29", ")")
                .replace("%5B", "[")
                .replace("%5D", "]")
                .replace("%2D", "-")
                .replace("%5F", "_");
            let trimmed = decoded.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn extract_zone_index_from_query(query: Option<&str>) -> Option<usize> {
    let q = query?;
    for pair in q.split('&') {
        if let Some(val) = pair.strip_prefix("zone=") {
            if let Ok(idx) = val.trim().parse::<usize>() {
                if idx < 3 {
                    return Some(idx);
                }
            }
        }
    }
    None
}

fn build_html_bundle(
    params: &SamplerParams,
    live_params: &LiveParamStore,
    ai_worker: &AiWorker,
) -> String {
    let raw_html = load_asset_bytes("index.html", include_bytes!("../ui/index.html"));
    let raw_css = load_asset_bytes("style.css", include_bytes!("../ui/style.css"));
    let raw_js = load_asset_bytes("app.js", include_bytes!("../ui/app.js"));

    let html_str = String::from_utf8_lossy(&raw_html);
    let css_str = String::from_utf8_lossy(&raw_css);
    let js_str = String::from_utf8_lossy(&raw_js);

    let init_json = build_init_state_json(params, live_params, ai_worker);
    let config = crate::audio_devices::load_config();
    let audio_status = crate::audio_devices::enumerate_audio_system_cached(&config);
    let audio_json = serde_json::to_string(&audio_status).unwrap_or_else(|_| "{}".to_string());

    let state_injection = format!(
        r#"<script>
window.__TEPUI_INIT_STATE__ = {};
window.__TEPUI_AUDIO_STATUS__ = {};
window.__TEPUI_NATIVE_IPC__ = true;
if (typeof window.ipc === 'undefined' && window.chrome && window.chrome.webview) {{
    window.ipc = {{ postMessage: function(msg) {{ window.chrome.webview.postMessage(msg); }} }};
}}
try {{
    if (window.chrome && window.chrome.webview) {{
        window.chrome.webview.postMessage('log:State injection script executed successfully');
    }}
}} catch (_) {{}}
</script>"#,
        init_json, audio_json
    );

    let inlined_css = format!("<style>\n{}\n</style>", css_str);
    let inlined_js = format!("<script>\n{}\n</script>", js_str);
    let icon_data_uri = format!(
        "data:image/png;base64,{}",
        synth_core::base64_encode(include_bytes!("../ui/tepui-icon.png"))
    );

    // Inyectar el icono en Base64 para renderizado instantáneo sin latencia
    let html_with_icon = html_str
        .replace(
            r#"href="tepui-icon.png""#,
            &format!(r#"href="{}""#, icon_data_uri),
        )
        .replace(
            r#"src="tepui-icon.png""#,
            &format!(r#"src="{}""#, icon_data_uri),
        );

    // Inyectar el CSS directamente
    let html_with_css = if html_with_icon.contains(r#"<link rel="stylesheet" href="style.css">"#) {
        html_with_icon.replace(r#"<link rel="stylesheet" href="style.css">"#, &inlined_css)
    } else {
        html_with_icon.replace("</head>", &format!("{}\n</head>", inlined_css))
    };

    // Inyectar el script con estado pre-inicializado
    let full_html = if html_with_css.contains(r#"<script src="app.js"></script>"#) {
        html_with_css.replace(
            r#"<script src="app.js"></script>"#,
            &format!("{}\n{}", state_injection, inlined_js),
        )
    } else {
        html_with_css.replace(
            "</body>",
            &format!("{}\n{}\n</body>", state_injection, inlined_js),
        )
    };

    full_html
}

pub fn set_param_from_client(
    params: &SamplerParams,
    live_params: &LiveParamStore,
    gui_param_tx: &Option<Sender<WebGuiEvent>>,
    id: &str,
    norm_val: f32,
) {
    let clamped = norm_val.clamp(0.0, 1.0);
    match id {
        "attack" => {
            let plain = params.attack.preview_plain(clamped);
            live_params.set_attack(plain);
            if let Some(tx) = gui_param_tx {
                let _ = tx.try_send(WebGuiEvent::SetParam {
                    id: GuiParamId::Attack,
                    normalized: clamped,
                });
            }
        }
        "decay" => {
            let plain = params.decay.preview_plain(clamped);
            live_params.set_decay(plain);
            if let Some(tx) = gui_param_tx {
                let _ = tx.try_send(WebGuiEvent::SetParam {
                    id: GuiParamId::Decay,
                    normalized: clamped,
                });
            }
        }
        "sustain" => {
            let plain = params.sustain.preview_plain(clamped);
            live_params.set_sustain(plain);
            if let Some(tx) = gui_param_tx {
                let _ = tx.try_send(WebGuiEvent::SetParam {
                    id: GuiParamId::Sustain,
                    normalized: clamped,
                });
            }
        }
        "release" => {
            let plain = params.release.preview_plain(clamped);
            live_params.set_release(plain);
            if let Some(tx) = gui_param_tx {
                let _ = tx.try_send(WebGuiEvent::SetParam {
                    id: GuiParamId::Release,
                    normalized: clamped,
                });
            }
        }
        "cutoff" => {
            let plain = params.cutoff.preview_plain(clamped);
            live_params.set_cutoff(plain);
            if let Some(tx) = gui_param_tx {
                let _ = tx.try_send(WebGuiEvent::SetParam {
                    id: GuiParamId::Cutoff,
                    normalized: clamped,
                });
            }
        }
        "resonance" => {
            let plain = params.resonance.preview_plain(clamped);
            live_params.set_resonance(plain);
            if let Some(tx) = gui_param_tx {
                let _ = tx.try_send(WebGuiEvent::SetParam {
                    id: GuiParamId::Resonance,
                    normalized: clamped,
                });
            }
        }
        "gain" => {
            let plain = params.gain.preview_plain(clamped);
            live_params.set_gain(plain);
            if let Some(tx) = gui_param_tx {
                let _ = tx.try_send(WebGuiEvent::SetParam {
                    id: GuiParamId::Gain,
                    normalized: clamped,
                });
            }
        }
        "space_mix" => {
            let plain = params.space_mix.preview_plain(clamped);
            live_params.set_space_mix(plain);
            if let Some(tx) = gui_param_tx {
                let _ = tx.try_send(WebGuiEvent::SetParam {
                    id: GuiParamId::SpaceMix,
                    normalized: clamped,
                });
            }
        }
        "lfo_rate" => {
            let plain = params.lfo_rate.preview_plain(clamped);
            live_params.set_lfo_rate(plain);
            if let Some(tx) = gui_param_tx {
                let _ = tx.try_send(WebGuiEvent::SetParam {
                    id: GuiParamId::LfoRate,
                    normalized: clamped,
                });
            }
        }
        "lfo_depth" => {
            let plain = params.lfo_depth.preview_plain(clamped);
            live_params.set_lfo_depth(plain);
            if let Some(tx) = gui_param_tx {
                let _ = tx.try_send(WebGuiEvent::SetParam {
                    id: GuiParamId::LfoDepth,
                    normalized: clamped,
                });
            }
        }
        "lfo_target" => {
            let target = (norm_val.round() as i32).clamp(0, 3);
            live_params.set_lfo_target(target);
            live_params.bump_revision();
            if let Some(tx) = gui_param_tx {
                let _ = tx.try_send(WebGuiEvent::SetParam {
                    id: GuiParamId::LfoTarget,
                    normalized: target as f32,
                });
            }
        }
        "arp_mode" => {
            let mode = (norm_val.round() as i32).clamp(0, 4);
            live_params.set_arp_mode(mode);
            live_params.bump_revision();
        }
        "wt_pos" => {
            live_params.set_wt_pos(clamped);
            live_params.bump_revision();
        }
        "wt_warp" => {
            live_params.set_wt_warp(clamped);
            live_params.bump_revision();
        }
        "unison_voices" => {
            let v = if norm_val > 1.05 {
                (norm_val.round() as u8).clamp(1, 8)
            } else {
                (1.0 + clamped * 7.0).round() as u8
            };
            live_params.set_unison_voices(v);
            live_params.bump_revision();
        }
        "unison_detune" => {
            live_params.set_unison_detune(clamped);
            live_params.bump_revision();
        }
        "sub_level" => {
            live_params.set_sub_level(clamped);
            live_params.bump_revision();
        }
        "fm_from_b" => {
            live_params.set_fm_from_b(clamped);
            // Cuando FM A<-B se activa, encender también el oscilador portador OSC B
            live_params.set_osc_b_level(if clamped > 0.001 {
                (clamped * 0.75).clamp(0.15, 0.85)
            } else {
                0.0
            });
            live_params.bump_revision();
        }
        "osc_b_level" => {
            live_params.set_osc_b_level(clamped);
            live_params.bump_revision();
        }
        "drive" | "drive_amount" => {
            live_params.set_drive_amount(clamped);
            live_params.bump_revision();
        }
        "ott" | "ott_amount" => {
            live_params.set_ott_amount(clamped);
            live_params.bump_revision();
        }
        "lfo_shape" => {
            let shape = (norm_val.round() as i32).clamp(0, 5);
            live_params.set_lfo_shape(shape);
            live_params.bump_revision();
        }
        "macro1" => {
            live_params.set_macro(0, clamped);
            live_params.bump_revision();
        }
        "macro2" => {
            live_params.set_macro(1, clamped);
            live_params.bump_revision();
        }
        "macro3" => {
            live_params.set_macro(2, clamped);
            live_params.bump_revision();
        }
        "macro4" => {
            live_params.set_macro(3, clamped);
            live_params.bump_revision();
        }
        "trim_start" => {
            live_params.set_trim_start_norm(clamped);
            live_params.bump_revision();
        }
        "trim_end" => {
            live_params.set_trim_end_norm(clamped);
            live_params.bump_revision();
        }
        "loop_start" => {
            live_params.set_loop_start_norm(clamped);
            live_params.bump_revision();
        }
        "loop_end" => {
            live_params.set_loop_end_norm(clamped);
            live_params.bump_revision();
        }
        "loop_enabled" => {
            live_params.set_loop_enabled_override(Some(norm_val >= 0.5));
            live_params.bump_revision();
        }
        "filter_mode" => {
            let mode = (norm_val.round() as i32).clamp(0, 8);
            live_params.set_filter_mode(mode);
            live_params.bump_revision();
            if let Some(tx) = gui_param_tx {
                let _ = tx.try_send(WebGuiEvent::SetParam {
                    id: GuiParamId::FilterMode,
                    normalized: mode as f32,
                });
            }
        }
        "root_note" => {
            let note = if norm_val > 1.0 {
                (norm_val.round() as i32).clamp(0, 127)
            } else {
                params.root_note.preview_plain(clamped).clamp(0, 127)
            };
            live_params.set_root_note(note);
            live_params.bump_revision();
            if let Some(tx) = gui_param_tx {
                let _ = tx.try_send(WebGuiEvent::SetParam {
                    id: GuiParamId::RootNote,
                    normalized: (note as f32 / 127.0).clamp(0.0, 1.0),
                });
            }
        }
        _ => {}
    }
    if matches!(
        id,
        "attack"
            | "decay"
            | "sustain"
            | "release"
            | "cutoff"
            | "resonance"
            | "gain"
            | "space_mix"
            | "lfo_rate"
            | "lfo_depth"
    ) {
        live_params.bump_revision();
    }
}

pub(crate) fn handle_native_ipc_message(
    body: &str,
    gui_midi_tx: &Sender<GuiMidiEvent>,
    params: &Arc<SamplerParams>,
    live_params: &Arc<LiveParamStore>,
    ai_worker: &Arc<AiWorker>,
    sample_rate: &Arc<AtomicU32>,
    gui_param_tx: &Option<Sender<WebGuiEvent>>,
) {
    let mut trimmed = body.trim();
    if ((trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
        && trimmed.len() >= 2
    {
        trimmed = &trimmed[1..trimmed.len() - 1];
    }
    let trimmed = trimmed.trim();
    if trimmed.is_empty() {
        return;
    }

    eprintln!("[TEPUI IPC RECV] {}", trimmed);

    // Logging desde el frontend JS
    if let Some(log_msg) = trimmed.strip_prefix("log:") {
        eprintln!("[TEPUI JS LOG] {}", log_msg);
        return;
    }

    // 0. Ultra-fast compact Panic / Kill all: "k" o "panic" (0 asignaciones)
    if trimmed == "k" || trimmed == "panic" || trimmed == "all_notes_off" {
        let _ = gui_midi_tx.try_send(GuiMidiEvent::AllNotesOff);
        return;
    }

    // 0b. Custom Freehand Wavetable Draw & Audio-to-Wavetable Resynthesis IPC:
    //     "wtdraw:v0,v1,..." | "wtdraw:clear" | "wtdraw:from_sample"
    if let Some(rest) = trimmed.strip_prefix("wtdraw:") {
        let cmd = rest.trim();
        if cmd.eq_ignore_ascii_case("clear") || cmd.eq_ignore_ascii_case("off") {
            live_params.clear_custom_wavetable();
            live_params.bump_gui_param_sync();
            live_params.bump_revision();
            return;
        }
        if cmd.eq_ignore_ascii_case("from_sample") || cmd.eq_ignore_ascii_case("wav_to_wt") {
            let sample = ai_worker.get_sample();
            let cycle = sample.extract_wavetable_cycle_64();
            live_params.set_custom_wavetable(&cycle);
            live_params.bump_gui_param_sync();
            live_params.bump_revision();
            return;
        }
        let mut pts = Vec::with_capacity(64);
        for tok in cmd.split(',') {
            if let Ok(v) = tok.trim().parse::<f32>() {
                pts.push(v.clamp(-1.0, 1.0));
                if pts.len() >= 128 {
                    break;
                }
            }
        }
        if !pts.is_empty() {
            live_params.set_custom_wavetable(&pts);
            live_params.bump_gui_param_sync();
            live_params.bump_revision();
        }
        return;
    }

    if trimmed == "restart_standalone" {
        let _ = crate::audio_devices::restart_standalone_app();
        return;
    }

    if let Some(cfg_json) = trimmed.strip_prefix("save_audio_config:") {
        let clean_json = cfg_json.trim();
        let parsed =
            serde_json::from_str::<crate::audio_devices::StandaloneAudioConfig>(clean_json)
                .or_else(|_| {
                    serde_json::from_str::<String>(clean_json).and_then(|unescaped| {
                        serde_json::from_str::<crate::audio_devices::StandaloneAudioConfig>(
                            &unescaped,
                        )
                    })
                })
                .or_else(|_| {
                    let unescaped = clean_json.replace(r#"\""#, r#"""#);
                    serde_json::from_str::<crate::audio_devices::StandaloneAudioConfig>(&unescaped)
                });

        match parsed {
            Ok(config) => match crate::audio_devices::save_config(&config) {
                Ok(_) => {
                    let _ = crate::audio_devices::connect_hardware_midi_port(
                        config.midi_input.as_deref(),
                    );
                    println!("[TEPUI CONFIG] Guardado via IPC exitosamente: {:?}", config);
                }
                Err(e) => eprintln!("[TEPUI CONFIG ERROR] Fallo al escribir archivo: {}", e),
            },
            Err(e) => {
                eprintln!(
                    "[TEPUI CONFIG ERROR] JSON inválido en IPC '{}': {}",
                    clean_json, e
                );
            }
        }
        return;
    }

    if let Some(rest) = trimmed.strip_prefix("save_cloud_license:") {
        let mut clean_json = rest.trim();
        if clean_json.ends_with(r#"\""#) {
            clean_json = &clean_json[..clean_json.len() - 2];
        } else if clean_json.ends_with('"') && !clean_json.starts_with('"') {
            clean_json = &clean_json[..clean_json.len() - 1];
        }
        let parsed =
            serde_json::from_str::<ai_bridge::CloudLicenseConfig>(clean_json).or_else(|_| {
                let unescaped = clean_json.replace(r#"\""#, r#"""#);
                serde_json::from_str::<ai_bridge::CloudLicenseConfig>(&unescaped)
            });
        if let Ok(mut cfg) = parsed {
            let existing = ai_bridge::load_cloud_license_config();
            if cfg.device_id.trim().is_empty() {
                cfg.device_id = existing.device_id;
            }
            if cfg.redeemed_credits == 0 && existing.redeemed_credits > 0 {
                cfg.redeemed_credits = existing.redeemed_credits;
            }
            if cfg.redeemed_codes.is_empty() && !existing.redeemed_codes.is_empty() {
                cfg.redeemed_codes = existing.redeemed_codes;
            }
            if !cfg.pro_tier_unlocked && existing.pro_tier_unlocked {
                cfg.pro_tier_unlocked = true;
            }
            if cfg.credits_spent == 0 && existing.credits_spent > 0 {
                cfg.credits_spent = existing.credits_spent;
            }
            let _ = ai_bridge::save_cloud_license_config(&cfg);
            let quota = ai_bridge::verify_cloud_license_quota(&cfg);
            ai_worker.apply_cloud_quota_report(&quota);
            live_params.bump_revision();
        }
        return;
    }

    if let Some(code_rest) = trimmed
        .strip_prefix("redeem_cloud_code:")
        .or_else(|| trimmed.strip_prefix("redeem_cloud_code,"))
    {
        let code = code_rest.trim().trim_matches('"');
        let mut cfg = ai_bridge::load_cloud_license_config();
        if let Ok(quota) = ai_bridge::redeem_cloud_license_code(code, &mut cfg) {
            let _ = ai_bridge::save_cloud_license_config(&cfg);
            ai_worker.apply_cloud_quota_report(&quota);
            live_params.bump_revision();
        }
        return;
    }

    // 1. Ultra-fast compact path: "1,note,vel" o "1:note:vel" o "n:note:vel" (NoteOn con 0 asignaciones)
    let note_on_rest = trimmed
        .strip_prefix("1,")
        .or_else(|| trimmed.strip_prefix("1:"))
        .or_else(|| trimmed.strip_prefix("n:"))
        .or_else(|| trimmed.strip_prefix("note_on:"));

    if let Some(rest) = note_on_rest {
        let delimiter = if rest.contains(',') { ',' } else { ':' };
        let mut parts = rest.split(delimiter);
        if let (Some(note_str), Some(vel_str)) = (parts.next(), parts.next()) {
            if let (Ok(note), Ok(vel)) =
                (note_str.trim().parse::<u8>(), vel_str.trim().parse::<f32>())
            {
                let clamped_vel = vel.clamp(0.01, 1.0);
                live_params.record_velocity_event(clamped_vel);
                let intervals = synth_core::expand_chord_intervals(live_params.get_chord_mode());
                for (idx, &semi) in intervals.iter().enumerate() {
                    let chord_note = (note as i32 + semi as i32).clamp(0, 127) as u8;
                    let voice_vel = if idx == 0 {
                        clamped_vel
                    } else {
                        (clamped_vel * 0.88).clamp(0.05, 1.0)
                    };
                    let _ = gui_midi_tx.try_send(GuiMidiEvent::NoteOn(chord_note, voice_vel));
                }
                return;
            }
        }
    }

    // 1b. Ultra-fast Pitch Bend IPC: "pb,<semitones>" o "pitch_bend:<semitones>"
    let pb_rest = trimmed
        .strip_prefix("pb,")
        .or_else(|| trimmed.strip_prefix("pb:"))
        .or_else(|| trimmed.strip_prefix("pitch_bend:"));
    if let Some(rest) = pb_rest {
        if let Ok(semi) = rest.trim().parse::<f32>() {
            let clamped = semi.clamp(-12.0, 12.0);
            live_params.set_pitch_bend_semitones(clamped);
            live_params.bump_midi_cc_events();
            live_params.bump_revision();
            let _ = gui_midi_tx.try_send(GuiMidiEvent::PitchBend(clamped));
            return;
        }
    }

    // 1c. Ultra-fast Mod Wheel (CC#1) IPC: "mw,<0..1>" o "mod_wheel:<0..1>"
    let mw_rest = trimmed
        .strip_prefix("mw,")
        .or_else(|| trimmed.strip_prefix("mw:"))
        .or_else(|| trimmed.strip_prefix("mod_wheel:"));
    if let Some(rest) = mw_rest {
        if let Ok(val) = rest.trim().parse::<f32>() {
            let clamped = val.clamp(0.0, 1.0);
            live_params.set_mod_wheel(clamped);
            live_params.bump_midi_cc_events();
            live_params.bump_revision();
            let _ = gui_midi_tx.try_send(GuiMidiEvent::ModWheel(clamped));
            return;
        }
    }

    // 1d. Ultra-fast Sustain Pedal (CC#64) IPC: "sus,<0|1>" o "sustain:<0|1>"
    let sus_rest = trimmed
        .strip_prefix("sus,")
        .or_else(|| trimmed.strip_prefix("sus:"))
        .or_else(|| trimmed.strip_prefix("sustain:"));
    if let Some(rest) = sus_rest {
        let cmd = rest.trim();
        let down = cmd == "1" || cmd.eq_ignore_ascii_case("true") || cmd.eq_ignore_ascii_case("on");
        live_params.set_sustain_pedal(down);
        live_params.bump_midi_cc_events();
        live_params.bump_revision();
        let _ = gui_midi_tx.try_send(GuiMidiEvent::SustainPedal(down));
        return;
    }

    // 2. Ultra-fast compact path: "0,note" o "0:note" o "f:note" o "note_off:note" (NoteOff)
    let note_off_rest = trimmed
        .strip_prefix("0,")
        .or_else(|| trimmed.strip_prefix("0:"))
        .or_else(|| trimmed.strip_prefix("f:"))
        .or_else(|| trimmed.strip_prefix("note_off:"));

    if let Some(rest) = note_off_rest {
        if let Ok(note) = rest.trim().parse::<u8>() {
            let intervals = synth_core::expand_chord_intervals(live_params.get_chord_mode());
            for &semi in intervals {
                let chord_note = (note as i32 + semi as i32).clamp(0, 127) as u8;
                let _ = gui_midi_tx.try_send(GuiMidiEvent::NoteOff(chord_note));
            }
            return;
        }
    }

    // 3. Ultra-fast compact path: "p,id,norm_val" o "p:id:norm_val" (Param change)
    let param_rest = trimmed
        .strip_prefix("p,")
        .or_else(|| trimmed.strip_prefix("p:"))
        .or_else(|| trimmed.strip_prefix("set_param:"));

    if let Some(rest) = param_rest {
        let delimiter = if rest.contains(',') { ',' } else { ':' };
        if let Some((id, val_str)) = rest.split_once(delimiter) {
            if let Ok(val) = val_str.trim().parse::<f32>() {
                set_param_from_client(params, live_params, gui_param_tx, id.trim(), val);
                return;
            }
        }
    }

    // 4. Ultra-fast compact path: "m,mode_int" o "m:mode_int" (Filter mode)
    let mode_rest = trimmed
        .strip_prefix("m,")
        .or_else(|| trimmed.strip_prefix("m:"));

    if let Some(rest) = mode_rest {
        if let Ok(val) = rest.trim().parse::<f32>() {
            set_param_from_client(params, live_params, gui_param_tx, "filter_mode", val);
            return;
        }
    }

    // 4a. Ultra-fast compact path: "lt,target_int" o "lt:target_int" (LFO Target)
    let lfo_target_rest = trimmed
        .strip_prefix("lt,")
        .or_else(|| trimmed.strip_prefix("lt:"));

    if let Some(rest) = lfo_target_rest {
        if let Ok(val) = rest.trim().parse::<f32>() {
            set_param_from_client(params, live_params, gui_param_tx, "lfo_target", val);
            return;
        }
    }

    // 4a-arp. Ultra-fast compact path: "arp,mode_int" o "arp:mode_int" (Arpegiador / Compás Venezolano)
    let arp_rest = trimmed
        .strip_prefix("arp,")
        .or_else(|| trimmed.strip_prefix("arp:"));

    if let Some(rest) = arp_rest {
        if let Ok(val) = rest.trim().parse::<f32>() {
            set_param_from_client(params, live_params, gui_param_tx, "arp_mode", val);
            return;
        }
    }

    // 4a-uv. Ultra-fast compact path: "uv,<1..8>" o "uni,<1..8>" (Unison Voices)
    let uv_rest = trimmed
        .strip_prefix("uv,")
        .or_else(|| trimmed.strip_prefix("uv:"))
        .or_else(|| trimmed.strip_prefix("uni,"))
        .or_else(|| trimmed.strip_prefix("uni:"));
    if let Some(rest) = uv_rest {
        if let Ok(v) = rest.trim().parse::<u8>() {
            live_params.set_unison_voices(v.clamp(1, 8));
            live_params.bump_gui_param_sync();
            live_params.bump_revision();
            return;
        }
    }

    // 4a-kt. Ultra-fast compact path: "kt,<0..1>" (SVF Filter Keytrack)
    let kt_rest = trimmed
        .strip_prefix("kt,")
        .or_else(|| trimmed.strip_prefix("kt:"));
    if let Some(rest) = kt_rest {
        if let Ok(v) = rest.trim().parse::<f32>() {
            live_params.set_filter_keytrack(v.clamp(0.0, 1.0));
            live_params.bump_gui_param_sync();
            live_params.bump_revision();
            return;
        }
    }

    // 4a-arpsteps. Ultra-fast compact path: "arpsteps,v0,v1,...,v11" (Full 12-Bar OLED Arp Velocity Pattern Sync)
    let arpsteps_rest = trimmed
        .strip_prefix("arpsteps,")
        .or_else(|| trimmed.strip_prefix("arpsteps:"));
    if let Some(rest) = arpsteps_rest {
        let mut parsed = [1.0_f32; 16];
        let mut count = 0usize;
        for part in rest.split(',') {
            if count >= 16 {
                break;
            }
            if let Ok(v) = part.trim().parse::<f32>() {
                parsed[count] = v.clamp(0.05, 1.0);
                count += 1;
            }
        }
        if count > 0 {
            live_params.set_arp_step_accents_slice(&parsed[..count]);
            live_params.bump_gui_param_sync();
            live_params.bump_revision();
            return;
        }
    }

    // 4a-arpstep. Ultra-fast compact path: "arpstep,<idx>,<accent>" (OLED Arp Step Velocity Sculpting)
    let arpstep_rest = trimmed
        .strip_prefix("arpstep,")
        .or_else(|| trimmed.strip_prefix("arpstep:"));
    if let Some(rest) = arpstep_rest {
        let mut parts = rest.split(',');
        if let (Some(s_idx), Some(s_acc)) = (parts.next(), parts.next()) {
            if let (Ok(idx), Ok(acc)) = (s_idx.trim().parse::<usize>(), s_acc.trim().parse::<f32>())
            {
                live_params.set_arp_step_accent(idx.min(15), acc.clamp(0.05, 1.0));
                live_params.bump_gui_param_sync();
                live_params.bump_revision();
                return;
            }
        }
    }

    // 4a-vel. Ultra-fast compact path: "vel,<0.05..1.0>" (Performance Velocity Buttons / Badge)
    let vel_rest = trimmed
        .strip_prefix("vel,")
        .or_else(|| trimmed.strip_prefix("vel:"));
    if let Some(rest) = vel_rest {
        if let Ok(v) = rest.trim().parse::<f32>() {
            let clamped = v.clamp(0.05, 1.0);
            live_params.record_velocity_event(clamped);
            live_params.bump_gui_param_sync();
            live_params.bump_revision();
            let _ = gui_midi_tx.try_send(GuiMidiEvent::SetVelocity(clamped));
            return;
        }
    }

    // 4a-wt3d. Ultra-fast compact path: "wt3d,<0|1>" (3D Wavetable Mode Toggle)
    let wt3d_rest = trimmed
        .strip_prefix("wt3d,")
        .or_else(|| trimmed.strip_prefix("wt3d:"));
    if let Some(rest) = wt3d_rest {
        let on = rest.trim() == "1" || rest.trim().eq_ignore_ascii_case("true");
        if on && live_params.get_wt_pos() < 0.01 && live_params.get_wt_warp() < 0.01 {
            live_params.set_wt_pos(0.35);
            live_params.set_wt_warp(0.25);
        } else if !on {
            live_params.set_wt_pos(0.0);
            live_params.set_wt_warp(0.0);
        }
        live_params.bump_gui_param_sync();
        live_params.bump_revision();
        return;
    }

    // 4a-fmb. Ultra-fast compact path: "fmb,<0..1>" (OSC B + FM A<-B Cross-Modulation)
    let fmb_rest = trimmed
        .strip_prefix("fmb,")
        .or_else(|| trimmed.strip_prefix("fmb:"));
    if let Some(rest) = fmb_rest {
        if let Ok(v) = rest.trim().parse::<f32>() {
            let clamped = v.clamp(0.0, 1.0);
            live_params.set_fm_from_b(clamped);
            live_params.set_osc_b_level(if clamped > 0.001 {
                (clamped * 0.75).clamp(0.15, 0.85)
            } else {
                0.0
            });
            live_params.bump_gui_param_sync();
            live_params.bump_revision();
            return;
        }
    }

    // 4a-ls. Ultra-fast compact path: "ls,<0..5>" (LFO Shape)
    let ls_rest = trimmed
        .strip_prefix("ls,")
        .or_else(|| trimmed.strip_prefix("ls:"));
    if let Some(rest) = ls_rest {
        if let Ok(s) = rest.trim().parse::<i32>() {
            live_params.set_lfo_shape(s.clamp(0, 5));
            live_params.bump_revision();
            return;
        }
    }

    // 4a-glide. Ultra-fast compact path: "glide,<0|1|2>" (Glide / Portamento & Legato mode)
    let glide_rest = trimmed
        .strip_prefix("glide,")
        .or_else(|| trimmed.strip_prefix("glide:"));
    if let Some(rest) = glide_rest {
        if let Ok(v) = rest.trim().parse::<u8>() {
            live_params.set_glide_mode(v.clamp(0, 2));
            live_params.bump_gui_param_sync();
            live_params.bump_revision();
            return;
        }
    }

    // 4a-pat. Ultra-fast compact path: "pat,<note>,<pressure>" (MPE Polyphonic Aftertouch)
    let pat_rest = trimmed
        .strip_prefix("pat,")
        .or_else(|| trimmed.strip_prefix("pat:"));
    if let Some(rest) = pat_rest {
        let mut parts = rest.split(',');
        if let (Some(n_str), Some(p_str)) = (parts.next(), parts.next()) {
            if let (Ok(note), Ok(pressure)) =
                (n_str.trim().parse::<u8>(), p_str.trim().parse::<f32>())
            {
                live_params.bump_midi_cc_events();
                live_params.bump_revision();
                let _ = gui_midi_tx
                    .try_send(GuiMidiEvent::PolyAftertouch(note, pressure.clamp(0.0, 1.0)));
                return;
            }
        }
    }

    // 4a-npb. Ultra-fast compact path: "npb,<note>,<semitones>" (MPE Per-Note Pitch Bend)
    let npb_rest = trimmed
        .strip_prefix("npb,")
        .or_else(|| trimmed.strip_prefix("npb:"));
    if let Some(rest) = npb_rest {
        let mut parts = rest.split(',');
        if let (Some(n_str), Some(b_str)) = (parts.next(), parts.next()) {
            if let (Ok(note), Ok(semitones)) =
                (n_str.trim().parse::<u8>(), b_str.trim().parse::<f32>())
            {
                live_params.bump_midi_cc_events();
                live_params.bump_revision();
                let _ = gui_midi_tx.try_send(GuiMidiEvent::NotePitchBend(
                    note,
                    semitones.clamp(-48.0, 48.0),
                ));
                return;
            }
        }
    }

    // 4a-cc. Ultra-fast compact path: "cc,<cc>,<0..1>" (MIDI CC & Live MIDI Learn)
    let cc_rest = trimmed
        .strip_prefix("cc,")
        .or_else(|| trimmed.strip_prefix("cc:"));
    if let Some(rest) = cc_rest {
        let mut parts = rest.split(',');
        if let (Some(c_str), Some(v_str)) = (parts.next(), parts.next()) {
            if let (Ok(cc), Ok(val)) = (c_str.trim().parse::<u8>(), v_str.trim().parse::<f32>()) {
                let clamped = val.clamp(0.0, 1.0);
                live_params.bump_midi_cc_events();
                let _ = live_params.apply_or_learn_midi_cc(cc & 0x7F, clamped);
                return;
            }
        }
    }

    // 4a-env2. Ultra-fast compact path: "env2,<target>,<depth>" (Filter / Mod Env 2)
    let env2_rest = trimmed
        .strip_prefix("env2,")
        .or_else(|| trimmed.strip_prefix("env2:"));
    if let Some(rest) = env2_rest {
        let mut parts = rest.split(',');
        if let (Some(t_str), Some(d_str)) = (parts.next(), parts.next()) {
            if let (Ok(target), Ok(depth)) =
                (t_str.trim().parse::<u8>(), d_str.trim().parse::<f32>())
            {
                live_params.set_env2_params(target.min(2), depth.clamp(-1.0, 1.0));
                live_params.bump_gui_param_sync();
                live_params.bump_revision();
                return;
            }
        } else if let Ok(target) = rest.trim().parse::<u8>() {
            let cur_depth = live_params.get_env2_depth();
            let next_depth = if cur_depth.abs() < 0.05 {
                0.75
            } else {
                cur_depth
            };
            live_params.set_env2_params(target.min(2), next_depth);
            live_params.bump_gui_param_sync();
            live_params.bump_revision();
            return;
        }
    }

    // 4a-envcrv. Ultra-fast compact path: "envcrv,<-1..1>" (ADSR Stage Curvature Tension)
    let envcrv_rest = trimmed
        .strip_prefix("envcrv,")
        .or_else(|| trimmed.strip_prefix("envcrv:"));
    if let Some(rest) = envcrv_rest {
        if let Ok(tension) = rest.trim().parse::<f32>() {
            live_params.set_env_curve(tension.clamp(-1.0, 1.0));
            live_params.bump_gui_param_sync();
            live_params.bump_revision();
            return;
        }
    }

    // 4a-arprate. Ultra-fast compact path: "arprate,<0..4>" (Arp Rate Subdivision 1/4, 1/8, 1/16, 1/8T, 1/16D)
    let arprate_rest = trimmed
        .strip_prefix("arprate,")
        .or_else(|| trimmed.strip_prefix("arprate:"));
    if let Some(rest) = arprate_rest {
        if let Ok(div) = rest.trim().parse::<u8>() {
            live_params.set_arp_rate_div(div.min(4));
            live_params.bump_gui_param_sync();
            live_params.bump_revision();
            return;
        }
    }

    // 4a-arpseq. Ultra-fast compact path: "arpseq,<12|16>" (16-Step MSEG / Ratchet Sequencer Mode)
    let arpseq_rest = trimmed
        .strip_prefix("arpseq,")
        .or_else(|| trimmed.strip_prefix("arpseq:"));
    if let Some(rest) = arpseq_rest {
        if let Ok(steps) = rest.trim().parse::<u8>() {
            live_params.set_arp_step_count(if steps >= 16 { 16 } else { 12 });
            live_params.bump_gui_param_sync();
            live_params.bump_revision();
            return;
        }
    }

    // 4a-gran. Ultra-fast compact path: "gran,<0|1|2>" (Sampler Granular Cloud / Freeze Mode)
    let gran_rest = trimmed
        .strip_prefix("gran,")
        .or_else(|| trimmed.strip_prefix("gran:"));
    if let Some(rest) = gran_rest {
        if let Ok(mode) = rest.trim().parse::<u8>() {
            live_params.set_granular_mode(mode.min(2));
            live_params.bump_gui_param_sync();
            live_params.bump_revision();
            return;
        }
    }

    // 4a-wtwarpmode. Ultra-fast compact path: "wtwarpmode,<0..3>" (Spectral Warp Algorithm: Fold / Asym / Sync / Quant)
    let wtwarpmode_rest = trimmed
        .strip_prefix("wtwarpmode,")
        .or_else(|| trimmed.strip_prefix("wtwarpmode:"));
    if let Some(rest) = wtwarpmode_rest {
        if let Ok(mode) = rest.trim().parse::<u8>() {
            let m = mode.min(3);
            live_params.set_wt_warp_mode(m);
            if m > 0 && live_params.get_wt_warp() < 0.05 {
                live_params.set_wt_warp(0.38);
            }
            live_params.bump_gui_param_sync();
            live_params.bump_revision();
            return;
        }
    }

    // 4a-fxmode. Ultra-fast compact path: "fxmode,<0..3>" (Master Bus FX Character: Canyon / 6-Stage Phaser / Analog Tape / Stereo Ping-Pong Delay)
    let fxmode_rest = trimmed
        .strip_prefix("fxmode,")
        .or_else(|| trimmed.strip_prefix("fxmode:"));
    if let Some(rest) = fxmode_rest {
        if let Ok(mode) = rest.trim().parse::<u8>() {
            let m = mode.min(3);
            live_params.set_fx_mode(m);
            if m > 0 && live_params.get_space_mix() < 0.05 {
                live_params.set_space_mix(0.32);
            }
            live_params.bump_gui_param_sync();
            live_params.bump_revision();
            return;
        }
    }

    // 4a-tuning. Ultra-fast compact path: "tuning,<0..3>" (Micro-Tuning / Temperaments: 12-TET / Just 5-Limit / Joropo Harp / Ethnic Maqam)
    let tuning_rest = trimmed
        .strip_prefix("tuning,")
        .or_else(|| trimmed.strip_prefix("tuning:"));
    if let Some(rest) = tuning_rest {
        if let Ok(mode) = rest.trim().parse::<u8>() {
            live_params.set_tuning_mode(mode.min(3));
            live_params.bump_gui_param_sync();
            live_params.bump_revision();
            return;
        }
    }

    // 4a-chordmode. Ultra-fast compact path: "chordmode,<0..3>" (Chord & Llanero Strum Generator: Off / Min7 / Maj9 / Strum)
    let chordmode_rest = trimmed
        .strip_prefix("chordmode,")
        .or_else(|| trimmed.strip_prefix("chordmode:"));
    if let Some(rest) = chordmode_rest {
        if let Ok(mode) = rest.trim().parse::<u8>() {
            live_params.set_chord_mode(mode.min(3));
            live_params.bump_gui_param_sync();
            live_params.bump_revision();
            return;
        }
    }

    // 4a-moddepth. Ultra-fast compact path: "moddepth,<cellIdx>,<-1.0..1.0>" (Bipolar 5x5 Mod Matrix Attenuverter)
    let moddepth_rest = trimmed
        .strip_prefix("moddepth,")
        .or_else(|| trimmed.strip_prefix("moddepth:"));
    if let Some(rest) = moddepth_rest {
        let mut parts = rest.split(',');
        if let (Some(c_str), Some(w_str)) = (parts.next(), parts.next()) {
            if let (Ok(cell_idx), Ok(weight)) =
                (c_str.trim().parse::<usize>(), w_str.trim().parse::<f32>())
            {
                if cell_idx < 25 {
                    let row = (cell_idx / 5) as u32;
                    let col = (cell_idx % 5) as u32;
                    live_params.set_mod_matrix_weight(cell_idx, weight.clamp(-1.0, 1.0));
                    if weight.abs() > 0.02 {
                        live_params.toggle_mod_matrix_pin(row, col, Some(true));
                    }
                    live_params.bump_gui_param_sync();
                    live_params.bump_revision();
                    return;
                }
            }
        }
    }

    // 4a-arpmidi. Ultra-fast compact path: "arpmidi:export" (Standard MIDI File .mid Clip Export from 16-Step Arpeggiator)
    if trimmed == "arpmidi:export" || trimmed.starts_with("arpmidi:") || trimmed.starts_with("arpmidi,") {
        let smf_bytes = live_params.export_arp_midi_bytes();
        let out_path = std::env::temp_dir().join("tepui-arp-pattern.mid");
        let _ = std::fs::write(&out_path, &smf_bytes);
        live_params.bump_patch_io_count();
        live_params.bump_revision();
        return;
    }

    // 4a-arpratchet. Ultra-fast compact path: "arpratchet,<idx>,<1..4>" (Arp Step Ratchet Subdivision)
    let arpratchet_rest = trimmed
        .strip_prefix("arpratchet,")
        .or_else(|| trimmed.strip_prefix("arpratchet:"));
    if let Some(rest) = arpratchet_rest {
        let mut parts = rest.split(',');
        if let (Some(s_idx), Some(s_rep)) = (parts.next(), parts.next()) {
            if let (Ok(idx), Ok(rep)) = (s_idx.trim().parse::<usize>(), s_rep.trim().parse::<u8>())
            {
                live_params.set_arp_step_ratchet(idx.min(15), rep.clamp(1, 4));
                live_params.bump_gui_param_sync();
                live_params.bump_revision();
                return;
            }
        }
    }

    // 4a-arpoct. Ultra-fast compact path: "arpoct,<idx>,<-2..2>" (Arp Step Octave Offset)
    let arpoct_rest = trimmed
        .strip_prefix("arpoct,")
        .or_else(|| trimmed.strip_prefix("arpoct:"));
    if let Some(rest) = arpoct_rest {
        let mut parts = rest.split(',');
        if let (Some(s_idx), Some(s_oct)) = (parts.next(), parts.next()) {
            if let (Ok(idx), Ok(oct)) = (s_idx.trim().parse::<usize>(), s_oct.trim().parse::<i8>())
            {
                live_params.set_arp_step_octave(idx.min(15), oct.clamp(-2, 2));
                live_params.bump_gui_param_sync();
                live_params.bump_revision();
                return;
            }
        }
    }

    // 4a-midilearn. Ultra-fast compact path: "midi_learn:<param_id>" (Right-Click / Button MIDI Learn Arm)
    let ml_rest = trimmed
        .strip_prefix("midi_learn:")
        .or_else(|| trimmed.strip_prefix("midi_learn,"));
    if let Some(rest) = ml_rest {
        live_params.arm_midi_learn_by_name(rest.trim());
        live_params.bump_gui_param_sync();
        live_params.bump_revision();
        return;
    }

    // 4a-dragwav. Ultra-fast compact path: "drag_wav_export" (OLED Drag-and-Drop / Quick Clip Export)
    if trimmed == "drag_wav_export" || trimmed.starts_with("drag_wav_export:") {
        live_params.bump_wav_drag_exports();
        live_params.bump_patch_io_count();
        live_params.bump_revision();
        return;
    }

    // 4a-rec. Ultra-fast compact path: "rec,start" | "rec,stop" (Grabadora Master en Vivo)
    let rec_rest = trimmed
        .strip_prefix("rec,")
        .or_else(|| trimmed.strip_prefix("rec:"));

    if let Some(rest) = rec_rest {
        let cmd = rest.trim();
        if cmd.eq_ignore_ascii_case("start") || cmd == "1" {
            let sr = f32::from_bits(sample_rate.load(Ordering::Relaxed)).max(8000.0);
            live_params.recorder.start_recording(sr);
            live_params.bump_revision();
            return;
        } else if cmd.eq_ignore_ascii_case("stop") || cmd == "0" {
            live_params.recorder.stop_recording();
            live_params.bump_revision();
            return;
        }
    }

    // 4a-z. Ultra-fast compact path: "z,0" | "z,1" | "z,2" | "z,clear" (Keyzone Assignment)
    let zone_rest = trimmed
        .strip_prefix("z,")
        .or_else(|| trimmed.strip_prefix("z:"));

    if let Some(rest) = zone_rest {
        let cmd = rest.trim();
        if cmd.eq_ignore_ascii_case("clear") || cmd == "-1" {
            ai_worker.clear_zones();
            live_params.set_active_keyzones_mask(0);
            live_params.bump_revision();
            return;
        }
        if let Ok(z_idx) = cmd.parse::<usize>() {
            if z_idx < 3 {
                ai_worker.assign_current_to_zone(z_idx);
                live_params.set_active_keyzones_mask(ai_worker.active_zones_mask());
                live_params.bump_revision();
                return;
            }
        }
    }

    // 4b. Ultra-fast compact path: "r,midi_note" o "r:midi_note" o "root_note:midi_note"
    let root_rest = trimmed
        .strip_prefix("r,")
        .or_else(|| trimmed.strip_prefix("r:"))
        .or_else(|| trimmed.strip_prefix("root_note:"));

    if let Some(rest) = root_rest {
        if let Ok(note) = rest.trim().parse::<i32>() {
            let clamped_note = note.clamp(0, 127);
            live_params.set_root_note(clamped_note);
            live_params.bump_revision();
            if let Some(tx) = gui_param_tx {
                let _ = tx.try_send(WebGuiEvent::SetParam {
                    id: GuiParamId::RootNote,
                    normalized: (clamped_note as f32 / 127.0).clamp(0.0, 1.0),
                });
            }
            return;
        }
    }

    // 4c. Ultra-fast compact path: "l,enabled,trimStart,trimEnd,loopStart,loopEnd"
    let loop_rest = trimmed
        .strip_prefix("l,")
        .or_else(|| trimmed.strip_prefix("l:"))
        .or_else(|| trimmed.strip_prefix("loop:"));

    if let Some(rest) = loop_rest {
        let delimiter = if rest.contains(',') { ',' } else { ':' };
        let parts: Vec<&str> = rest.split(delimiter).collect();
        if parts.len() >= 5 {
            if let (Ok(en), Ok(ts), Ok(te), Ok(ls), Ok(le)) = (
                parts[0].trim().parse::<i32>(),
                parts[1].trim().parse::<f32>(),
                parts[2].trim().parse::<f32>(),
                parts[3].trim().parse::<f32>(),
                parts[4].trim().parse::<f32>(),
            ) {
                live_params.set_loop_enabled_override(Some(en != 0));
                live_params.set_trim_start_norm(ts.clamp(0.0, 0.99));
                live_params.set_trim_end_norm(te.clamp(0.01, 1.0));
                live_params.set_loop_start_norm(ls.clamp(0.0, 0.98));
                live_params.set_loop_end_norm(le.clamp(0.02, 1.0));
                live_params.bump_revision();
                return;
            }
        }
    }

    // 4d. ODD UI Telemetry & Parametric Preset / Mod Matrix / Favorites Persistence IPC
    if let Some(json_str) = trimmed.strip_prefix("preset_apply:") {
        #[derive(Deserialize, Default)]
        struct PresetApplyPayload {
            prompt: Option<String>,
            attack: Option<f32>,
            decay: Option<f32>,
            sustain: Option<f32>,
            release: Option<f32>,
            cutoff: Option<f32>,
            resonance: Option<f32>,
            filter_mode: Option<i32>,
            gain: Option<f32>,
            space_mix: Option<f32>,
            lfo_rate: Option<f32>,
            lfo_depth: Option<f32>,
            lfo_target: Option<i32>,
            arp_mode: Option<i32>,
            root_note: Option<i32>,
            mod_matrix_mask: Option<u32>,
            wt_pos: Option<f32>,
            wt_warp: Option<f32>,
            unison_voices: Option<u8>,
            unison_detune: Option<f32>,
            sub_level: Option<f32>,
            drive_amount: Option<f32>,
            ott_amount: Option<f32>,
            lfo_shape: Option<i32>,
            macros: Option<[f32; 4]>,
        }
        if let Ok(p) = serde_json::from_str::<PresetApplyPayload>(json_str.trim()) {
            if let Some(v) = p.attack {
                live_params.set_attack(v.clamp(0.001, 5.0));
            }
            if let Some(v) = p.decay {
                live_params.set_decay(v.clamp(0.001, 5.0));
            }
            if let Some(v) = p.sustain {
                live_params.set_sustain(v.clamp(0.0, 1.0));
            }
            if let Some(v) = p.release {
                live_params.set_release(v.clamp(0.001, 10.0));
            }
            if let Some(v) = p.cutoff {
                live_params.set_cutoff(v.clamp(20.0, 20000.0));
            }
            if let Some(v) = p.resonance {
                live_params.set_resonance(v.clamp(0.1, 10.0));
            }
            if let Some(v) = p.filter_mode {
                live_params.set_filter_mode(v.clamp(0, 6));
            }
            if let Some(v) = p.gain {
                live_params.set_gain(v.clamp(0.0, 2.0));
            }
            if let Some(v) = p.space_mix {
                live_params.set_space_mix(v.clamp(0.0, 1.0));
            }
            if let Some(v) = p.lfo_rate {
                live_params.set_lfo_rate(v.clamp(0.1, 20.0));
            }
            if let Some(v) = p.lfo_depth {
                live_params.set_lfo_depth(v.clamp(0.0, 1.0));
            }
            if let Some(v) = p.lfo_target {
                live_params.set_lfo_target(v.clamp(0, 3));
            }
            if let Some(v) = p.lfo_shape {
                live_params.set_lfo_shape(v.clamp(0, 5));
            }
            if let Some(v) = p.arp_mode {
                live_params.set_arp_mode(v.clamp(0, 4));
            }
            if let Some(v) = p.root_note {
                live_params.set_root_note(v.clamp(0, 127));
            }
            if let Some(mask) = p.mod_matrix_mask {
                live_params.set_mod_matrix_mask(mask);
            }
            if let Some(v) = p.wt_pos {
                live_params.set_wt_pos(v.clamp(0.0, 1.0));
            }
            if let Some(v) = p.wt_warp {
                live_params.set_wt_warp(v.clamp(0.0, 1.0));
            }
            if let Some(v) = p.unison_voices {
                live_params.set_unison_voices(v.clamp(1, 8));
            }
            if let Some(v) = p.unison_detune {
                live_params.set_unison_detune(v.clamp(0.0, 1.0));
            }
            if let Some(v) = p.sub_level {
                live_params.set_sub_level(v.clamp(0.0, 1.0));
            }
            if let Some(v) = p.drive_amount {
                live_params.set_drive_amount(v.clamp(0.0, 1.0));
            }
            if let Some(v) = p.ott_amount {
                live_params.set_ott_amount(v.clamp(0.0, 1.0));
            }
            if let Some(m) = p.macros {
                for (i, val) in m.iter().enumerate() {
                    live_params.set_macro(i, val.clamp(0.0, 1.0));
                }
            }
            if let Some(prompt) = p.prompt {
                if !prompt.trim().is_empty() {
                    let sr = f32::from_bits(sample_rate.load(Ordering::Relaxed)).max(8000.0);
                    ai_worker.request_generation(prompt.trim().to_string(), sr, None);
                }
            }
        }
        live_params.bump_ui_preset_loads();
        live_params.bump_revision();
        return;
    }

    if trimmed.starts_with("preset:") || trimmed.starts_with("preset,") {
        live_params.bump_ui_preset_loads();
        live_params.bump_revision();
        return;
    }

    if let Some(mask_str) = trimmed
        .strip_prefix("matrix_mask:")
        .or_else(|| trimmed.strip_prefix("matrix_mask,"))
    {
        if let Ok(mask) = mask_str.trim().parse::<u32>() {
            live_params.set_mod_matrix_mask(mask);
        }
        live_params.bump_ui_matrix_switches();
        live_params.bump_revision();
        return;
    }

    if let Some(rest) = trimmed
        .strip_prefix("matrix:")
        .or_else(|| trimmed.strip_prefix("matrix,"))
    {
        let parts: Vec<&str> = rest.split(',').collect();
        if parts.len() >= 2 {
            if let (Ok(r), Ok(c)) = (
                parts[0].trim().parse::<u32>(),
                parts[1].trim().parse::<u32>(),
            ) {
                let explicit = parts
                    .get(2)
                    .and_then(|s| s.trim().parse::<u8>().ok())
                    .map(|v| v != 0);
                live_params.toggle_mod_matrix_pin(r, c, explicit);
            }
        }
        live_params.bump_ui_matrix_switches();
        live_params.bump_revision();
        return;
    }

    if let Some(fav_id) = trimmed
        .strip_prefix("fav_toggle:")
        .or_else(|| trimmed.strip_prefix("fav_toggle,"))
    {
        let _ = toggle_persisted_favorite(fav_id);
        live_params.bump_ui_fav_toggles();
        live_params.bump_revision();
        return;
    }

    if let Some(user_json) = trimmed.strip_prefix("user_preset_save:") {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(user_json.trim()) {
            append_persisted_user_preset(val);
        }
        live_params.bump_user_presets_saved();
        live_params.bump_revision();
        return;
    }

    if parse_resize_ipc(trimmed).is_some() {
        live_params.bump_ui_resize_events();
        live_params.bump_revision();
        return;
    }

    // 5. Prefix paths (AI Synthesizer Patch Architect + Sample Generation)
    if let Some(prompt) = trimmed
        .strip_prefix("gen:")
        .or_else(|| trimmed.strip_prefix("generate:"))
        .or_else(|| trimmed.strip_prefix("g,"))
    {
        let clean_prompt = prompt.trim();
        apply_ai_synthesized_patch_to_live_params(clean_prompt, live_params);
        let sr = f32::from_bits(sample_rate.load(Ordering::Relaxed));
        ai_worker.request_generation(clean_prompt.to_string(), sr, None);
        return;
    }

    // 6. JSON fallback
    #[derive(Deserialize)]
    struct IpcJsonMsg {
        #[serde(rename = "type")]
        msg_type: String,
        id: Option<String>,
        value: Option<f32>,
        note: Option<u8>,
        velocity: Option<f32>,
        prompt: Option<String>,
    }

    if trimmed.starts_with('{') {
        if let Ok(msg) = serde_json::from_str::<IpcJsonMsg>(trimmed) {
            match msg.msg_type.as_str() {
                "note_on" => {
                    if let Some(note) = msg.note {
                        let vel = msg.velocity.unwrap_or(0.9).clamp(0.01, 1.0);
                        live_params.record_velocity_event(vel);
                        let _ = gui_midi_tx.try_send(GuiMidiEvent::NoteOn(note, vel));
                    }
                }
                "note_off" => {
                    if let Some(note) = msg.note {
                        let _ = gui_midi_tx.try_send(GuiMidiEvent::NoteOff(note));
                    }
                }
                "set_param" => {
                    if let (Some(id), Some(val)) = (msg.id, msg.value) {
                        set_param_from_client(params, live_params, gui_param_tx, &id, val);
                    }
                }
                "generate" => {
                    if let Some(prompt) = msg.prompt {
                        let clean_prompt = prompt.trim().to_string();
                        apply_ai_synthesized_patch_to_live_params(&clean_prompt, live_params);
                        let sr = f32::from_bits(sample_rate.load(Ordering::Relaxed));
                        ai_worker.request_generation(clean_prompt, sr, None);
                    }
                }
                "panic" | "all_notes_off" => {
                    let _ = gui_midi_tx.try_send(GuiMidiEvent::AllNotesOff);
                }
                _ => {}
            }
        }
    }
}

pub fn apply_patch_spec_to_live_params(
    spec: &ai_bridge::AiSynthesizerPatchSpec,
    live_params: &LiveParamStore,
    count_as_ui_load: bool,
) {
    live_params.set_attack(spec.attack.clamp(0.001, 5.0));
    live_params.set_decay(spec.decay.clamp(0.001, 5.0));
    live_params.set_sustain(spec.sustain.clamp(0.0, 1.0));
    live_params.set_release(spec.release.clamp(0.001, 10.0));
    live_params.set_cutoff(spec.cutoff.clamp(20.0, 20000.0));
    live_params.set_resonance(spec.resonance.clamp(0.1, 10.0));
    live_params.set_filter_mode(i32::from(spec.filter_mode.clamp(0, 3)));
    live_params.set_lfo_rate(spec.lfo_rate.clamp(0.1, 20.0));
    live_params.set_lfo_depth(spec.lfo_depth.clamp(0.0, 1.0));
    live_params.set_lfo_target(i32::from(spec.lfo_target.clamp(0, 3)));
    live_params.set_arp_mode(i32::from(spec.arp_mode.clamp(0, 4)));
    live_params.set_space_mix(spec.space_mix.clamp(0.0, 1.0));
    live_params.set_mod_matrix_mask(spec.mod_matrix_mask);
    live_params.set_root_note((spec.root_note as i32).clamp(0, 127));
    if count_as_ui_load {
        live_params.bump_ui_preset_loads();
    }
    live_params.bump_revision();
}

pub fn apply_ai_synthesized_patch_to_live_params(prompt: &str, live_params: &LiveParamStore) {
    let (spec, _backend) = ai_bridge::design_patch_spec_from_prompt(prompt);
    apply_patch_spec_to_live_params(&spec, live_params, true);

    // Síntesis Procedural Local de Parámetros Vital/Serum a partir de la semántica del prompt:
    // Funciona con 0 créditos Cloud y 100% offline.
    let lower = prompt.to_lowercase();
    let is_bass = lower.contains("bass")
        || lower.contains("bajo")
        || lower.contains("808")
        || lower.contains("sub")
        || lower.contains("reese")
        || lower.contains("furruco");
    let is_pad = lower.contains("pad")
        || lower.contains("ambient")
        || lower.contains("etereo")
        || lower.contains("etéreo")
        || lower.contains("drone")
        || lower.contains("choir")
        || lower.contains("warm");
    let is_acid = lower.contains("acid")
        || lower.contains("303")
        || lower.contains("agresiv")
        || lower.contains("neuro")
        || lower.contains("distort")
        || lower.contains("cyberpunk");
    let is_fm = lower.contains("fm")
        || lower.contains("bell")
        || lower.contains("campana")
        || lower.contains("metal")
        || lower.contains("glass")
        || lower.contains("cristal");
    let is_wide = lower.contains("supersaw")
        || lower.contains("anthem")
        || lower.contains("wide")
        || lower.contains("brass")
        || is_pad;

    let wt_pos = if is_acid {
        0.72
    } else if is_fm {
        0.58
    } else if is_pad {
        0.38
    } else if is_bass {
        0.24
    } else {
        0.30
    };
    let wt_warp = if is_acid {
        0.54
    } else if is_fm {
        0.42
    } else if is_bass {
        0.28
    } else {
        0.16
    };
    let unison_voices = if is_wide {
        7
    } else if is_bass || is_acid {
        3
    } else {
        2
    };
    let unison_detune = if is_wide {
        0.46
    } else if is_bass {
        0.22
    } else {
        0.15
    };
    let sub_level = if is_bass {
        0.72
    } else if is_acid {
        0.45
    } else {
        0.15
    };
    let fm_from_b = if is_fm {
        0.48
    } else if is_acid {
        0.26
    } else {
        0.0
    };
    let drive_amount = if is_acid {
        0.58
    } else if is_bass {
        0.34
    } else {
        0.10
    };
    let ott_amount = if is_acid || is_wide {
        0.52
    } else if is_bass {
        0.38
    } else {
        0.18
    };

    live_params.set_wt_pos(wt_pos);
    live_params.set_wt_warp(wt_warp);
    live_params.set_unison_voices(unison_voices);
    live_params.set_unison_detune(unison_detune);
    live_params.set_sub_level(sub_level);
    live_params.set_fm_from_b(fm_from_b);
    live_params.set_osc_b_level(if fm_from_b > 0.001 {
        (fm_from_b * 0.75).clamp(0.15, 0.85)
    } else {
        0.0
    });
    live_params.set_drive_amount(drive_amount);
    live_params.set_ott_amount(ott_amount);
    live_params.bump_revision();
}

pub fn parse_resize_ipc(body: &str) -> Option<(u32, u32)> {
    let mut trimmed = body.trim();
    if ((trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
        && trimmed.len() >= 2
    {
        trimmed = &trimmed[1..trimmed.len() - 1];
    }
    let rest = trimmed
        .strip_prefix("resize:")
        .or_else(|| trimmed.strip_prefix("resize,"))?;
    let delimiter = if rest.contains(',') { ',' } else { ':' };
    let (w_str, h_str) = rest.split_once(delimiter)?;
    let w = w_str.trim().parse::<u32>().ok()?.clamp(640, 3840);
    let h = h_str.trim().parse::<u32>().ok()?.clamp(480, 2160);
    Some((w, h))
}

/// Sincroniza hacia el Host DAW (`GuiContext` / `ParamSetter`) cualquier parámetro expuesto que haya cambiado
/// desde la interfaz WebView2, permitiendo grabar automatización (Write / Latch / Touch) en el DAW.
pub fn sync_host_daw_changed_params(
    ctx: &dyn GuiContext,
    params: &SamplerParams,
    live_params: &LiveParamStore,
) {
    let setter = nih_plug::prelude::ParamSetter::new(ctx);

    let live_attack = live_params.get_attack();
    if (params.attack.value() - live_attack).abs() > 1e-5 {
        setter.begin_set_parameter(&params.attack);
        setter.set_parameter(&params.attack, live_attack);
        setter.end_set_parameter(&params.attack);
    }

    let live_decay = live_params.get_decay();
    if (params.decay.value() - live_decay).abs() > 1e-5 {
        setter.begin_set_parameter(&params.decay);
        setter.set_parameter(&params.decay, live_decay);
        setter.end_set_parameter(&params.decay);
    }

    let live_sustain = live_params.get_sustain();
    if (params.sustain.value() - live_sustain).abs() > 1e-5 {
        setter.begin_set_parameter(&params.sustain);
        setter.set_parameter(&params.sustain, live_sustain);
        setter.end_set_parameter(&params.sustain);
    }

    let live_release = live_params.get_release();
    if (params.release.value() - live_release).abs() > 1e-5 {
        setter.begin_set_parameter(&params.release);
        setter.set_parameter(&params.release, live_release);
        setter.end_set_parameter(&params.release);
    }

    let live_cutoff = live_params.get_cutoff();
    if (params.cutoff.value() - live_cutoff).abs() > 0.05 {
        setter.begin_set_parameter(&params.cutoff);
        setter.set_parameter(&params.cutoff, live_cutoff);
        setter.end_set_parameter(&params.cutoff);
    }

    let live_reso = live_params.get_resonance();
    if (params.resonance.value() - live_reso).abs() > 1e-4 {
        setter.begin_set_parameter(&params.resonance);
        setter.set_parameter(&params.resonance, live_reso);
        setter.end_set_parameter(&params.resonance);
    }

    let live_mode = live_params.filter_mode.load(Ordering::Relaxed);
    if params.filter_mode.value() != live_mode {
        setter.begin_set_parameter(&params.filter_mode);
        setter.set_parameter(&params.filter_mode, live_mode);
        setter.end_set_parameter(&params.filter_mode);
    }

    let live_gain = live_params.get_gain();
    if (params.gain.value() - live_gain).abs() > 1e-5 {
        setter.begin_set_parameter(&params.gain);
        setter.set_parameter(&params.gain, live_gain);
        setter.end_set_parameter(&params.gain);
    }

    let live_space = live_params.get_space_mix();
    if (params.space_mix.value() - live_space).abs() > 1e-4 {
        setter.begin_set_parameter(&params.space_mix);
        setter.set_parameter(&params.space_mix, live_space);
        setter.end_set_parameter(&params.space_mix);
    }

    let live_root = live_params.get_root_note();
    if params.root_note.value() != live_root {
        setter.begin_set_parameter(&params.root_note);
        setter.set_parameter(&params.root_note, live_root);
        setter.end_set_parameter(&params.root_note);
    }

    let live_lfo_rate = live_params.get_lfo_rate();
    if (params.lfo_rate.value() - live_lfo_rate).abs() > 1e-4 {
        setter.begin_set_parameter(&params.lfo_rate);
        setter.set_parameter(&params.lfo_rate, live_lfo_rate);
        setter.end_set_parameter(&params.lfo_rate);
    }

    let live_lfo_depth = live_params.get_lfo_depth();
    if (params.lfo_depth.value() - live_lfo_depth).abs() > 1e-4 {
        setter.begin_set_parameter(&params.lfo_depth);
        setter.set_parameter(&params.lfo_depth, live_lfo_depth);
        setter.end_set_parameter(&params.lfo_depth);
    }

    let live_lfo_target = live_params.lfo_target.load(Ordering::Relaxed);
    if params.lfo_target.value() != live_lfo_target {
        setter.begin_set_parameter(&params.lfo_target);
        setter.set_parameter(&params.lfo_target, live_lfo_target);
        setter.end_set_parameter(&params.lfo_target);
    }

    let live_lfo_shape = live_params.get_lfo_shape().as_i32();
    if params.lfo_shape.value() != live_lfo_shape {
        setter.begin_set_parameter(&params.lfo_shape);
        setter.set_parameter(&params.lfo_shape, live_lfo_shape);
        setter.end_set_parameter(&params.lfo_shape);
    }

    let live_arp_mode = live_params.arp_mode.load(Ordering::Relaxed);
    if params.arp_mode.value() != live_arp_mode {
        setter.begin_set_parameter(&params.arp_mode);
        setter.set_parameter(&params.arp_mode, live_arp_mode);
        setter.end_set_parameter(&params.arp_mode);
    }

    let live_wt_pos = live_params.get_wt_pos();
    if (params.wt_pos.value() - live_wt_pos).abs() > 1e-4 {
        setter.begin_set_parameter(&params.wt_pos);
        setter.set_parameter(&params.wt_pos, live_wt_pos);
        setter.end_set_parameter(&params.wt_pos);
    }

    let live_wt_warp = live_params.get_wt_warp();
    if (params.wt_warp.value() - live_wt_warp).abs() > 1e-4 {
        setter.begin_set_parameter(&params.wt_warp);
        setter.set_parameter(&params.wt_warp, live_wt_warp);
        setter.end_set_parameter(&params.wt_warp);
    }

    let live_uni = live_params.get_unison_voices() as i32;
    if params.unison_voices.value() != live_uni {
        setter.begin_set_parameter(&params.unison_voices);
        setter.set_parameter(&params.unison_voices, live_uni);
        setter.end_set_parameter(&params.unison_voices);
    }

    let live_detune = live_params.get_unison_detune();
    if (params.unison_detune.value() - live_detune).abs() > 1e-4 {
        setter.begin_set_parameter(&params.unison_detune);
        setter.set_parameter(&params.unison_detune, live_detune);
        setter.end_set_parameter(&params.unison_detune);
    }

    let live_sub = live_params.get_sub_level();
    if (params.sub_level.value() - live_sub).abs() > 1e-4 {
        setter.begin_set_parameter(&params.sub_level);
        setter.set_parameter(&params.sub_level, live_sub);
        setter.end_set_parameter(&params.sub_level);
    }

    let live_fm = live_params.get_fm_from_b();
    if (params.fm_from_b.value() - live_fm).abs() > 1e-4 {
        setter.begin_set_parameter(&params.fm_from_b);
        setter.set_parameter(&params.fm_from_b, live_fm);
        setter.end_set_parameter(&params.fm_from_b);
    }

    let live_drive = live_params.get_drive_amount();
    if (params.drive_amount.value() - live_drive).abs() > 1e-4 {
        setter.begin_set_parameter(&params.drive_amount);
        setter.set_parameter(&params.drive_amount, live_drive);
        setter.end_set_parameter(&params.drive_amount);
    }

    let live_ott = live_params.get_ott_amount();
    if (params.ott_amount.value() - live_ott).abs() > 1e-4 {
        setter.begin_set_parameter(&params.ott_amount);
        setter.set_parameter(&params.ott_amount, live_ott);
        setter.end_set_parameter(&params.ott_amount);
    }

    let live_kt = live_params.get_filter_keytrack();
    if (params.filter_keytrack.value() - live_kt).abs() > 1e-4 {
        setter.begin_set_parameter(&params.filter_keytrack);
        setter.set_parameter(&params.filter_keytrack, live_kt);
        setter.end_set_parameter(&params.filter_keytrack);
    }

    let live_macros = live_params.get_macros();
    if (params.macro1.value() - live_macros[0]).abs() > 1e-4 {
        setter.begin_set_parameter(&params.macro1);
        setter.set_parameter(&params.macro1, live_macros[0]);
        setter.end_set_parameter(&params.macro1);
    }
    if (params.macro2.value() - live_macros[1]).abs() > 1e-4 {
        setter.begin_set_parameter(&params.macro2);
        setter.set_parameter(&params.macro2, live_macros[1]);
        setter.end_set_parameter(&params.macro2);
    }
    if (params.macro3.value() - live_macros[2]).abs() > 1e-4 {
        setter.begin_set_parameter(&params.macro3);
        setter.set_parameter(&params.macro3, live_macros[2]);
        setter.end_set_parameter(&params.macro3);
    }
    if (params.macro4.value() - live_macros[3]).abs() > 1e-4 {
        setter.begin_set_parameter(&params.macro4);
        setter.set_parameter(&params.macro4, live_macros[3]);
        setter.end_set_parameter(&params.macro4);
    }
}

impl Editor for WebViewEditor {
    fn spawn(
        &self,
        parent: ParentWindowHandle,
        context: Arc<dyn GuiContext>,
    ) -> Box<dyn Any + Send> {
        let native_window = NativeParentWindow(parent);
        let (w, h) = self.size();

        #[cfg(windows)]
        let win_hwnd: win32_pump::HWND = match parent {
            ParentWindowHandle::Win32Hwnd(hwnd) => hwnd as win32_pump::HWND,
            _ => std::ptr::null_mut(),
        };
        #[cfg(windows)]
        let win_hwnd_usize = win_hwnd as usize;

        #[cfg(windows)]
        if !win_hwnd.is_null() {
            unsafe {
                win32_pump::apply_window_icon_png(win_hwnd, include_bytes!("../ui/tepui-icon.png"));
            }
        }

        let gui_midi_tx = self.gui_midi_tx.clone();
        let params = self.params.clone();
        let live_params = self.live_params.clone();
        let ai_worker = self.ai_worker.clone();
        let sample_rate = self.sample_rate.clone();
        let gui_param_tx = self.gui_param_tx.clone();
        let size_w_ipc = self.size_w.clone();
        let size_h_ipc = self.size_h.clone();
        let ctx_ipc = context.clone();
        #[cfg(windows)]
        let size_w_reg = self.size_w.clone();
        #[cfg(windows)]
        let size_h_reg = self.size_h.clone();

        let params_proto = self.params.clone();
        let live_params_proto = self.live_params.clone();
        let ai_worker_proto = self.ai_worker.clone();
        let gui_param_tx_proto = self.gui_param_tx.clone();

        let webview_data_dir = std::env::var("TEPUI_WEBVIEW_DATA_DIR")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                let base = std::env::var("LOCALAPPDATA")
                    .or_else(|_| std::env::var("APPDATA"))
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|_| std::env::temp_dir());
                base.join("Tepui").join(format!("WebView2_{}", std::env::current_exe().ok().and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned())).unwrap_or_else(|| "Studio".to_string())))
            });
        let _ = std::fs::create_dir_all(&webview_data_dir);
        let mut web_context = wry::WebContext::new(Some(webview_data_dir));
        let builder = WebViewBuilder::with_web_context(&mut web_context)
            .with_background_color((12, 14, 18, 255));

        let builder = builder
                    .with_custom_protocol("tepui".into(), move |_webview_id, req: Request<Vec<u8>>| {
                        let path = req.uri().path().trim_start_matches('/');
                        let method = req.method().as_str();
                        if !path.ends_with("api/telemetry")
                            && !path.ends_with("api/init-state")
                            && !path.ends_with("api/dev-mtime")
                            && !path.ends_with("favicon.ico")
                        {
                            println!("[TEPUI PROTO REQ] uri: {:?}, method: {:?}", req.uri(), req.method());
                        }

                        if method == "OPTIONS" {
                            return Response::builder()
                                .status(StatusCode::NO_CONTENT)
                                .header("Access-Control-Allow-Origin", "*")
                                .header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
                                .header("Access-Control-Allow-Headers", "Content-Type")
                                .body(Cow::Borrowed(&[] as &'static [u8]))
                                .unwrap();
                        }

                        if path.ends_with("favicon.ico") || path.ends_with("tepui-icon.png") {
                            return Response::builder()
                                .header(CONTENT_TYPE, "image/png")
                                .header("Access-Control-Allow-Origin", "*")
                                .body(Cow::Borrowed(include_bytes!("../ui/tepui-icon.png") as &'static [u8]))
                                .unwrap();
                        }

                        let clean_path = path.trim_start_matches('/').trim_end_matches('/');
                        let is_html_root = clean_path.is_empty()
                            || clean_path == "index.html"
                            || clean_path.ends_with("/index.html")
                            || clean_path == "localhost"
                            || clean_path.ends_with("/localhost")
                            || clean_path == "app"
                            || clean_path.ends_with("/app");

                        if is_html_root {
                            let bundled = build_html_bundle(&params_proto, &live_params_proto, &ai_worker_proto);
                            Response::builder()
                                .header(CONTENT_TYPE, "text/html; charset=utf-8")
                                .header("Cache-Control", "no-store, no-cache, must-revalidate")
                                .header("Access-Control-Allow-Origin", "*")
                                .body(Cow::Owned(bundled.into_bytes()))
                                .unwrap()
                        } else if path == "style.css" || path.ends_with("/style.css") {
                            let bytes = load_asset_bytes("style.css", include_bytes!("../ui/style.css"));
                            Response::builder()
                                .header(CONTENT_TYPE, "text/css; charset=utf-8")
                                .header("Cache-Control", "no-store, no-cache, must-revalidate")
                                .header("Access-Control-Allow-Origin", "*")
                                .body(bytes)
                                .unwrap()
                        } else if path == "app.js" || path.ends_with("/app.js") {
                            let bytes = load_asset_bytes("app.js", include_bytes!("../ui/app.js"));
                            Response::builder()
                                .header(CONTENT_TYPE, "application/javascript; charset=utf-8")
                                .header("Cache-Control", "no-store, no-cache, must-revalidate")
                                .header("Access-Control-Allow-Origin", "*")
                                .body(bytes)
                                .unwrap()
                        } else if path.ends_with("api/init-state") {
                            let json = build_init_state_json(&params_proto, &live_params_proto, &ai_worker_proto);
                            Response::builder()
                                .header(CONTENT_TYPE, "application/json; charset=utf-8")
                                .header("Access-Control-Allow-Origin", "*")
                                .body(Cow::Owned(json.into_bytes()))
                                .unwrap()
                        } else if path.ends_with("api/telemetry") {
                            let json = build_telemetry_json(&params_proto, &live_params_proto, &ai_worker_proto);
                            Response::builder()
                                .header(CONTENT_TYPE, "application/json; charset=utf-8")
                                .header("Access-Control-Allow-Origin", "*")
                                .body(Cow::Owned(json.into_bytes()))
                                .unwrap()
                        } else if path.ends_with("api/load-wav") {
                            let sample_name = extract_sample_name_from_query(req.uri().query())
                                .unwrap_or_else(|| "Imported WAV Sample".to_string());
                            let target_zone = extract_zone_index_from_query(req.uri().query());
                            match ai_worker_proto.load_wav_bytes(req.body(), Some(sample_name)) {
                                Ok(loaded) => {
                                    let root = loaded.root_note as i32;
                                    live_params_proto.set_root_note(root);
                                    if let Some(z_idx) = target_zone {
                                        ai_worker_proto.assign_current_to_zone(z_idx);
                                        live_params_proto.set_active_keyzones_mask(
                                            ai_worker_proto.active_zones_mask(),
                                        );
                                        live_params_proto.bump_wav_zone_drops();
                                    }
                                    live_params_proto.bump_revision();
                                    if let Some(tx) = &gui_param_tx_proto {
                                        let _ = tx.try_send(WebGuiEvent::SetParam {
                                            id: GuiParamId::RootNote,
                                            normalized: (root as f32 / 127.0).clamp(0.0, 1.0),
                                        });
                                    }
                                    let json = build_init_state_json(&params_proto, &live_params_proto, &ai_worker_proto);
                                    Response::builder()
                                        .header(CONTENT_TYPE, "application/json; charset=utf-8")
                                        .header("Access-Control-Allow-Origin", "*")
                                        .body(Cow::Owned(json.into_bytes()))
                                        .unwrap()
                                }
                                Err(err) => {
                                    let safe_err = err.replace('"', "'");
                                    let err_json = format!(r#"{{"success":false,"error":"{}"}}"#, safe_err);
                                    Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .header(CONTENT_TYPE, "application/json; charset=utf-8")
                                        .header("Access-Control-Allow-Origin", "*")
                                        .body(Cow::Owned(err_json.into_bytes()))
                                        .unwrap()
                                }
                            }
                        } else if path.ends_with("api/export-wav") || path.ends_with("api/export-clip") {
                            let sample = ai_worker_proto.get_sample();
                            let wav_bytes = live_params_proto.export_trimmed_wav_bytes(&sample);
                            Response::builder()
                                .header(CONTENT_TYPE, "audio/wav")
                                .header("Access-Control-Allow-Origin", "*")
                                .body(Cow::Owned(wav_bytes))
                                .unwrap()
                        } else if path.ends_with("api/export-live-rec") {
                            let wav_bytes = live_params_proto.recorder.export_wav_bytes();
                            Response::builder()
                                .header(CONTENT_TYPE, "audio/wav")
                                .header("Access-Control-Allow-Origin", "*")
                                .body(Cow::Owned(wav_bytes))
                                .unwrap()
                        } else if path.ends_with("api/export-patch") {
                            let patch = crate::params::TepuiPatch::export_from_live(&live_params_proto, &ai_worker_proto);
                            let json = serde_json::to_string_pretty(&patch).unwrap_or_else(|_| "{}".to_string());
                            Response::builder()
                                .header(CONTENT_TYPE, "application/json; charset=utf-8")
                                .header("Access-Control-Allow-Origin", "*")
                                .body(Cow::Owned(json.into_bytes()))
                                .unwrap()
                        } else if path.ends_with("api/load-patch") {
                            match serde_json::from_slice::<crate::params::TepuiPatch>(req.body()) {
                                Ok(patch) => match patch.apply_to_live(&live_params_proto, &ai_worker_proto) {
                                    Ok(()) => {
                                        if let Some(tx) = &gui_param_tx_proto {
                                            let _ = tx.try_send(WebGuiEvent::SetParam {
                                                id: GuiParamId::AllRefresh,
                                                normalized: 1.0,
                                            });
                                        }
                                        let json = build_init_state_json(&params_proto, &live_params_proto, &ai_worker_proto);
                                        Response::builder()
                                            .header(CONTENT_TYPE, "application/json; charset=utf-8")
                                            .header("Access-Control-Allow-Origin", "*")
                                            .body(Cow::Owned(json.into_bytes()))
                                            .unwrap()
                                    }
                                    Err(err) => {
                                        let safe_err = err.replace('"', "'");
                                        let err_json = format!(r#"{{"success":false,"error":"{}"}}"#, safe_err);
                                        Response::builder()
                                            .status(StatusCode::BAD_REQUEST)
                                            .header(CONTENT_TYPE, "application/json; charset=utf-8")
                                            .header("Access-Control-Allow-Origin", "*")
                                            .body(Cow::Owned(err_json.into_bytes()))
                                            .unwrap()
                                    }
                                },
                                Err(err) => {
                                    let safe_err = err.to_string().replace('"', "'");
                                    let err_json = format!(r#"{{"success":false,"error":"Patch .tepui inválido: {}"}}"#, safe_err);
                                    Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .header(CONTENT_TYPE, "application/json; charset=utf-8")
                                        .header("Access-Control-Allow-Origin", "*")
                                        .body(Cow::Owned(err_json.into_bytes()))
                                        .unwrap()
                                }
                            }
                        } else if path.ends_with("api/wav-to-wt") {
                            let sample = ai_worker_proto.get_sample();
                            let cycle = sample.extract_wavetable_cycle_64();
                            live_params_proto.set_custom_wavetable(&cycle);
                            live_params_proto.bump_gui_param_sync();
                            live_params_proto.bump_revision();
                            let json = serde_json::json!({
                                "success": true,
                                "cycle": cycle.to_vec(),
                                "sample_name": sample.name,
                            })
                            .to_string();
                            Response::builder()
                                .header(CONTENT_TYPE, "application/json; charset=utf-8")
                                .header("Access-Control-Allow-Origin", "*")
                                .body(Cow::Owned(json.into_bytes()))
                                .unwrap()
                        } else if path.ends_with("api/audio-devices") {
                            let config = crate::audio_devices::load_config();
                            let status = crate::audio_devices::enumerate_audio_system(&config);
                            let json = serde_json::to_string(&status).unwrap_or_else(|_| "{}".to_string());
                            Response::builder()
                                .header(CONTENT_TYPE, "application/json; charset=utf-8")
                                .header("Access-Control-Allow-Origin", "*")
                                .body(Cow::Owned(json.into_bytes()))
                                .unwrap()
                        } else if path.ends_with("api/save-audio-config") {
                            let body = req.body();
                            let resp_json = match serde_json::from_slice::<crate::audio_devices::StandaloneAudioConfig>(body) {
                                Ok(cfg) => match crate::audio_devices::save_config(&cfg) {
                                    Ok(_) => {
                                        let active_midi = crate::audio_devices::connect_hardware_midi_port(cfg.midi_input.as_deref());
                                        println!("[TEPUI CONFIG] Guardado via Custom Protocol: {:?} (MIDI activo: {:?})", cfg, active_midi);
                                        serde_json::json!({
                                            "success": true,
                                            "message": "Configuración guardada y puerto MIDI conectado en caliente",
                                            "active_midi_input": active_midi,
                                        })
                                        .to_string()
                                    }
                                    Err(e) => format!(r#"{{"success":false,"error":"{}"}}"#, e),
                                    },
                                    Err(e) => format!(r#"{{"success":false,"error":"JSON inválido: {}"}}"#, e),
                                };
                            Response::builder()
                                .header(CONTENT_TYPE, "application/json; charset=utf-8")
                                .header("Access-Control-Allow-Origin", "*")
                                .body(Cow::Owned(resp_json.into_bytes()))
                                .unwrap()
                        } else if path.ends_with("api/restart-standalone") {
                            let resp_json = match crate::audio_devices::restart_standalone_app() {
                                Ok(_) => r#"{"success":true,"message":"Reiniciando aplicación..."}"#.to_string(),
                                Err(e) => format!(r#"{{"success":false,"error":"{}"}}"#, e),
                            };
                            Response::builder()
                                .header(CONTENT_TYPE, "application/json; charset=utf-8")
                                .header("Access-Control-Allow-Origin", "*")
                                .body(Cow::Owned(resp_json.into_bytes()))
                                .unwrap()
                        } else if path.ends_with("api/cloud-license") {
                            let cfg = ai_bridge::load_cloud_license_config();
                            let quota = ai_bridge::CloudEdgeQuotaReport {
                                tier: ai_worker_proto.cloud_tier(),
                                credits_remaining: ai_worker_proto.cloud_credits_remaining(),
                                daily_neurons_used: ai_worker_proto.cloud_daily_neurons(),
                                max_daily_neurons: 9500,
                                cached: false,
                                budget_guard_tripped: ai_worker_proto.cloud_budget_guard_trips() > 0,
                                message: "Escudo Anti-Costos KV activo (Tope gratuito: 9,500 Neurons/día)".to_string(),
                            };
                            let json = serde_json::json!({
                                "config": cfg,
                                "quota": quota,
                            })
                            .to_string();
                            Response::builder()
                                .header(CONTENT_TYPE, "application/json; charset=utf-8")
                                .header("Access-Control-Allow-Origin", "*")
                                .body(Cow::Owned(json.into_bytes()))
                                .unwrap()
                        } else if path.ends_with("api/save-cloud-license") {
                            let body = req.body();
                            let resp_json = match serde_json::from_slice::<ai_bridge::CloudLicenseConfig>(body) {
                                Ok(mut cfg) => {
                                    let existing = ai_bridge::load_cloud_license_config();
                                    if cfg.device_id.trim().is_empty() {
                                        cfg.device_id = existing.device_id;
                                    }
                                    if cfg.redeemed_credits == 0 && existing.redeemed_credits > 0 {
                                        cfg.redeemed_credits = existing.redeemed_credits;
                                    }
                                    if cfg.redeemed_codes.is_empty() && !existing.redeemed_codes.is_empty() {
                                        cfg.redeemed_codes = existing.redeemed_codes;
                                    }
                                    if !cfg.pro_tier_unlocked && existing.pro_tier_unlocked {
                                        cfg.pro_tier_unlocked = true;
                                    }
                                    if cfg.credits_spent == 0 && existing.credits_spent > 0 {
                                        cfg.credits_spent = existing.credits_spent;
                                    }
                                    match ai_bridge::save_cloud_license_config(&cfg) {
                                        Ok(_) => {
                                            let quota = ai_bridge::verify_cloud_license_quota(&cfg);
                                            ai_worker_proto.apply_cloud_quota_report(&quota);
                                            live_params_proto.bump_revision();
                                            serde_json::json!({
                                                "success": true,
                                                "config": cfg,
                                                "quota": quota,
                                            })
                                            .to_string()
                                        }
                                        Err(e) => serde_json::json!({
                                            "success": false,
                                            "error": e,
                                        })
                                        .to_string(),
                                    }
                                }
                                Err(e) => serde_json::json!({
                                    "success": false,
                                    "error": format!("JSON inválido: {}", e),
                                })
                                .to_string(),
                            };
                            Response::builder()
                                .header(CONTENT_TYPE, "application/json; charset=utf-8")
                                .header("Access-Control-Allow-Origin", "*")
                                .body(Cow::Owned(resp_json.into_bytes()))
                                .unwrap()
                        } else if path.ends_with("api/redeem-cloud-code") || path.ends_with("api/credits/redeem") {
                            let body = req.body();
                            let code_str = serde_json::from_slice::<serde_json::Value>(body)
                                .ok()
                                .and_then(|v| v.get("code").and_then(|c| c.as_str()).map(|s| s.to_string()))
                                .unwrap_or_default();
                            let mut cfg = ai_bridge::load_cloud_license_config();
                            let (status_code, resp_json) = match ai_bridge::redeem_cloud_license_code(&code_str, &mut cfg) {
                                Ok(quota) => {
                                    let _ = ai_bridge::save_cloud_license_config(&cfg);
                                    ai_worker_proto.apply_cloud_quota_report(&quota);
                                    live_params_proto.bump_revision();
                                    (
                                        StatusCode::OK,
                                        serde_json::json!({
                                            "ok": true,
                                            "success": true,
                                            "tier": quota.tier,
                                            "credits_remaining": quota.credits_remaining,
                                            "message": quota.message,
                                            "config": cfg,
                                            "quota": quota,
                                        })
                                        .to_string(),
                                    )
                                }
                                Err(e) => (
                                    StatusCode::BAD_REQUEST,
                                    serde_json::json!({
                                        "ok": false,
                                        "success": false,
                                        "error": e,
                                    })
                                    .to_string(),
                                ),
                            };
                            Response::builder()
                                .status(status_code)
                                .header(CONTENT_TYPE, "application/json; charset=utf-8")
                                .header("Access-Control-Allow-Origin", "*")
                                .body(Cow::Owned(resp_json.into_bytes()))
                                .unwrap()
                        } else if path.ends_with("api/webhooks/checkout") {
                            let sig = req
                                .headers()
                                .get("x-signature-256")
                                .or_else(|| req.headers().get("x-hook-signature"))
                                .and_then(|v| v.to_str().ok())
                                .unwrap_or("");
                            let secret = std::env::var("TEPUI_WEBHOOK_SECRET")
                                .unwrap_or_else(|_| "whsec_tepui_default_edge_secret".to_string());
                            let mut cfg = ai_bridge::load_cloud_license_config();
                            let (status_code, resp_json) = match ai_bridge::process_checkout_webhook_payload(
                                req.body(),
                                sig,
                                &secret,
                                &mut cfg,
                            ) {
                                Ok(quota) => {
                                    let _ = ai_bridge::save_cloud_license_config(&cfg);
                                    ai_worker_proto.apply_cloud_quota_report(&quota);
                                    live_params_proto.bump_revision();
                                    (
                                        StatusCode::OK,
                                        serde_json::json!({
                                            "ok": true,
                                            "quota": quota,
                                        })
                                        .to_string(),
                                    )
                                }
                                Err(e) => {
                                    let code = if e.contains("HTTP 401") {
                                        StatusCode::UNAUTHORIZED
                                    } else {
                                        StatusCode::BAD_REQUEST
                                    };
                                    (
                                        code,
                                        serde_json::json!({
                                            "ok": false,
                                            "error": e,
                                        })
                                        .to_string(),
                                    )
                                }
                            };
                            Response::builder()
                                .status(status_code)
                                .header(CONTENT_TYPE, "application/json; charset=utf-8")
                                .header("Access-Control-Allow-Origin", "*")
                                .body(Cow::Owned(resp_json.into_bytes()))
                                .unwrap()
                        } else {
                            Response::builder()
                                .status(StatusCode::NOT_FOUND)
                                .header(CONTENT_TYPE, "text/plain")
                                .body(Cow::Borrowed(b"Not Found" as &'static [u8]))
                                .unwrap()
                        }
                    })
                    .with_url("tepui://localhost/index.html")
                    .with_devtools(true)
                    .with_initialization_script(r#"
                        window.__TEPUI_NATIVE_IPC__ = true;
                        if (typeof window.ipc === 'undefined' && window.chrome && window.chrome.webview) {
                            window.ipc = { postMessage: function(msg) { window.chrome.webview.postMessage(msg); } };
                        }
                    "#)
                    .with_ipc_handler(move |req| {
                        if let Some((rw, rh)) = parse_resize_ipc(req.body()) {
                            size_w_ipc.store(rw, Ordering::Relaxed);
                            size_h_ipc.store(rh, Ordering::Relaxed);
                            let _ = ctx_ipc.request_resize();
                            live_params.bump_ui_resize_events();
                            live_params.bump_revision();
                            #[cfg(windows)]
                            unsafe {
                                win32_pump::request_window_resize(
                                    win_hwnd_usize as win32_pump::HWND,
                                    rw,
                                    rh,
                                );
                            }
                            return;
                        }
                        let rev_before = live_params.get_revision();
                        handle_native_ipc_message(
                            req.body(),
                            &gui_midi_tx,
                            &params,
                            &live_params,
                            &ai_worker,
                            &sample_rate,
                            &gui_param_tx,
                        );
                        if live_params.get_revision() != rev_before {
                            sync_host_daw_changed_params(ctx_ipc.as_ref(), &params, &live_params);
                        }
                    })
                    .with_bounds(Rect {
                        position: LogicalPosition::new(0.0, 0.0).into(),
                        size: LogicalSize::new(w as f64, h as f64).into(),
                    })
                    .with_on_page_load_handler(|event, url| {
                        let ev_str = match event {
                            PageLoadEvent::Started => "Started",
                            PageLoadEvent::Finished => "Finished",
                        };
                        eprintln!("[TEPUI WEBVIEW LOAD] {} url: {}", ev_str, url);
                    });

        match builder.build_as_child(&native_window) {
            Ok(webview) => {
                let webview = Rc::new(webview);
                #[cfg(windows)]
                if !win_hwnd.is_null() {
                    win32_pump::register_active_webview(win32_pump::ActiveResizeState {
                        hwnd: win_hwnd,
                        track_hwnd: unsafe { win32_pump::setup_resizable_window(win_hwnd, w, h) },
                        webview: Rc::clone(&webview),
                        size_w: size_w_reg,
                        size_h: size_h_reg,
                        last_w: w as i32,
                        last_h: h as i32,
                    });
                    unsafe {
                        win32_pump::SetTimer(
                            win_hwnd,
                            win32_pump::TIMER_ID,
                            12,
                            Some(win32_pump::timer_pump_proc),
                        );
                    }
                }
                let cur_url = webview.url().unwrap_or_default();
                println!(
                    "[TEPUI WEBVIEW] WebView Serverless 0-lag incrustado exitosamente! URL: {}",
                    cur_url
                );

                let bounds = Rect {
                    position: LogicalPosition::new(0.0, 0.0).into(),
                    size: LogicalSize::new(w as f64, h as f64).into(),
                };
                if let Err(e) = webview.set_bounds(bounds) {
                    eprintln!("[TEPUI WEBVIEW] Error al configurar bounds: {:?}", e);
                }
                let _ = webview.focus();

                #[cfg(windows)]
                if !win_hwnd.is_null() {
                    unsafe {
                        let mut msg: win32_pump::MSG = std::mem::zeroed();
                        let mut initial_drain = 0;
                        while win32_pump::PeekMessageW(
                            &mut msg,
                            std::ptr::null_mut(),
                            0,
                            0,
                            win32_pump::PM_REMOVE,
                        ) != 0
                        {
                            win32_pump::TranslateMessage(&msg);
                            win32_pump::DispatchMessageW(&msg);
                            initial_drain += 1;
                            if initial_drain >= 32 {
                                break;
                            }
                        }
                    }
                }

                Box::new(WebEditorHandle {
                    _webview: webview,
                    _web_context: web_context,
                    #[cfg(windows)]
                    parent_hwnd: win_hwnd,
                })
            }
            Err(e) => {
                eprintln!("[TEPUI WEBVIEW] Error creando WebView hijo en DAW: {:?}", e);
                Box::new(())
            }
        }
    }

    fn size(&self) -> (u32, u32) {
        (
            self.size_w.load(Ordering::Relaxed),
            self.size_h.load(Ordering::Relaxed),
        )
    }

    fn set_scale_factor(&self, factor: f32) -> bool {
        self.scale_factor.store(factor.to_bits(), Ordering::Relaxed);
        true
    }

    fn param_value_changed(&self, _id: &str, _normalized_value: f32) {
        self.live_params.set_attack(self.params.attack.value());
        self.live_params.set_decay(self.params.decay.value());
        self.live_params.set_sustain(self.params.sustain.value());
        self.live_params.set_release(self.params.release.value());
        self.live_params.set_cutoff(self.params.cutoff.value());
        self.live_params
            .set_resonance(self.params.resonance.value());
        self.live_params
            .set_filter_mode(self.params.filter_mode.value());
        self.live_params.set_gain(self.params.gain.value());
        self.live_params
            .set_space_mix(self.params.space_mix.value());
        self.live_params
            .set_root_note(self.params.root_note.value());
        self.live_params.set_lfo_rate(self.params.lfo_rate.value());
        self.live_params
            .set_lfo_depth(self.params.lfo_depth.value());
        self.live_params
            .set_lfo_target(self.params.lfo_target.value());
        self.live_params.bump_revision();
    }

    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {}

    fn param_values_changed(&self) {
        self.param_value_changed("", 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::bounded;

    #[test]
    fn test_build_html_bundle() {
        let params = Arc::new(SamplerParams::default());
        let live_params = Arc::new(LiveParamStore::default());
        let ai_worker = Arc::new(AiWorker::new(44100.0));
        let bundle = build_html_bundle(&params, &live_params, &ai_worker);
        assert!(!bundle.is_empty());
        assert!(bundle.contains("<style>"));
        assert!(bundle.contains("window.__TEPUI_INIT_STATE__"));
        assert!(bundle.contains("window.__TEPUI_AUDIO_STATUS__"));
        assert!(bundle.contains("TEPUI"));
        assert!(bundle.contains("EURORACK MATRIX"));
        assert!(bundle.contains("PRESET BROWSER"));
        assert!(bundle.contains("SOUND PACK CARDS"));
        assert!(bundle.contains("oled-svf"));
        assert!(bundle.contains("oled-adsr"));
        assert!(bundle.contains("oled-arp"));
        assert!(bundle.contains("btn-wtdraw-toggle"));
        assert!(bundle.contains("btn-wav-to-wt"));
        if let Ok(out_path) = std::env::var("TEPUI_DUMP_PREVIEW") {
            if let Some(parent) = std::path::Path::new(&out_path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&out_path, &bundle);
        }
    }

    #[test]
    fn test_native_ipc_compact_note_on_off() {
        let (gui_midi_tx, gui_midi_rx) = bounded(32);
        let params = Arc::new(SamplerParams::default());
        let live_params = Arc::new(LiveParamStore::default());
        let ai_worker = Arc::new(AiWorker::new(44100.0));
        let sample_rate = Arc::new(AtomicU32::new(44100.0_f32.to_bits()));

        // 1. Compact Note On: "1,60,0.85"
        handle_native_ipc_message(
            "1,60,0.85",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );

        let event = gui_midi_rx.try_recv().expect("NoteOn event expected");
        match event {
            GuiMidiEvent::NoteOn(note, vel) => {
                assert_eq!(note, 60);
                assert!((vel - 0.85).abs() < 1e-4);
            }
            _ => panic!("Expected NoteOn"),
        }

        // 2. Compact Note Off: "0,60"
        handle_native_ipc_message(
            "0,60",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );

        let event = gui_midi_rx.try_recv().expect("NoteOff event expected");
        match event {
            GuiMidiEvent::NoteOff(note) => {
                assert_eq!(note, 60);
            }
            _ => panic!("Expected NoteOff"),
        }

        // 3. Compact Param change: "p,cutoff,0.5"
        handle_native_ipc_message(
            "p,cutoff,0.5",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        let cutoff = live_params.get_cutoff();
        assert!(cutoff > 20.0 && cutoff < 20000.0);

        // 4. JSON fallback: NoteOn
        handle_native_ipc_message(
            r#"{"type":"note_on","note":72,"velocity":0.95}"#,
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );

        let event = gui_midi_rx.try_recv().expect("JSON NoteOn expected");
        match event {
            GuiMidiEvent::NoteOn(note, vel) => {
                assert_eq!(note, 72);
                assert!((vel - 0.95).abs() < 1e-4);
            }
            _ => panic!("Expected NoteOn"),
        }

        // 5. Compact Panic: "k"
        handle_native_ipc_message(
            "k",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        let event = gui_midi_rx.try_recv().expect("AllNotesOff event expected");
        assert_eq!(event, GuiMidiEvent::AllNotesOff);

        // 6. Compact Panic word: "panic"
        handle_native_ipc_message(
            "panic",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        let event = gui_midi_rx.try_recv().expect("AllNotesOff event expected");
        assert_eq!(event, GuiMidiEvent::AllNotesOff);

        // 7. Filter mode: "m,2"
        handle_native_ipc_message(
            "m,2",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        assert_eq!(
            live_params.get_filter_mode(),
            synth_core::FilterMode::Bandpass
        );

        // 8. Save Audio Config IPC (direct & escaped JSON)
        handle_native_ipc_message(
            r#"save_audio_config:{"output_device":"Test Speaker","input_device":null,"sample_rate":48000,"period_size":1056,"midi_input":null}"#,
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        let loaded = crate::audio_devices::load_config();
        assert_eq!(loaded.output_device.as_deref(), Some("Test Speaker"));
        assert_eq!(loaded.sample_rate, 48000);
        assert_eq!(loaded.period_size, 1056);

        // Restaurar configuración limpia por defecto para no contaminar el entorno del usuario
        let clean_cfg = crate::audio_devices::StandaloneAudioConfig::default();
        let _ = crate::audio_devices::save_config(&clean_cfg);
    }

    #[test]
    fn test_root_note_ipc_and_telemetry_json() {
        let (gui_midi_tx, _gui_midi_rx) = bounded(16);
        let (gui_param_tx, gui_param_rx) = bounded(16);
        let params = Arc::new(SamplerParams::default());
        let live_params = Arc::new(LiveParamStore::default());
        let ai_worker = Arc::new(AiWorker::new(44100.0));
        let sample_rate = Arc::new(AtomicU32::new(44100.0_f32.to_bits()));

        let initial_rev = live_params.get_revision();
        assert_eq!(live_params.get_root_note(), 60);

        // 1. Enviar IPC compacto de root_note: "r,48" (C3)
        handle_native_ipc_message(
            "r,48",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &Some(gui_param_tx.clone()),
        );
        assert_eq!(live_params.get_root_note(), 48);
        assert!(live_params.get_revision() > initial_rev);

        let evt = gui_param_rx
            .try_recv()
            .expect("Expected SetParam for root_note");
        match evt {
            WebGuiEvent::SetParam { id, normalized } => {
                assert_eq!(id, GuiParamId::RootNote);
                assert!((normalized - (48.0 / 127.0)).abs() < 1e-4);
            }
        }

        // 2. Verificar telemetría JSON en tiempo real
        live_params.set_peaks(0.65, 0.72);
        live_params.set_active_voices(4);

        let telem_json = build_telemetry_json(&params, &live_params, &ai_worker);
        assert!(telem_json.contains(r#""root_note":48"#));
        assert!(telem_json.contains(r#""active_voices":4"#));
        assert!(telem_json.contains("Concert Grand Piano"));

        // 3. Verificar importación/exportación WAV reflejada en init_state_json
        let current_sample = ai_worker.get_sample();
        let wav_bytes = synth_core::encode_wav_f32(&current_sample);
        let loaded = ai_worker
            .load_wav_bytes(&wav_bytes, Some("Custom Studio Synth".to_string()))
            .expect("WAV load should succeed");
        assert_eq!(loaded.name, "Custom Studio Synth");

        let init_json = build_init_state_json(&params, &live_params, &ai_worker);
        assert!(init_json.contains("Custom Studio Synth"));
        assert!(init_json.contains(r#""active_voices":4"#));
    }

    #[test]
    fn test_region_loop_ipc_and_odd_metrics_telemetry() {
        let (gui_midi_tx, _gui_midi_rx) = bounded(16);
        let params = Arc::new(SamplerParams::default());
        let live_params = Arc::new(LiveParamStore::default());
        let ai_worker = Arc::new(AiWorker::new(44100.0));
        let sample_rate = Arc::new(AtomicU32::new(44100.0_f32.to_bits()));

        // 1. Enviar IPC compacto de región/loop: "l,1,0.08,0.92,0.22,0.78"
        handle_native_ipc_message(
            "l,1,0.08,0.92,0.22,0.78",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        assert_eq!(live_params.get_loop_enabled_override(), Some(true));
        assert!((live_params.get_trim_start_norm() - 0.08).abs() < 1e-4);
        assert!((live_params.get_trim_end_norm() - 0.92).abs() < 1e-4);
        assert!((live_params.get_loop_start_norm() - 0.22).abs() < 1e-4);
        assert!((live_params.get_loop_end_norm() - 0.78).abs() < 1e-4);

        // 2. Enviar cambio de Space FX Mix via IPC compacto: "p,space_mix,0.65"
        handle_native_ipc_message(
            "p,space_mix,0.65",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        assert!((live_params.get_space_mix() - 0.65).abs() < 1e-4);

        // 3. Simular contadores ODD y verificar serialización JSON
        live_params.bump_dsp_blocks();
        live_params.bump_dsp_blocks();
        live_params.add_limiter_activations(3);
        live_params.bump_persistence_restores();

        let telem_json = build_telemetry_json(&params, &live_params, &ai_worker);
        assert!(telem_json.contains(r#""dsp_blocks_processed":2"#));
        assert!(telem_json.contains(r#""dsp_limiter_activations":3"#));
        assert!(telem_json.contains(r#""persistence_restores":1"#));
        assert!(telem_json.contains(r#""loop_enabled":true"#));
        assert!(telem_json.contains(r#""space_mix":0.65"#));
    }

    #[test]
    fn test_tepui_patch_roundtrip_and_lfo_keyzone_ipc() {
        let (gui_midi_tx, _gui_midi_rx) = bounded(16);
        let params = Arc::new(SamplerParams::default());
        let live_params = Arc::new(LiveParamStore::default());
        let ai_worker = Arc::new(AiWorker::new(44100.0));
        let sample_rate = Arc::new(AtomicU32::new(44100.0_f32.to_bits()));

        // 1. Configurar LFO y Keyzones vía IPC compacto
        handle_native_ipc_message(
            "p,lfo_rate,0.5",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        handle_native_ipc_message(
            "p,lfo_depth,0.75",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        handle_native_ipc_message(
            "lt,1",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        handle_native_ipc_message(
            "z,0",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        handle_native_ipc_message(
            "z,2",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );

        assert!((live_params.get_lfo_depth() - 0.75).abs() < 1e-4);
        assert_eq!(live_params.get_lfo_target(), synth_core::LfoTarget::Pitch);
        assert_eq!(live_params.get_active_keyzones_mask(), 0b101); // Low (bit 0) + High (bit 2)

        // 2. Exportar Patch .tepui completo
        let exported = crate::params::TepuiPatch::export_from_live(&live_params, &ai_worker);
        assert_eq!(exported.magic, "TEPUI_PATCH_V1");
        assert_eq!(exported.vendor, "Gran Sabana Sonic Lab");
        assert!(!exported.wav_b64.is_empty());
        assert_eq!(live_params.get_patch_io_count(), 1);

        // 3. Restaurar Patch en un almacén fresco y verificar todos los parámetros + contador ODD
        let fresh_live = Arc::new(LiveParamStore::default());
        let fresh_worker = Arc::new(AiWorker::new(44100.0));
        exported
            .apply_to_live(&fresh_live, &fresh_worker)
            .expect("apply_to_live should succeed");

        assert_eq!(fresh_live.get_patch_io_count(), 1);
        assert!((fresh_live.get_lfo_depth() - 0.75).abs() < 1e-4);
        assert_eq!(fresh_live.get_lfo_target(), synth_core::LfoTarget::Pitch);
        assert_eq!(fresh_live.get_active_keyzones_mask(), 0b101);
        assert_eq!(fresh_worker.active_zones_mask(), 0b101);
        assert!(fresh_worker.zone_names()[0].is_some());
        assert!(fresh_worker.zone_names()[1].is_none());
        assert!(fresh_worker.zone_names()[2].is_some());

        // 4. Limpiar Keyzones vía IPC
        handle_native_ipc_message(
            "z,clear",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        assert_eq!(live_params.get_active_keyzones_mask(), 0);
        assert_eq!(parse_resize_ipc("resize:1127,828"), Some((1127, 828)));
        assert_eq!(parse_resize_ipc("\"resize:200,100\""), Some((640, 480)));

        // 5. ODD UI Telemetry counters (preset, matrix, resize)
        assert_eq!(live_params.get_ui_preset_loads(), 0);
        assert_eq!(live_params.get_ui_matrix_switches(), 0);
        assert_eq!(live_params.get_ui_resize_events(), 0);

        handle_native_ipc_message(
            "preset:joropo-arpa-01",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        handle_native_ipc_message(
            "matrix:2,1",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        handle_native_ipc_message(
            "resize:1127,828",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );

        assert_eq!(live_params.get_ui_preset_loads(), 1);
        assert_eq!(live_params.get_ui_matrix_switches(), 1);
        assert_eq!(live_params.get_ui_resize_events(), 1);
        assert_eq!(live_params.get_mod_matrix_mask(), 1 << (2 * 5 + 1));

        // 6. Probar preset_apply:<json>, fav_toggle:<id> y user_preset_save:<json>
        handle_native_ipc_message(
            r#"preset_apply:{"attack":0.04,"cutoff":3200.0,"resonance":2.4,"lfo_rate":6.5,"lfo_depth":0.6,"arp_mode":1,"mod_matrix_mask":130}"#,
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        handle_native_ipc_message(
            "fav_toggle:joropo-arpa-01",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        handle_native_ipc_message(
            r#"user_preset_save:{"id":"usr-odd-1","name":"Custom ODD Lead","category":"synthwave"}"#,
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );

        assert_eq!(live_params.get_ui_preset_loads(), 2);
        assert_eq!(live_params.get_ui_fav_toggles(), 1);
        assert_eq!(live_params.get_user_presets_saved(), 1);
        assert_eq!(live_params.get_mod_matrix_mask(), 130);
        assert!((live_params.get_cutoff() - 3200.0).abs() < 1e-2);

        // 7. Probar Velocity IPC (1,note,vel) y controles de expresion (pb, mw, sus)
        handle_native_ipc_message(
            "1,60,0.42",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        handle_native_ipc_message(
            "pb,1.5",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        handle_native_ipc_message(
            "mw,0.75",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        handle_native_ipc_message(
            "sus,1",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );

        assert!((live_params.get_last_velocity() - 0.42).abs() < 1e-4);
        assert_eq!(live_params.get_velocity_events(), 1);
        assert!((live_params.get_pitch_bend_semitones() - 1.5).abs() < 1e-4);
        assert!((live_params.get_mod_wheel() - 0.75).abs() < 1e-4);
        assert!(live_params.get_sustain_pedal());

        let telem_odd_json = build_telemetry_json(&params, &live_params, &ai_worker);
        assert!(telem_odd_json.contains(r#""ui_preset_loads":2"#));
        assert!(telem_odd_json.contains(r#""ui_matrix_switches":1"#));
        assert!(telem_odd_json.contains(r#""ui_resize_events":1"#));
        assert!(telem_odd_json.contains(r#""ui_fav_toggles":1"#));
        assert!(telem_odd_json.contains(r#""user_presets_saved":1"#));
        assert!(telem_odd_json.contains(r#""mod_matrix_active_routes":2"#));
        assert!(telem_odd_json.contains(r#""velocity_events":1"#));
        assert!(telem_odd_json.contains(r#""sustain_pedal":true"#));
        assert!(telem_odd_json.contains(r#""ai_presets_synthesized":"#));
        assert!(telem_odd_json.contains(r#""piano_roll_auto_trims":"#));
        assert!(telem_odd_json.contains(r#""last_detected_pitch_hz":"#));
        assert!(telem_odd_json.contains(r#""cloud_tier":"#));
        assert!(telem_odd_json.contains(r#""cloud_credits_remaining":"#));
        assert!(telem_odd_json.contains(r#""cloud_kv_cache_hits":"#));
        assert!(telem_odd_json.contains(r#""cloud_daily_neurons":"#));

        // 8. Probar arquitecto IA de Presets + Samples (gen:<prompt>) auto-poblando LiveParamStore
        handle_native_ipc_message(
            "gen:Bright Joropo Arpa Llanera Pluck E4",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        assert_eq!(live_params.get_root_note(), 64);
        assert_eq!(live_params.get_arp_mode(), synth_core::ArpMode::Joropo);
        assert!(live_params.get_cutoff() > 8000.0);

        // 9. Probar resíntesis Audio-to-Wavetable vía IPC (wtdraw:from_sample)
        handle_native_ipc_message(
            "wtdraw:from_sample",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        assert!(live_params.is_custom_wavetable_active());
        assert_eq!(live_params.get_custom_wavetable_draws(), 1);

        // 10. Probar Dual Oscillator Cross-Modulation IPC (fmb,<0..1>)
        handle_native_ipc_message(
            "fmb,0.65",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        assert!((live_params.get_fm_from_b() - 0.65).abs() < 1e-4);
        assert!(live_params.get_osc_b_level() > 0.45);

        let telem_final = build_telemetry_json(&params, &live_params, &ai_worker);
        assert!(telem_final.contains(r#""hardware_midi_events":"#));
        assert!(telem_final.contains(r#""fm_from_b":0.65"#));
    }

    #[test]
    fn test_every_ui_control_and_oled_impacts_dsp_audio_output() {
        use synth_core::{SampleBuffer, StereoFxRack, VoiceManager};

        let sr = 48000.0_f32;
        let (gui_midi_tx, _gui_midi_rx) = crossbeam_channel::bounded(64);
        let live_params = Arc::new(LiveParamStore::default());
        let ai_worker = Arc::new(AiWorker::new(sr));
        let sample_rate = Arc::new(AtomicU32::new(sr.to_bits()));
        let params = Arc::new(SamplerParams::new_linked(
            live_params.clone(),
            Some(ai_worker.clone()),
            Some(sample_rate.clone()),
        ));

        // Señal base rica en armónicos (C4 = 261.63 Hz + armónicos 2x, 3x, 4x, 6x, 8x)
        let samples: Vec<f32> = (0..12000)
            .map(|i| {
                let t = i as f32 / sr;
                let w = std::f32::consts::TAU * 261.63 * t;
                (0.50 * w.sin()
                    + 0.22 * (w * 2.0).sin()
                    + 0.14 * (w * 3.0).sin()
                    + 0.08 * (w * 4.0).sin()
                    + 0.04 * (w * 6.0).sin()
                    + 0.02 * (w * 8.0).sin())
                    * 0.65
            })
            .collect();
        let mut test_sample = SampleBuffer::new_mono(samples, sr, 60);
        test_sample.loop_enabled = true;
        test_sample.loop_start = 480;
        test_sample.loop_end = 9600;

        let send = |msg: &str| {
            handle_native_ipc_message(
                msg,
                &gui_midi_tx,
                &params,
                &live_params,
                &ai_worker,
                &sample_rate,
                &None,
            );
        };

        let render_audio = |lp: &LiveParamStore, note: u8, frames: usize| -> (Vec<f32>, Vec<f32>) {
            let mut vm = VoiceManager::new(sr, 16);
            let mut fx = StereoFxRack::new(sr);

            vm.update_envelope_params(
                lp.get_attack(),
                lp.get_decay(),
                lp.get_sustain(),
                lp.get_release(),
            );
            vm.update_filter_params(lp.get_cutoff(), lp.get_resonance(), lp.get_filter_mode());
            vm.set_filter_keytrack(lp.get_filter_keytrack());
            vm.update_lfo_params(lp.get_lfo_rate(), lp.get_lfo_depth(), lp.get_lfo_target());
            vm.set_mod_matrix_mask(lp.get_mod_matrix_mask());
            vm.update_vital_serum_params(
                lp.get_wt_pos(),
                lp.get_wt_warp(),
                lp.get_unison_voices(),
                lp.get_unison_detune(),
                lp.get_sub_level(),
                lp.get_lfo_shape(),
                lp.get_macros(),
            );
            vm.set_osc_b_fm_params(lp.get_fm_from_b(), lp.get_osc_b_level());
            if lp.is_custom_wavetable_active() {
                let cycle = lp.get_custom_wavetable_cycle();
                vm.set_custom_wavetable_cycle(&cycle);
            }
            vm.update_arp_params(lp.get_arp_mode(), lp.get_arp_bpm());
            if lp.is_arp_custom_accents_active() {
                let accents = lp.get_arp_step_accents();
                vm.set_arp_step_accents(&accents, true);
            }
            vm.set_root_note_override(Some(lp.get_root_note() as u8), &test_sample);
            vm.set_region_override(lp.get_region_override());
            vm.set_pitch_bend(lp.get_pitch_bend_semitones());
            vm.set_mod_wheel(lp.get_mod_wheel());
            vm.set_sustain_pedal(lp.get_sustain_pedal());

            let m4 = lp.get_macro(3);
            fx.set_mix((lp.get_space_mix() + m4 * 0.35).clamp(0.0, 1.0));
            fx.set_drive((lp.get_drive_amount() + m4 * 0.45).clamp(0.0, 1.0));
            fx.set_ott((lp.get_ott_amount() + m4 * 0.50).clamp(0.0, 1.0));
            fx.set_tempo_sync(lp.get_arp_bpm(), lp.get_arp_mode());

            vm.note_on(note, lp.get_last_velocity(), &test_sample);
            let mut l = vec![0.0_f32; frames];
            let mut r = vec![0.0_f32; frames];
            vm.process_block(&test_sample, &mut l, &mut r);
            fx.process_block(&mut l, &mut r);

            let gain = lp.get_gain();
            for (sl, sr_val) in l.iter_mut().zip(r.iter_mut()) {
                *sl *= gain;
                *sr_val *= gain;
            }
            lp.recorder.push_stereo_block_rt(&l, &r, true);
            (l, r)
        };

        let l1_diff = |a: &[f32], b: &[f32]| -> f32 {
            a.iter().zip(b.iter()).map(|(x, y)| (x - y).abs()).sum()
        };

        // 0. Línea base limpia (Sin efectos espaciales para aislar oscilador)
        send("p,space_mix,0.0");
        let (base_l, base_r) = render_audio(&live_params, 60, 1024);
        assert!(base_l.iter().map(|x| x.abs()).sum::<f32>() > 10.0);

        // 1. MÓDULO 1 — Wavetable 3D OLED / Knobs (wt_pos, wt_warp)
        send("p,wt_pos,0.85");
        send("p,wt_warp,0.75");
        let (wt_l, _) = render_audio(&live_params, 60, 1024);
        assert!(
            l1_diff(&base_l, &wt_l) > 5.0,
            "Module 1 WT POS + WARP must alter synthesized waveform"
        );
        send("p,wt_pos,0.0");
        send("p,wt_warp,0.0");

        // 2. MÓDULO 1 — Unison Voices Button (uni,8) + Detune Knob (p,unison_detune,0.85)
        send("uni,8");
        send("p,unison_detune,0.85");
        assert_eq!(live_params.get_unison_voices(), 8);
        let (uni_l, uni_r) = render_audio(&live_params, 60, 1024);
        let base_stereo_spread = l1_diff(&base_l, &base_r);
        let uni_stereo_spread = l1_diff(&uni_l, &uni_r);
        assert!(
            uni_stereo_spread > base_stereo_spread + 5.0,
            "Module 1 UNI:8V + Detune must widen stereo field: base={}, uni={}",
            base_stereo_spread,
            uni_stereo_spread
        );
        send("uni,1");
        send("p,unison_detune,0.20");

        // 3. MÓDULO 1 — Sub-Oscillator (-1 Oct) Knob & FM B Cross-Mod Button
        send("p,sub_level,0.95");
        let (sub_l, _) = render_audio(&live_params, 60, 1024);
        assert!(
            l1_diff(&base_l, &sub_l) > 8.0,
            "Module 1 SUB -1 Knob must inject sub-octave energy"
        );
        send("p,sub_level,0.0");

        send("fmb,0.85");
        let (fm_l, _) = render_audio(&live_params, 60, 1024);
        assert!(
            l1_diff(&base_l, &fm_l) > 8.0,
            "Module 1 FM B Button must modulate phase/harmonics"
        );
        send("fmb,0.0");

        // 4. MÓDULO 1 — WT DRAW OLED Freehand Wavetable Drawing
        send("wtdraw:0.9,0.7,-0.9,0.8,-0.8,0.95,-0.95,0.6,-0.6,0.85,-0.85,0.4,-0.4,0.9,-0.9,0.0");
        let (draw_l, _) = render_audio(&live_params, 60, 1024);
        assert!(
            l1_diff(&base_l, &draw_l) > 5.0,
            "Module 1 WT DRAW OLED must synthesize custom hand-drawn cycle"
        );
        send("wtdraw:off");

        // 5. MÓDULO 2 — SVF Filter 2D OLED / Knobs (Cutoff, Resonance, Mode LP/HP/BP, Keytrack)
        send("m,0"); // Lowpass
        send("p,cutoff,0.12"); // Low cutoff (~150 Hz)
        send("p,resonance,0.55");
        let (lp_l, _) = render_audio(&live_params, 60, 1024);
        assert!(
            l1_diff(&base_l, &lp_l) > 10.0,
            "Module 2 SVF Lowpass Cutoff/Resonance must filter harmonics"
        );

        // Keytrack: con KEY:100% (kt,1.00) vs KEY:OFF (kt,0.00) en nota aguda C6 (MIDI 84, ~1046 Hz)
        send("p,cutoff,0.45"); // ~850 Hz
        send("kt,0.00");
        let (kt_off_l, _) = render_audio(&live_params, 84, 1024);
        send("kt,1.00");
        let (kt_on_l, _) = render_audio(&live_params, 84, 1024);
        assert!(
            l1_diff(&kt_off_l, &kt_on_l) > 2.0,
            "Module 2 KEY:100% button must open filter cutoff on high notes compared to KEY:OFF"
        );
        send("kt,0.50");
        send("p,cutoff,1.0");
        send("p,resonance,0.061");

        // 6. MÓDULO 3 — ADSR Envelope 2D OLED / Faders
        send("p,attack,0.85"); // Slow attack
        let (slow_atk_l, _) = render_audio(&live_params, 60, 512);
        let fast_early_energy: f32 = base_l[..256].iter().map(|x| x.abs()).sum();
        let slow_early_energy: f32 = slow_atk_l[..256].iter().map(|x| x.abs()).sum();
        assert!(
            fast_early_energy > slow_early_energy * 3.0,
            "Module 3 ADSR Attack fader/OLED must ramp initial amplitude: fast={}, slow={}",
            fast_early_energy,
            slow_early_energy
        );
        send("p,attack,0.05");

        // 7. MÓDULO 4 — LFO Knobs, Shape OLED & 5x5 Mod Matrix Grid
        send("p,lfo_rate,0.75");
        send("p,lfo_depth,0.95");
        send("lt,1"); // Pitch Vibrato
        send("ls,2"); // Saw LFO
        let (lfo_l, _) = render_audio(&live_params, 60, 2048);
        let (base_2k_l, _) = {
            send("p,lfo_depth,0.0");
            render_audio(&live_params, 60, 2048)
        };
        assert!(
            l1_diff(&base_2k_l, &lfo_l) > 5.0,
            "Module 4 LFO Rate/Depth/Shape must modulate pitch/filter"
        );

        // Pin de Matriz 5x5: LFO2 -> PAN (fila 1, col 3)
        send("matrix:1,3,1");
        let (mat_l, mat_r) = render_audio(&live_params, 60, 1024);
        assert!(
            l1_diff(&mat_l, &mat_r) > 1.0,
            "Module 4 5x5 Mod Matrix LFO2->PAN pin must create stereo pan modulation"
        );
        send("matrix:1,3,0");

        // 8. MÓDULO 5 — Poly Arpeggiator & Interactive Step Velocity OLED (#oled-arp)
        send("arp,3"); // 6/8 Joropo Sync
        send("arpstep,0,0.18"); // Esculpir paso 1 muy suave en el OLED
        let (arp_soft_l, _) = render_audio(&live_params, 60, 1024);
        send("arpstep,0,1.00"); // Esculpir paso 1 al 100% en el OLED
        let (arp_loud_l, _) = render_audio(&live_params, 60, 1024);
        let soft_arp_e: f32 = arp_soft_l.iter().map(|x| x.abs()).sum();
        let loud_arp_e: f32 = arp_loud_l.iter().map(|x| x.abs()).sum();
        assert!(
            loud_arp_e > soft_arp_e * 2.0,
            "Module 5 #oled-arp step bar sculpting must control arpeggiator step velocity: loud={}, soft={}",
            loud_arp_e,
            soft_arp_e
        );
        send("arp,0");

        // 9. MÓDULO 6 — OTT, Drive, Spatial Canyon 2D OLED & Master Recorder
        send("rec,start");
        send("p,drive,0.85");
        send("p,ott,0.80");
        send("p,space_mix,0.65");
        let (fx_l, _) = render_audio(&live_params, 60, 1024);
        send("rec,stop");
        assert!(
            l1_diff(&base_l, &fx_l) > 10.0,
            "Module 6 Drive + OTT + Spatial Canyon must saturate, compress, and diffuse audio"
        );
        assert!(
            live_params.recorder.recorded_frames() >= 1024,
            "Module 6 REC LIVE must capture synthesized stereo frames"
        );
    }
}
