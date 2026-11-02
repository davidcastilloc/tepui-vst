//! Editor Gráfico Nativo WebView2 (Microsoft Edge Chromium) para VST3 y CLAP.
//!
//! Incrusta la interfaz HTML5/CSS3/JS directamente dentro de la ventana nativa (HWND)
//! que entrega el DAW (Ableton Live, FL Studio, Reaper, Bitwig) o la app standalone.
//!
//! 100% Serverless: Cero servidores HTTP, cero WebSockets, cero puertos TCP abiertos.
//! Todos los assets se cargan en memoria vía protocolo virtual nativo `tepui://app/`.

use std::any::Any;
use std::borrow::Cow;
use std::num::NonZeroIsize;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use ai_bridge::AiWorker;
use crossbeam_channel::Sender;
use nih_plug::prelude::{Editor, GuiContext, Param, ParentWindowHandle};
use raw_window_handle_06::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawWindowHandle,
    Win32WindowHandle, WindowHandle,
};
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
    arp_mode: i32,
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
}

pub struct NativeHwnd(pub isize);

impl HasWindowHandle for NativeHwnd {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let nz = NonZeroIsize::new(self.0).ok_or(HandleError::Unavailable)?;
        let win32 = Win32WindowHandle::new(nz);
        unsafe { Ok(WindowHandle::borrow_raw(RawWindowHandle::Win32(win32))) }
    }
}

impl HasDisplayHandle for NativeHwnd {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::windows())
    }
}

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
#[allow(non_camel_case_types, clippy::upper_case_acronyms)]
mod win32_pump {
    use std::ffi::c_void;

    pub type HWND = *mut c_void;
    pub type UINT_PTR = usize;
    pub type UINT = u32;
    pub type DWORD = u32;
    pub type BOOL = i32;

    #[repr(C)]
    pub struct POINT {
        pub x: i32,
        pub y: i32,
    }

    #[repr(C)]
    pub struct MSG {
        pub hwnd: HWND,
        pub message: UINT,
        pub wparam: usize,
        pub lparam: isize,
        pub time: DWORD,
        pub pt: POINT,
        pub lprivate: DWORD,
    }

    pub type TIMERPROC = Option<unsafe extern "system" fn(HWND, UINT, UINT_PTR, DWORD)>;
    pub type HICON = *mut c_void;

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct RECT {
        pub left: i32,
        pub top: i32,
        pub right: i32,
        pub bottom: i32,
    }

    pub struct ActiveResizeState {
        pub hwnd: HWND,
        pub track_hwnd: HWND,
        pub webview: std::rc::Rc<wry::WebView>,
        pub size_w: std::sync::Arc<std::sync::atomic::AtomicU32>,
        pub size_h: std::sync::Arc<std::sync::atomic::AtomicU32>,
        pub last_w: i32,
        pub last_h: i32,
    }

    thread_local! {
        static ACTIVE_WEBVIEW_RESIZE: std::cell::RefCell<Option<ActiveResizeState>> = const { std::cell::RefCell::new(None) };
    }

    extern "system" {
        pub fn SetTimer(
            hwnd: HWND,
            id_event: UINT_PTR,
            elapse: UINT,
            timer_func: TIMERPROC,
        ) -> UINT_PTR;
        pub fn KillTimer(hwnd: HWND, id_event: UINT_PTR) -> BOOL;
        pub fn PeekMessageW(
            msg: *mut MSG,
            hwnd: HWND,
            msg_filter_min: UINT,
            msg_filter_max: UINT,
            remove_msg: UINT,
        ) -> BOOL;
        pub fn TranslateMessage(msg: *const MSG) -> BOOL;
        pub fn DispatchMessageW(msg: *const MSG) -> isize;
        pub fn GetAncestor(hwnd: HWND, ga_flags: UINT) -> HWND;
        pub fn GetParent(hwnd: HWND) -> HWND;
        pub fn GetClientRect(hwnd: HWND, lp_rect: *mut RECT) -> BOOL;
        pub fn GetWindowRect(hwnd: HWND, lp_rect: *mut RECT) -> BOOL;
        pub fn GetWindowLongW(hwnd: HWND, n_index: i32) -> i32;
        pub fn SetWindowLongW(hwnd: HWND, n_index: i32, dw_new_long: i32) -> i32;
        pub fn SetWindowPos(
            hwnd: HWND,
            hwnd_insert_after: HWND,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            u_flags: UINT,
        ) -> BOOL;
        pub fn SendMessageW(hwnd: HWND, msg: UINT, wparam: usize, lparam: isize) -> isize;
        pub fn CreateIconFromResourceEx(
            presbits: *const u8,
            dwres_size: DWORD,
            f_icon: BOOL,
            dw_ver: DWORD,
            cx_desired: i32,
            cy_desired: i32,
            flags: UINT,
        ) -> HICON;
    }

    pub const PM_REMOVE: UINT = 0x0001;
    pub const TIMER_ID: UINT_PTR = 0xAE87A;
    pub const GA_ROOT: UINT = 2;
    pub const WM_SETICON: UINT = 0x0080;
    pub const ICON_SMALL: usize = 0;
    pub const ICON_BIG: usize = 1;
    pub const GWL_STYLE: i32 = -16;
    pub const WS_THICKFRAME: i32 = 0x0004_0000;
    pub const WS_MAXIMIZEBOX: i32 = 0x0001_0000;
    pub const WS_MINIMIZEBOX: i32 = 0x0002_0000;
    pub const WS_CLIPCHILDREN: i32 = 0x0200_0000;
    pub const SWP_NOSIZE: UINT = 0x0001;
    pub const SWP_NOMOVE: UINT = 0x0002;
    pub const SWP_NOZORDER: UINT = 0x0004;
    pub const SWP_NOACTIVATE: UINT = 0x0010;
    pub const SWP_FRAMECHANGED: UINT = 0x0020;

    pub unsafe fn setup_resizable_window(hwnd: HWND, initial_w: u32, initial_h: u32) -> HWND {
        let root_hwnd = GetAncestor(hwnd, GA_ROOT);
        let parent_hwnd = GetParent(hwnd);
        let target = if !root_hwnd.is_null() {
            root_hwnd
        } else {
            hwnd
        };

        let mut root_client = RECT::default();
        let is_dedicated_window = if !target.is_null()
            && GetClientRect(target, &mut root_client) != 0
        {
            let cw = root_client.right - root_client.left;
            let ch = root_client.bottom - root_client.top;
            target == hwnd
                || parent_hwnd == target
                || ((cw - initial_w as i32).abs() <= 160 && (ch - initial_h as i32).abs() <= 160)
        } else {
            false
        };

        if is_dedicated_window && !target.is_null() {
            let style = GetWindowLongW(target, GWL_STYLE);
            let new_style =
                style | WS_THICKFRAME | WS_MAXIMIZEBOX | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
            if new_style != style {
                SetWindowLongW(target, GWL_STYLE, new_style);
                SetWindowPos(
                    target,
                    std::ptr::null_mut(),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                );
            }
            target
        } else {
            hwnd
        }
    }

    pub fn register_active_webview(state: ActiveResizeState) {
        ACTIVE_WEBVIEW_RESIZE.with(|cell| {
            *cell.borrow_mut() = Some(state);
        });
    }

    pub fn clear_active_webview() {
        ACTIVE_WEBVIEW_RESIZE.with(|cell| {
            *cell.borrow_mut() = None;
        });
    }

    pub unsafe fn sync_webview_bounds() {
        ACTIVE_WEBVIEW_RESIZE.with(|cell| {
            if let Some(state) = cell.borrow_mut().as_mut() {
                if state.track_hwnd.is_null() {
                    return;
                }
                let mut rect = RECT::default();
                if GetClientRect(state.track_hwnd, &mut rect) != 0 {
                    let cw = rect.right - rect.left;
                    let ch = rect.bottom - rect.top;
                    if cw >= 320 && ch >= 240 && (cw != state.last_w || ch != state.last_h) {
                        state.last_w = cw;
                        state.last_h = ch;
                        state
                            .size_w
                            .store(cw as u32, std::sync::atomic::Ordering::Relaxed);
                        state
                            .size_h
                            .store(ch as u32, std::sync::atomic::Ordering::Relaxed);
                        if state.hwnd != state.track_hwnd && !state.hwnd.is_null() {
                            SetWindowPos(
                                state.hwnd,
                                std::ptr::null_mut(),
                                0,
                                0,
                                cw,
                                ch,
                                SWP_NOZORDER | SWP_NOACTIVATE,
                            );
                        }
                        let _ = state.webview.set_bounds(wry::Rect {
                            position: wry::dpi::PhysicalPosition::new(0, 0).into(),
                            size: wry::dpi::PhysicalSize::new(cw as u32, ch as u32).into(),
                        });
                    }
                }
            }
        });
    }

    pub unsafe fn request_window_resize(new_w: u32, new_h: u32) {
        ACTIVE_WEBVIEW_RESIZE.with(|cell| {
            if let Some(state) = cell.borrow_mut().as_mut() {
                let cw = new_w.clamp(640, 3840) as i32;
                let ch = new_h.clamp(480, 2160) as i32;
                state.last_w = cw;
                state.last_h = ch;
                state
                    .size_w
                    .store(cw as u32, std::sync::atomic::Ordering::Relaxed);
                state
                    .size_h
                    .store(ch as u32, std::sync::atomic::Ordering::Relaxed);

                if !state.track_hwnd.is_null() {
                    let mut win_rect = RECT::default();
                    let mut client_rect = RECT::default();
                    let (mut outer_w, mut outer_h) = (cw, ch);
                    if GetWindowRect(state.track_hwnd, &mut win_rect) != 0
                        && GetClientRect(state.track_hwnd, &mut client_rect) != 0
                    {
                        let cur_client_w = (client_rect.right - client_rect.left).max(1);
                        let cur_client_h = (client_rect.bottom - client_rect.top).max(1);
                        let border_w = ((win_rect.right - win_rect.left) - cur_client_w).max(0);
                        let border_h = ((win_rect.bottom - win_rect.top) - cur_client_h).max(0);
                        outer_w = cw + border_w;
                        outer_h = ch + border_h;
                    }
                    SetWindowPos(
                        state.track_hwnd,
                        std::ptr::null_mut(),
                        0,
                        0,
                        outer_w,
                        outer_h,
                        SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }

                if state.hwnd != state.track_hwnd && !state.hwnd.is_null() {
                    SetWindowPos(
                        state.hwnd,
                        std::ptr::null_mut(),
                        0,
                        0,
                        cw,
                        ch,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }

                let _ = state.webview.set_bounds(wry::Rect {
                    position: wry::dpi::PhysicalPosition::new(0, 0).into(),
                    size: wry::dpi::PhysicalSize::new(cw as u32, ch as u32).into(),
                });
            }
        });
    }

    pub unsafe fn apply_window_icon(hwnd: HWND) {
        let png_bytes = include_bytes!("../ui/tepui-icon.png");
        let hicon_big = CreateIconFromResourceEx(
            png_bytes.as_ptr(),
            png_bytes.len() as DWORD,
            1,
            0x00030000,
            64,
            64,
            0,
        );
        let hicon_small = CreateIconFromResourceEx(
            png_bytes.as_ptr(),
            png_bytes.len() as DWORD,
            1,
            0x00030000,
            32,
            32,
            0,
        );
        let root_hwnd = GetAncestor(hwnd, GA_ROOT);
        for target in [hwnd, root_hwnd] {
            if !target.is_null() {
                if !hicon_small.is_null() {
                    SendMessageW(target, WM_SETICON, ICON_SMALL, hicon_small as isize);
                }
                if !hicon_big.is_null() {
                    SendMessageW(target, WM_SETICON, ICON_BIG, hicon_big as isize);
                }
            }
        }
    }

    pub unsafe extern "system" fn timer_pump_proc(
        _hwnd: HWND,
        _msg: UINT,
        _id: UINT_PTR,
        _time: DWORD,
    ) {
        sync_webview_bounds();
        let mut msg: MSG = std::mem::zeroed();
        let mut count = 0;
        // Drenar mensajes dirigidos a hilos y ventanas hijas de WebView2 que baseview omite en GetMessage
        while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            count += 1;
            if count > 64 {
                break;
            }
        }
    }
}

struct WebEditorHandle {
    _webview: Rc<WebView>,
    #[cfg(windows)]
    parent_hwnd: win32_pump::HWND,
}

impl Drop for WebEditorHandle {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            win32_pump::clear_active_webview();
            win32_pump::KillTimer(self.parent_hwnd, win32_pump::TIMER_ID);
        }
    }
}

// Safety: el handle es liberado exclusivamente en el hilo GUI del DAW al cerrar la ventana.
unsafe impl Send for WebEditorHandle {}

fn load_asset_bytes(filename: &str, embedded: &'static [u8]) -> Cow<'static, [u8]> {
    let candidates = [
        format!(
            "C:/Users/pc911/plugin_vst/crates/plugin_vst/ui/{}",
            filename
        ),
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
    };

    let init_msg = InitMessage {
        msg_type: "init",
        params: ParamValues {
            attack: live_params.get_attack(),
            decay: live_params.get_decay(),
            sustain: live_params.get_sustain(),
            release: live_params.get_release(),
            cutoff: live_params.get_cutoff(),
            resonance: live_params.get_resonance(),
            filter_mode: match live_params.get_filter_mode() {
                synth_core::FilterMode::Lowpass => 0,
                synth_core::FilterMode::Highpass => 1,
                synth_core::FilterMode::Bandpass => 2,
                synth_core::FilterMode::Bypass => 3,
            },
            gain: live_params.get_gain(),
            space_mix: live_params.get_space_mix(),
            root_note: effective_root,
            lfo_rate: live_params.get_lfo_rate(),
            lfo_depth: live_params.get_lfo_depth(),
            lfo_target: live_params.get_lfo_target().as_i32(),
            arp_mode: live_params.get_arp_mode().as_i32(),
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
    };

    serde_json::to_string(&init_msg).unwrap_or_else(|_| "{}".to_string())
}

pub fn build_telemetry_json(
    params: &SamplerParams,
    live_params: &LiveParamStore,
    ai_worker: &AiWorker,
) -> String {
    let sample = ai_worker.get_sample();
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
            filter_mode: match live_params.get_filter_mode() {
                synth_core::FilterMode::Lowpass => 0,
                synth_core::FilterMode::Highpass => 1,
                synth_core::FilterMode::Bandpass => 2,
                synth_core::FilterMode::Bypass => 3,
            },
            gain: live_params.get_gain(),
            space_mix: live_params.get_space_mix(),
            root_note: effective_root,
            lfo_rate: live_params.get_lfo_rate(),
            lfo_depth: live_params.get_lfo_depth(),
            lfo_target: live_params.get_lfo_target().as_i32(),
            arp_mode: live_params.get_arp_mode().as_i32(),
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
    let audio_status = crate::audio_devices::enumerate_audio_system(&config);
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
            let mode = (norm_val.round() as i32).clamp(0, 3);
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

fn handle_native_ipc_message(
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
                Ok(_) => println!("[TEPUI CONFIG] Guardado via IPC exitosamente: {:?}", config),
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
                let _ = gui_midi_tx.try_send(GuiMidiEvent::NoteOn(note, vel.clamp(0.0, 1.0)));
                return;
            }
        }
    }

    // 2. Ultra-fast compact path: "0,note" o "0:note" o "f:note" o "note_off:note" (NoteOff)
    let note_off_rest = trimmed
        .strip_prefix("0,")
        .or_else(|| trimmed.strip_prefix("0:"))
        .or_else(|| trimmed.strip_prefix("f:"))
        .or_else(|| trimmed.strip_prefix("note_off:"));

    if let Some(rest) = note_off_rest {
        if let Ok(note) = rest.trim().parse::<u8>() {
            let _ = gui_midi_tx.try_send(GuiMidiEvent::NoteOff(note));
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

    // 5. Prefix paths
    if let Some(prompt) = trimmed.strip_prefix("gen:") {
        let sr = f32::from_bits(sample_rate.load(Ordering::Relaxed));
        ai_worker.request_generation(prompt.trim().to_string(), sr, None);
        return;
    }

    if let Some(prompt) = trimmed.strip_prefix("generate:") {
        let sr = f32::from_bits(sample_rate.load(Ordering::Relaxed));
        ai_worker.request_generation(prompt.trim().to_string(), sr, None);
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
                        let vel = msg.velocity.unwrap_or(0.9);
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
                        let sr = f32::from_bits(sample_rate.load(Ordering::Relaxed));
                        ai_worker.request_generation(prompt, sr, None);
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

impl Editor for WebViewEditor {
    fn spawn(
        &self,
        parent: ParentWindowHandle,
        context: Arc<dyn GuiContext>,
    ) -> Box<dyn Any + Send> {
        match parent {
            ParentWindowHandle::Win32Hwnd(hwnd) => {
                let hwnd_isize = hwnd as isize;
                let native_window = NativeHwnd(hwnd_isize);
                let (w, h) = self.size();
                println!(
                    "[TEPUI WEBVIEW] Incrustando WebView2 Serverless (tepui://app/) en HWND {:?}",
                    hwnd
                );

                #[cfg(windows)]
                unsafe {
                    win32_pump::apply_window_icon(hwnd as win32_pump::HWND);
                    let _ = win32_pump::setup_resizable_window(hwnd as win32_pump::HWND, w, h);
                    win32_pump::SetTimer(
                        hwnd as win32_pump::HWND,
                        win32_pump::TIMER_ID,
                        8,
                        Some(win32_pump::timer_pump_proc),
                    );
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
                let size_w_reg = self.size_w.clone();
                let size_h_reg = self.size_h.clone();

                let params_proto = self.params.clone();
                let live_params_proto = self.live_params.clone();
                let ai_worker_proto = self.ai_worker.clone();
                let gui_param_tx_proto = self.gui_param_tx.clone();

                let builder = WebViewBuilder::new().with_background_color((12, 14, 18, 255));

                let builder = builder
                    .with_custom_protocol("tepui".into(), move |_webview_id, req: Request<Vec<u8>>| {
                        let path = req.uri().path().trim_start_matches('/');
                        let method = req.method().as_str();
                        if !path.ends_with("api/telemetry")
                            && !path.ends_with("api/init-state")
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
                                .header("Access-Control-Allow-Origin", "*")
                                .body(Cow::Owned(bundled.into_bytes()))
                                .unwrap()
                        } else if path == "style.css" || path.ends_with("/style.css") {
                            let bytes = load_asset_bytes("style.css", include_bytes!("../ui/style.css"));
                            Response::builder()
                                .header(CONTENT_TYPE, "text/css; charset=utf-8")
                                .header("Access-Control-Allow-Origin", "*")
                                .body(bytes)
                                .unwrap()
                        } else if path == "app.js" || path.ends_with("/app.js") {
                            let bytes = load_asset_bytes("app.js", include_bytes!("../ui/app.js"));
                            Response::builder()
                                .header(CONTENT_TYPE, "application/javascript; charset=utf-8")
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
                            match ai_worker_proto.load_wav_bytes(req.body(), Some(sample_name)) {
                                Ok(loaded) => {
                                    let root = loaded.root_note as i32;
                                    live_params_proto.set_root_note(root);
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
                        } else if path.ends_with("api/export-wav") {
                            let sample = ai_worker_proto.get_sample();
                            let wav_bytes = synth_core::encode_wav_f32(&sample);
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
                                        println!("[TEPUI CONFIG] Guardado via Custom Protocol: {:?}", cfg);
                                        r#"{"success":true,"message":"Configuración guardada exitosamente"}"#.to_string()
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
                            #[cfg(windows)]
                            unsafe {
                                win32_pump::request_window_resize(rw, rh);
                            }
                            return;
                        }
                        handle_native_ipc_message(
                            req.body(),
                            &gui_midi_tx,
                            &params,
                            &live_params,
                            &ai_worker,
                            &sample_rate,
                            &gui_param_tx,
                        );
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
                        win32_pump::register_active_webview(win32_pump::ActiveResizeState {
                            hwnd: hwnd as win32_pump::HWND,
                            track_hwnd: unsafe {
                                win32_pump::setup_resizable_window(hwnd as win32_pump::HWND, w, h)
                            },
                            webview: Rc::clone(&webview),
                            size_w: size_w_reg,
                            size_h: size_h_reg,
                            last_w: w as i32,
                            last_h: h as i32,
                        });
                        let cur_url = webview.url().unwrap_or_default();
                        println!("[TEPUI WEBVIEW] WebView2 Serverless 0-lag incrustado exitosamente! URL: {}", cur_url);

                        let bounds = Rect {
                            position: LogicalPosition::new(0.0, 0.0).into(),
                            size: LogicalSize::new(w as f64, h as f64).into(),
                        };
                        if let Err(e) = webview.set_bounds(bounds) {
                            eprintln!("[TEPUI WEBVIEW] Error al configurar bounds: {:?}", e);
                        }
                        let _ = webview.focus();

                        #[cfg(windows)]
                        unsafe {
                            let mut msg: win32_pump::MSG = std::mem::zeroed();
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
                            }
                        }

                        Box::new(WebEditorHandle {
                            _webview: webview,
                            #[cfg(windows)]
                            parent_hwnd: hwnd as win32_pump::HWND,
                        })
                    }
                    Err(e) => {
                        eprintln!(
                            "[TEPUI WEBVIEW] Error creando WebView2 hijo en DAW: {:?}",
                            e
                        );
                        Box::new(())
                    }
                }
            }
            _ => {
                eprintln!("[TEPUI WEBVIEW] Handle de ventana no soportado (se requiere Win32Hwnd)");
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
    }
}
