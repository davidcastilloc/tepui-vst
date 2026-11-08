//! # `base_plugin_webview` — Host WebView Agnóstico, Aislamiento Multi-Instancia y Bus IPC Tipado
//!
//! Provee el editor nativo reutilizable `GenericWebViewEditor` (`nih_plug::prelude::Editor`),
//! el contrato de comandos IPC tipados (`GuiToHostCommand` y `HostToGuiEvent`),
//! el aislamiento dinámico de perfiles de usuario (`%LOCALAPPDATA%/AudioPlugins/<Vendor>/<PluginName>/WebViewProfile`),
//! las cabeceras de seguridad CSP para el protocolo virtual en memoria `app://localhost/`
//! y el bombeo de telemetría atómica a 60 FPS libre de contrapresión.

use std::any::Any;
use std::borrow::Cow;
use std::num::{NonZeroIsize, NonZeroU32};
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use base_plugin_core::{AtomicParamBank, ParamIndex, MAX_PLUGIN_PARAMS};
use nih_plug::prelude::{Editor, GuiContext, ParentWindowHandle};
use raw_window_handle_06::{
    AppKitWindowHandle, DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle,
    RawDisplayHandle, RawWindowHandle, Win32WindowHandle, WindowHandle, XcbDisplayHandle,
    XcbWindowHandle,
};
use serde::{Deserialize, Serialize};
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::http::{header::CONTENT_TYPE, Request, Response, StatusCode};
use wry::{Rect, WebViewBuilder};

/// Esquema de URI virtual en memoria libre de puertos TCP locales.
pub const DEFAULT_VIRTUAL_URI_SCHEME: &str = "app";

/// URL raíz de carga del documento principal del plugin.
pub const DEFAULT_VIRTUAL_INDEX_URL: &str = "app://localhost/index.html";

/// Política de seguridad de contenido (CSP) endurecida para el contenedor WebView.
pub const HARDENED_CSP_HEADER: &str = "default-src 'self' app: http://app.localhost https://app.localhost; script-src 'self' 'unsafe-inline' app: http://app.localhost https://app.localhost; style-src 'self' 'unsafe-inline' app: http://app.localhost https://app.localhost; img-src 'self' data: app:; connect-src 'self' app: http://app.localhost https://app.localhost; object-src 'none'; frame-ancestors 'none';";

/// Comandos tipados emitidos desde la interfaz gráfica (React / `@plugin-base/ui-bridge`) hacia el host Rust.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "data")]
pub enum GuiToHostCommand {
    /// Inicio de gesto de automatización DAW (Touch / Latch / Write).
    ParamGestureBegin { param_id: ParamIndex },
    /// Mutación de valor normalizado `[0.0, 1.0]`.
    ParamSetValue { param_id: ParamIndex, value: f32 },
    /// Fin de gesto de automatización DAW.
    ParamGestureEnd { param_id: ParamIndex },
    /// Nota MIDI activada desde teclado o pad virtual.
    MidiNoteOn { note: u8, velocity: f32 },
    /// Nota MIDI liberada desde teclado o pad virtual.
    MidiNoteOff { note: u8 },
    /// Pitch Bend bipolar en semitonos.
    MidiPitchBend { semitones: f32 },
    /// Solicitud de cambio de dimensiones lógicas de la ventana.
    RequestResize { width: u32, height: u32 },
    /// Carga de preset por identificador.
    LoadPreset { preset_id: String },
    /// Guardado de preset de usuario.
    SavePreset { name: String },
    /// Comando de extensión específico de un plugin concreto.
    CustomExtension {
        name: String,
        payload: serde_json::Value,
    },
}

/// Eventos tipados emitidos desde el host Rust hacia la interfaz gráfica.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "evt", content = "data")]
pub enum HostToGuiEvent {
    /// Sincronización de parámetro originada por automatización del DAW o carga de preset.
    ParamSync { param_id: ParamIndex, value: f32 },
    /// Cuadro de telemetría a 60 FPS para medidores y analizadores.
    MeterUpdate {
        peak_l: f32,
        peak_r: f32,
        rms_l: f32,
        rms_r: f32,
        active_voices: u16,
    },
    /// Confirmación de carga de preset.
    PresetLoaded { preset_id: String, success: bool },
}

/// Trait para que plugins derivados registren comandos IPC propios sin modificar `base_plugin_webview`.
pub trait IpcExtensionHandler: Send + Sync + 'static {
    /// Procesa un comando de extensión y retorna opcionalmente una respuesta JSON.
    fn handle_custom_command(
        &self,
        command_name: &str,
        payload: &serde_json::Value,
    ) -> Option<serde_json::Value>;
}

/// Resuelve el directorio de perfil de usuario (UDF) aislado por proveedor y nombre de plugin
/// para evitar bloqueos de base de datos SQLite en WebView2 cuando coexisten múltiples plugins.
pub fn resolve_isolated_webview_profile_dir(vendor_name: &str, plugin_name: &str) -> PathBuf {
    let sanitize = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    };
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);

    let host_exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "host".to_string());

    base.join("AudioPlugins")
        .join(sanitize(vendor_name))
        .join(sanitize(plugin_name))
        .join(format!("WebViewProfile_{}", sanitize(&host_exe)))
}

/// Puente atómico libre de contrapresión (zero-backpressure) para empujar telemetría a 60 FPS hacia el WebView.
pub struct AtomicMeterBridge {
    peak_l: AtomicU32,
    peak_r: AtomicU32,
    gr_db: AtomicU32,
}

impl Default for AtomicMeterBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl AtomicMeterBridge {
    /// Crea un nuevo puente de medidores atómicos.
    pub fn new() -> Self {
        Self {
            peak_l: AtomicU32::new(0.0_f32.to_bits()),
            peak_r: AtomicU32::new(0.0_f32.to_bits()),
            gr_db: AtomicU32::new(0.0_f32.to_bits()),
        }
    }

    /// Publica picos estéreo desde el hilo de audio en $O(1)$ (`Ordering::Relaxed`).
    #[inline(always)]
    pub fn publish_peaks_rt(&self, peak_l: f32, peak_r: f32) {
        self.peak_l
            .store(peak_l.abs().to_bits(), Ordering::Relaxed);
        self.peak_r
            .store(peak_r.abs().to_bits(), Ordering::Relaxed);
    }

    /// Publica picos estéreo y reducción de ganancia (dB) desde el hilo de audio en $O(1)$.
    #[inline(always)]
    pub fn publish_dynamics_rt(&self, peak_l: f32, peak_r: f32, gain_reduction_db: f32) {
        self.peak_l
            .store(peak_l.abs().to_bits(), Ordering::Relaxed);
        self.peak_r
            .store(peak_r.abs().to_bits(), Ordering::Relaxed);
        self.gr_db
            .store(gain_reduction_db.to_bits(), Ordering::Relaxed);
    }

    /// Lee `(peak_l, peak_r, gain_reduction_db)` de forma atómica.
    #[inline(always)]
    pub fn read_snapshot(&self) -> (f32, f32, f32) {
        (
            f32::from_bits(self.peak_l.load(Ordering::Relaxed)),
            f32::from_bits(self.peak_r.load(Ordering::Relaxed)),
            f32::from_bits(self.gr_db.load(Ordering::Relaxed)),
        )
    }

    /// Genera la expresión JavaScript compacta para evaluación directa a 60 FPS sin asignación HTTP.
    pub fn format_js_push_frame(&self) -> String {
        let (l, r, _gr) = self.read_snapshot();
        format!(
            "if(window.__onMeterFrame){{window.__onMeterFrame({:.5},{:.5});}}",
            l, r
        )
    }
}

/// Despacha un comando IPC JSON entrante sobre un `AtomicParamBank` y un manejador opcional de extensión.
pub fn dispatch_ipc_json<const MAX_PARAMS: usize>(
    raw_json: &str,
    param_bank: &AtomicParamBank<MAX_PARAMS>,
    extension: Option<&dyn IpcExtensionHandler>,
) -> Result<GuiToHostCommand, serde_json::Error> {
    let cmd: GuiToHostCommand = serde_json::from_str(raw_json)?;
    match &cmd {
        GuiToHostCommand::ParamSetValue { param_id, value } => {
            param_bank.store_normalized(*param_id, *value);
        }
        GuiToHostCommand::CustomExtension { name, payload } => {
            if let Some(ext) = extension {
                let _ = ext.handle_custom_command(name, payload);
            }
        }
        _ => {}
    }
    Ok(cmd)
}

/// Adaptador `raw-window-handle 0.6` sobre `ParentWindowHandle` de `nih-plug`.
pub struct NativeParentWindow(pub ParentWindowHandle);

impl HasWindowHandle for NativeParentWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        match self.0 {
            ParentWindowHandle::Win32Hwnd(hwnd) => {
                let nz = NonZeroIsize::new(hwnd as isize).ok_or(HandleError::Unavailable)?;
                let win32 = Win32WindowHandle::new(nz);
                unsafe { Ok(WindowHandle::borrow_raw(RawWindowHandle::Win32(win32))) }
            }
            ParentWindowHandle::AppKitNsView(ns_view) => {
                let ptr = NonNull::new(ns_view).ok_or(HandleError::Unavailable)?;
                let appkit = AppKitWindowHandle::new(ptr);
                unsafe { Ok(WindowHandle::borrow_raw(RawWindowHandle::AppKit(appkit))) }
            }
            ParentWindowHandle::X11Window(xid) => {
                let nz = NonZeroU32::new(xid).ok_or(HandleError::Unavailable)?;
                let xcb = XcbWindowHandle::new(nz);
                unsafe { Ok(WindowHandle::borrow_raw(RawWindowHandle::Xcb(xcb))) }
            }
        }
    }
}

impl HasDisplayHandle for NativeParentWindow {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        match self.0 {
            ParentWindowHandle::Win32Hwnd(_) => Ok(DisplayHandle::windows()),
            ParentWindowHandle::AppKitNsView(_) => Ok(DisplayHandle::appkit()),
            ParentWindowHandle::X11Window(_) => {
                let xcb = XcbDisplayHandle::new(None, 0);
                unsafe { Ok(DisplayHandle::borrow_raw(RawDisplayHandle::Xcb(xcb))) }
            }
        }
    }
}

/// Bombeo de mensajes Win32 (`HWND_MESSAGE`), sincronización de dimensiones y estilos de ventana redimensionable.
#[cfg(windows)]
#[allow(non_camel_case_types, clippy::upper_case_acronyms)]
pub mod win32_pump {
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
        static ACTIVE_WEBVIEW_RESIZE: std::cell::RefCell<Vec<ActiveResizeState>> = const { std::cell::RefCell::new(Vec::new()) };
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

    /// Habilita bordes redimensionables y botones de maximizar/minimizar cuando se ejecuta en ventana dedicada.
    ///
    /// # Safety
    /// Requiere un descriptor `HWND` válido del hilo GUI activo.
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
            let mut list = cell.borrow_mut();
            if let Some(existing) = list.iter_mut().find(|s| s.hwnd == state.hwnd) {
                *existing = state;
            } else {
                list.push(state);
            }
        });
    }

    pub fn remove_active_webview(hwnd: HWND) {
        ACTIVE_WEBVIEW_RESIZE.with(|cell| {
            cell.borrow_mut().retain(|s| s.hwnd != hwnd);
        });
    }

    /// Sincroniza los límites del WebView hijo con el rectángulo cliente de su ventana contenedora.
    ///
    /// # Safety
    /// Invocado desde el temporizador Win32 del hilo GUI propietario.
    pub unsafe fn sync_webview_bounds() {
        ACTIVE_WEBVIEW_RESIZE.with(|cell| {
            for state in cell.borrow_mut().iter_mut() {
                if state.track_hwnd.is_null() {
                    continue;
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

    /// Solicita programáticamente el cambio de dimensiones de la ventana del plugin.
    ///
    /// # Safety
    /// Opera sobre descriptores `HWND` registrados en el hilo actual.
    pub unsafe fn request_window_resize(target_hwnd: HWND, new_w: u32, new_h: u32) {
        ACTIVE_WEBVIEW_RESIZE.with(|cell| {
            let mut list = cell.borrow_mut();
            let opt_state = if !target_hwnd.is_null() {
                list.iter_mut().find(|s| s.hwnd == target_hwnd)
            } else {
                list.last_mut()
            };
            if let Some(state) = opt_state {
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

    /// Aplica un ícono PNG en memoria a la ventana del plugin y a su ventana raíz.
    ///
    /// # Safety
    /// Requiere `hwnd` válido y bytes PNG íntegros.
    pub unsafe fn apply_window_icon_png(hwnd: HWND, png_bytes: &[u8]) {
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

    /// Callback de temporizador Win32 que sincroniza bordes y drena mensajes `HWND_MESSAGE` de WebView2.
    ///
    /// # Safety
    /// Invocado por el despachador de mensajes de Windows (`WM_TIMER`).
    pub unsafe extern "system" fn timer_pump_proc(
        _hwnd: HWND,
        _msg: UINT,
        _id: UINT_PTR,
        _time: DWORD,
    ) {
        sync_webview_bounds();
        let mut msg: MSG = std::mem::zeroed();
        let mut count = 0;
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

struct ActiveWebViewHandle {
    #[allow(dead_code)]
    webview: std::rc::Rc<wry::WebView>,
    #[allow(dead_code)]
    web_context: wry::WebContext,
    #[cfg(windows)]
    parent_hwnd: win32_pump::HWND,
}

impl Drop for ActiveWebViewHandle {
    fn drop(&mut self) {
        #[cfg(windows)]
        if !self.parent_hwnd.is_null() {
            unsafe {
                win32_pump::remove_active_webview(self.parent_hwnd);
                win32_pump::KillTimer(self.parent_hwnd, win32_pump::TIMER_ID);
            }
        }
    }
}

// Seguro en plugins VST3/CLAP donde el host gestiona el ciclo de vida del editor en el hilo GUI.
unsafe impl Send for ActiveWebViewHandle {}

/// Callback para propagar cambios de parámetros de la UI hacia los ParamPtr de nih_plug (DAW automation).
pub type DawSyncCallback = Arc<dyn Fn(&dyn GuiContext, ParamIndex, f32) + Send + Sync>;

/// Editor WebView2/WebKit genérico y reutilizable para cualquier plugin basado en `base_plugin_core`.
pub struct GenericWebViewEditor {
    vendor_name: &'static str,
    plugin_name: &'static str,
    width: u32,
    height: u32,
    html_document: Arc<String>,
    param_bank: Arc<AtomicParamBank<MAX_PLUGIN_PARAMS>>,
    meter_bridge: Arc<AtomicMeterBridge>,
    daw_sync_cb: Option<DawSyncCallback>,
}

impl GenericWebViewEditor {
    /// Construye un nuevo editor WebView serverless con protocolo virtual `app://` y perfil aislado.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        vendor_name: &'static str,
        plugin_name: &'static str,
        width: u32,
        height: u32,
        html_document: String,
        param_bank: Arc<AtomicParamBank<MAX_PLUGIN_PARAMS>>,
        meter_bridge: Arc<AtomicMeterBridge>,
        daw_sync_cb: Option<DawSyncCallback>,
    ) -> Self {
        Self {
            vendor_name,
            plugin_name,
            width,
            height,
            html_document: Arc::new(html_document),
            param_bank,
            meter_bridge,
            daw_sync_cb,
        }
    }
}

impl Editor for GenericWebViewEditor {
    fn spawn(
        &self,
        parent: ParentWindowHandle,
        context: Arc<dyn GuiContext>,
    ) -> Box<dyn Any + Send> {
        let native_parent = NativeParentWindow(parent);
        #[cfg(windows)]
        let win_hwnd: win32_pump::HWND = match parent {
            ParentWindowHandle::Win32Hwnd(hwnd) => hwnd as win32_pump::HWND,
            _ => std::ptr::null_mut(),
        };

        let profile_dir = resolve_isolated_webview_profile_dir(self.vendor_name, self.plugin_name);
        let _ = std::fs::create_dir_all(&profile_dir);
        let mut web_context = wry::WebContext::new(Some(profile_dir));

        let html_doc = Arc::clone(&self.html_document);
        let bank_proto = Arc::clone(&self.param_bank);
        let meter_proto = Arc::clone(&self.meter_bridge);
        let ctx_proto = Arc::clone(&context);
        let sync_proto = self.daw_sync_cb.clone();

        let bank_ipc = Arc::clone(&self.param_bank);
        let ctx_ipc = Arc::clone(&context);
        let sync_ipc = self.daw_sync_cb.clone();

        let w = self.width;
        let h = self.height;

        let builder = WebViewBuilder::with_web_context(&mut web_context)
            .with_background_color((18, 19, 22, 255))
            .with_custom_protocol(
                DEFAULT_VIRTUAL_URI_SCHEME.into(),
                move |_webview_id, req: Request<Vec<u8>>| {
                    let path = req.uri().path().trim_start_matches('/');
                    let method = req.method().as_str();

                    if method == "OPTIONS" {
                        return Response::builder()
                            .status(StatusCode::NO_CONTENT)
                            .header("Access-Control-Allow-Origin", "*")
                            .header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
                            .header("Access-Control-Allow-Headers", "Content-Type")
                            .body(Cow::Borrowed(&[] as &'static [u8]))
                            .unwrap();
                    }

                    if path.ends_with("api/telemetry") {
                        let (pk_l, pk_r, gr_db) = meter_proto.read_snapshot();
                        let mut norm_params = [0.0_f32; 16];
                        for (idx, slot) in norm_params.iter_mut().enumerate() {
                            *slot = bank_proto.load_normalized(idx as u16);
                        }
                        let body = serde_json::json!({
                            "peak_l": pk_l,
                            "peak_r": pk_r,
                            "gr_db": gr_db,
                            "params": norm_params,
                        })
                        .to_string();
                        return Response::builder()
                            .status(StatusCode::OK)
                            .header(CONTENT_TYPE, "application/json; charset=utf-8")
                            .header("Cache-Control", "no-store, no-cache, must-revalidate")
                            .header("Access-Control-Allow-Origin", "*")
                            .body(Cow::Owned(body.into_bytes()))
                            .unwrap();
                    }
                    if path.ends_with("api/cmd") {
                        if let Ok(GuiToHostCommand::ParamSetValue { param_id, value }) =
                            dispatch_ipc_json(
                                &String::from_utf8_lossy(req.body()),
                                &bank_proto,
                                None,
                            )
                        {
                            if let Some(ref cb) = sync_proto {
                                cb(ctx_proto.as_ref(), param_id, value);
                            }
                        }
                        return Response::builder()
                            .status(StatusCode::OK)
                            .header(CONTENT_TYPE, "application/json; charset=utf-8")
                            .header("Cache-Control", "no-store, no-cache, must-revalidate")
                            .header("Access-Control-Allow-Origin", "*")
                            .body(Cow::Borrowed(b"{\"ok\":true}" as &[u8]))
                            .unwrap();
                    }
                    Response::builder()
                        .status(StatusCode::OK)
                        .header(CONTENT_TYPE, "text/html; charset=utf-8")
                        .header("Cache-Control", "no-store, no-cache, must-revalidate")
                        .header("Access-Control-Allow-Origin", "*")
                        .body(Cow::Owned(html_doc.as_bytes().to_vec()))
                        .unwrap()
                },
            )
            .with_devtools(true)
            .with_initialization_script(
                r#"
                if (typeof window.ipc === 'undefined' && window.chrome && window.chrome.webview) {
                    window.ipc = { postMessage: function(msg) { window.chrome.webview.postMessage(msg); } };
                }
                "#,
            )
            .with_ipc_handler(move |req: Request<String>| {
                if let Ok(GuiToHostCommand::ParamSetValue { param_id, value }) =
                    dispatch_ipc_json(req.body(), &bank_ipc, None)
                {
                    if let Some(ref cb) = sync_ipc {
                        cb(ctx_ipc.as_ref(), param_id, value);
                    }
                }
            })
            .with_bounds(Rect {
                position: LogicalPosition::new(0.0, 0.0).into(),
                size: LogicalSize::new(w as f64, h as f64).into(),
            })
            .with_on_page_load_handler(|event, url| {
                let ev_str = match event {
                    wry::PageLoadEvent::Started => "Started",
                    wry::PageLoadEvent::Finished => "Finished",
                };
                eprintln!("[BASE_PLUGIN_WEBVIEW LOAD] {} url: {}", ev_str, url);
            })
            .with_url(DEFAULT_VIRTUAL_INDEX_URL);

        match builder.build_as_child(&native_parent) {
            Ok(webview) => {
                let webview = std::rc::Rc::new(webview);
                #[cfg(windows)]
                if !win_hwnd.is_null() {
                    win32_pump::register_active_webview(win32_pump::ActiveResizeState {
                        hwnd: win_hwnd,
                        track_hwnd: unsafe { win32_pump::setup_resizable_window(win_hwnd, w, h) },
                        webview: std::rc::Rc::clone(&webview),
                        size_w: Arc::new(AtomicU32::new(w)),
                        size_h: Arc::new(AtomicU32::new(h)),
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

                let bounds = Rect {
                    position: LogicalPosition::new(0.0, 0.0).into(),
                    size: LogicalSize::new(w as f64, h as f64).into(),
                };
                let _ = webview.set_bounds(bounds);
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

                Box::new(ActiveWebViewHandle {
                    webview,
                    web_context,
                    #[cfg(windows)]
                    parent_hwnd: win_hwnd,
                })
            }
            Err(err) => {
                eprintln!("[BASE_PLUGIN_WEBVIEW] Error al crear WebView: {:?}", err);
                Box::new(())
            }
        }
    }

    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn set_scale_factor(&self, _factor: f32) -> bool {
        true
    }

    fn param_value_changed(&self, _id: &str, _normalized_value: f32) {}

    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {}

    fn param_values_changed(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_isolated_profile_dir_separates_vendors_and_plugins() {
        let dir_a = resolve_isolated_webview_profile_dir("StudioAlpha", "BusCompressor");
        let dir_b = resolve_isolated_webview_profile_dir("StudioAlpha", "ParametricEQ");
        assert_ne!(dir_a, dir_b);
        assert!(dir_a.to_string_lossy().contains("BusCompressor"));
        assert!(dir_b.to_string_lossy().contains("ParametricEQ"));
    }

    #[test]
    fn test_typed_ipc_command_roundtrip_and_bank_dispatch() {
        let bank = AtomicParamBank::<MAX_PLUGIN_PARAMS>::new(0.0);
        let msg = r#"{"cmd":"ParamSetValue","data":{"param_id":2,"value":0.75}}"#;
        let parsed = dispatch_ipc_json(msg, &bank, None).expect("valid IPC JSON");
        assert_eq!(
            parsed,
            GuiToHostCommand::ParamSetValue {
                param_id: 2,
                value: 0.75
            }
        );
        assert!((bank.load_normalized(2) - 0.75).abs() < 1e-6);
    }
}
