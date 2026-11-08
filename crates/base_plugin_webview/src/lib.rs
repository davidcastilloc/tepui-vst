//! # `base_plugin_webview` — Host WebView Agnóstico, Aislamiento Multi-Instancia y Bus IPC Tipado
//!
//! Provee el contrato de comandos IPC tipados (`GuiToHostCommand` y `HostToGuiEvent`),
//! el aislamiento dinámico de perfiles de usuario (`%LOCALAPPDATA%/AudioPlugins/<Vendor>/<PluginName>/WebViewProfile`),
//! las cabeceras de seguridad CSP para el protocolo virtual en memoria `app://localhost/`
//! y el bombeo de telemetría atómica a 60 FPS libre de contrapresión.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use base_plugin_core::{AtomicParamBank, ParamIndex};
use serde::{Deserialize, Serialize};

/// Esquema de URI virtual en memoria libre de puertos TCP locales.
pub const DEFAULT_VIRTUAL_URI_SCHEME: &str = "app";

/// URL raíz de carga del documento principal del plugin.
pub const DEFAULT_VIRTUAL_INDEX_URL: &str = "app://localhost/index.html";

/// Política de seguridad de contenido (CSP) endurecida para el contenedor WebView.
pub const HARDENED_CSP_HEADER: &str = "default-src 'self' app:; script-src 'self' 'unsafe-inline' app:; style-src 'self' 'unsafe-inline' app:; img-src 'self' data: app:; connect-src 'self' app:; object-src 'none'; frame-ancestors 'none';";

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

    /// Genera la expresión JavaScript compacta para evaluación directa a 60 FPS sin asignación HTTP.
    pub fn format_js_push_frame(&self) -> String {
        let l = f32::from_bits(self.peak_l.load(Ordering::Relaxed));
        let r = f32::from_bits(self.peak_r.load(Ordering::Relaxed));
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

#[cfg(test)]
mod tests {
    use super::*;
    use base_plugin_core::MAX_PLUGIN_PARAMS;

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
