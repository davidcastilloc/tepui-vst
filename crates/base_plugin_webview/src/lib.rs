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

    base.join("AudioPlugins")
        .join(sanitize(vendor_name))
        .join(sanitize(plugin_name))
        .join("WebViewProfile")
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

struct ActiveWebViewHandle {
    #[allow(dead_code)]
    webview: wry::WebView,
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
                    if path == "api/telemetry" {
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
                            .header("Access-Control-Allow-Origin", "*")
                            .body(Cow::Owned(body.into_bytes()))
                            .unwrap();
                    }
                    if path == "api/cmd" {
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
                            .header("Access-Control-Allow-Origin", "*")
                            .body(Cow::Borrowed(b"{\"ok\":true}" as &[u8]))
                            .unwrap();
                    }
                    Response::builder()
                        .status(StatusCode::OK)
                        .header(CONTENT_TYPE, "text/html; charset=utf-8")
                        .header("Content-Security-Policy", HARDENED_CSP_HEADER)
                        .header("X-Content-Type-Options", "nosniff")
                        .body(Cow::Owned(html_doc.as_bytes().to_vec()))
                        .unwrap()
                },
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
            .with_url(DEFAULT_VIRTUAL_INDEX_URL);

        match builder.build_as_child(&native_parent) {
            Ok(webview) => {
                let _ = webview.focus();
                Box::new(ActiveWebViewHandle { webview })
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
