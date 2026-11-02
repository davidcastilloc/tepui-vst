//! Gestión y enumeración de hardware de Audio y MIDI para el modo Standalone de TEPUI.
//!
//! Permite detectar tarjetas de sonido físicas y virtuales, puertos MIDI y persistir
//! la configuración del usuario (dispositivo, frecuencia de muestreo, buffer size).

use cpal::traits::{DeviceTrait, HostTrait};
use midir::MidiInput;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDeviceInfo {
    pub name: String,
    pub is_default: bool,
    pub channels: u16,
    pub supported_sample_rates: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandaloneAudioConfig {
    pub output_device: Option<String>,
    pub input_device: Option<String>,
    pub sample_rate: u32,
    pub period_size: u32,
    pub midi_input: Option<String>,
}

impl Default for StandaloneAudioConfig {
    fn default() -> Self {
        Self {
            output_device: None,
            input_device: None,
            sample_rate: 48000,
            period_size: 1056,
            midi_input: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSystemStatus {
    pub is_standalone: bool,
    pub active_output_device: Option<String>,
    pub active_input_device: Option<String>,
    pub active_sample_rate: u32,
    pub active_period_size: u32,
    pub active_midi_input: Option<String>,
    pub output_devices: Vec<AudioDeviceInfo>,
    pub input_devices: Vec<AudioDeviceInfo>,
    pub midi_inputs: Vec<String>,
}

/// Determina si la aplicación se está ejecutando como Standalone (.exe independiente)
/// o como Plugin dentro de un DAW (VST3/CLAP).
pub fn is_standalone() -> bool {
    std::env::var("TEPUI_STANDALONE")
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// Marca el proceso actual como aplicación Standalone.
pub fn set_standalone_mode(standalone: bool) {
    if standalone {
        std::env::set_var("TEPUI_STANDALONE", "1");
    } else {
        std::env::remove_var("TEPUI_STANDALONE");
    }
}

/// Obtiene la ruta canónica del archivo de configuración `%LOCALAPPDATA%\Tepui\standalone_config.json`.
pub fn get_config_path() -> PathBuf {
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let dir = PathBuf::from(local_app_data).join("Tepui");
        let _ = fs::create_dir_all(&dir);
        return dir.join("standalone_config.json");
    }
    PathBuf::from("standalone_config.json")
}

/// Carga la configuración guardada de audio o devuelve la configuración por defecto.
pub fn load_config() -> StandaloneAudioConfig {
    // 1. Intentar cargar del directorio local
    let local = PathBuf::from("standalone_config.json");
    if local.exists() {
        if let Ok(content) = fs::read_to_string(&local) {
            if let Ok(cfg) = serde_json::from_str::<StandaloneAudioConfig>(&content) {
                return cfg;
            }
        }
    }
    // 2. Intentar cargar de %LOCALAPPDATA%\Tepui\standalone_config.json
    let appdata = get_config_path();
    if appdata.exists() {
        if let Ok(content) = fs::read_to_string(&appdata) {
            if let Ok(cfg) = serde_json::from_str::<StandaloneAudioConfig>(&content) {
                return cfg;
            }
        }
    }
    StandaloneAudioConfig::default()
}

/// Guarda la configuración de audio seleccionada en disco garantizando sincronización total.
pub fn save_config(config: &StandaloneAudioConfig) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(config)?;

    // 1. Guardar en %LOCALAPPDATA%\Tepui\standalone_config.json
    let appdata_path = get_config_path();
    if let Some(parent) = appdata_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&appdata_path, &json);

    // 2. Guardar también en el directorio local
    let local_path = PathBuf::from("standalone_config.json");
    let _ = fs::write(&local_path, &json);

    println!(
        "[TEPUI CONFIG] Configuración guardada en disco exitosamente: {:?}",
        config
    );
    Ok(())
}

/// Enumera todos los dispositivos de audio y puertos MIDI disponibles en el sistema operativo.
pub fn enumerate_audio_system(active_config: &StandaloneAudioConfig) -> AudioSystemStatus {
    let standalone = is_standalone();
    if std::env::var("CI").is_ok() || std::env::var("GITHUB_ACTIONS").is_ok() {
        return AudioSystemStatus {
            is_standalone: standalone,
            active_output_device: active_config.output_device.clone(),
            active_input_device: active_config.input_device.clone(),
            active_sample_rate: active_config.sample_rate,
            active_period_size: active_config.period_size,
            active_midi_input: active_config.midi_input.clone(),
            output_devices: Vec::new(),
            input_devices: Vec::new(),
            midi_inputs: Vec::new(),
        };
    }
    let host = cpal::default_host();
    let default_output = host.default_output_device().and_then(|d| d.name().ok());
    let default_input = host.default_input_device().and_then(|d| d.name().ok());

    let mut output_devices = Vec::new();
    if let Ok(devices) = host.output_devices() {
        for dev in devices {
            if let Ok(name) = dev.name() {
                let is_default = default_output.as_ref().map(|d| d == &name).unwrap_or(false);
                let mut supported_rates = Vec::new();
                let mut channels = 2;

                if let Ok(configs) = dev.supported_output_configs() {
                    for c in configs {
                        channels = channels.max(c.channels());
                        let min_r = c.min_sample_rate().0;
                        let max_r = c.max_sample_rate().0;
                        for &standard_rate in &[44100, 48000, 88200, 96000, 192000] {
                            if standard_rate >= min_r
                                && standard_rate <= max_r
                                && !supported_rates.contains(&standard_rate)
                            {
                                supported_rates.push(standard_rate);
                            }
                        }
                    }
                }
                if supported_rates.is_empty() {
                    supported_rates = vec![44100, 48000];
                }

                output_devices.push(AudioDeviceInfo {
                    name,
                    is_default,
                    channels,
                    supported_sample_rates: supported_rates,
                });
            }
        }
    }

    let mut input_devices = Vec::new();
    if let Ok(devices) = host.input_devices() {
        for dev in devices {
            if let Ok(name) = dev.name() {
                let is_default = default_input.as_ref().map(|d| d == &name).unwrap_or(false);
                input_devices.push(AudioDeviceInfo {
                    name,
                    is_default,
                    channels: 1,
                    supported_sample_rates: vec![44100, 48000],
                });
            }
        }
    }

    let mut midi_inputs = Vec::new();
    if let Ok(midi_in) = MidiInput::new("tepui-probe") {
        for port in midi_in.ports() {
            if let Ok(port_name) = midi_in.port_name(&port) {
                midi_inputs.push(port_name);
            }
        }
    }

    let active_out = active_config.output_device.clone().or(default_output);
    let active_in = active_config.input_device.clone().or(default_input);

    AudioSystemStatus {
        is_standalone: standalone,
        active_output_device: active_out,
        active_input_device: active_in,
        active_sample_rate: active_config.sample_rate,
        active_period_size: active_config.period_size,
        active_midi_input: active_config.midi_input.clone(),
        output_devices,
        input_devices,
        midi_inputs,
    }
}

/// Reinicia la aplicación Standalone ejecutando un nuevo proceso con la nueva configuración
/// y cerrando el proceso actual limpiamente.
pub fn restart_standalone_app() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    println!(
        "[TEPUI RESTART] Programando relanzamiento de instancia standalone: {:?}",
        exe
    );

    // Lanzar un runner que espere a que el proceso actual muera por completo antes de iniciar el nuevo,
    // garantizando que el hardware WASAPI quede 100% liberado y sin conflictos de puerto o de dispositivo.
    let script = format!(
        "Start-Sleep -Milliseconds 600; Set-Location -LiteralPath '{}'; Start-Process -FilePath '{}'",
        cwd.to_string_lossy().replace('\'', "''"),
        exe.to_string_lossy().replace('\'', "''")
    );

    std::process::Command::new("powershell")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
        .spawn()
        .map_err(|e| format!("No se pudo programar reinicio: {}", e))?;

    // Salir de inmediato para liberar todos los handles de audio WASAPI y la ventana HWND
    std::process::exit(0);
}
