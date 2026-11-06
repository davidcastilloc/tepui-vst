//! Gestión y enumeración de hardware de Audio y MIDI para el modo Standalone de TEPUI.
//!
//! Permite detectar tarjetas de sonido físicas y virtuales, puertos MIDI y persistir
//! la configuración del usuario (dispositivo, frecuencia de muestreo, buffer size).

use cpal::traits::{DeviceTrait, HostTrait};
use crossbeam_channel::Sender;
use midir::{MidiInput, MidiInputConnection};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::GuiMidiEvent;

static HARDWARE_MIDI_EVENTS: AtomicU32 = AtomicU32::new(0);

struct HardwareMidiRouterState {
    tx: Option<Sender<GuiMidiEvent>>,
    active_port_name: Option<String>,
    _conn: Option<MidiInputConnection<()>>,
}

static HARDWARE_MIDI_ROUTER: OnceLock<Mutex<HardwareMidiRouterState>> = OnceLock::new();

fn router_mutex() -> &'static Mutex<HardwareMidiRouterState> {
    HARDWARE_MIDI_ROUTER.get_or_init(|| {
        Mutex::new(HardwareMidiRouterState {
            tx: None,
            active_port_name: None,
            _conn: None,
        })
    })
}

/// Retorna la cantidad acumulada de eventos MIDI recibidos desde puertos USB/hardware en modo Standalone.
pub fn hardware_midi_events_count() -> u32 {
    HARDWARE_MIDI_EVENTS.load(Ordering::Relaxed)
}

/// Retorna el nombre del puerto MIDI USB/hardware conectado actualmente en vivo.
pub fn active_hardware_midi_port() -> Option<String> {
    router_mutex()
        .lock()
        .ok()
        .and_then(|s| s.active_port_name.clone())
}

/// Decodifica un paquete MIDI 1.0 estándar (2 o 3 bytes) proveniente de un controlador USB/hardware
/// hacia `GuiMidiEvent` sin asignaciones de memoria (`zero-allocation`).
pub fn parse_hardware_midi_packet(bytes: &[u8]) -> Option<GuiMidiEvent> {
    if bytes.is_empty() {
        return None;
    }
    let status = bytes[0] & 0xF0;
    match status {
        0x90 if bytes.len() >= 3 => {
            let note = bytes[1] & 0x7F;
            let vel = bytes[2] & 0x7F;
            if vel > 0 {
                let norm_vel = (vel as f32 / 127.0).clamp(0.01, 1.0);
                Some(GuiMidiEvent::NoteOn(note, norm_vel))
            } else {
                Some(GuiMidiEvent::NoteOff(note))
            }
        }
        0x80 if bytes.len() >= 2 => {
            let note = bytes[1] & 0x7F;
            Some(GuiMidiEvent::NoteOff(note))
        }
        0xA0 if bytes.len() >= 3 => {
            let note = bytes[1] & 0x7F;
            let pressure = ((bytes[2] & 0x7F) as f32 / 127.0).clamp(0.0, 1.0);
            Some(GuiMidiEvent::PolyAftertouch(note, pressure))
        }
        0xB0 if bytes.len() >= 3 => {
            let cc = bytes[1] & 0x7F;
            let raw_val = bytes[2] & 0x7F;
            let norm = (raw_val as f32 / 127.0).clamp(0.0, 1.0);
            match cc {
                1 => Some(GuiMidiEvent::ModWheel(norm)),
                11 | 74 => Some(GuiMidiEvent::BrightnessCC74(norm)),
                64 => Some(GuiMidiEvent::SustainPedal(raw_val >= 64)),
                120 | 123 => Some(GuiMidiEvent::AllNotesOff),
                _ => Some(GuiMidiEvent::MidiCC(cc, norm)),
            }
        }
        0xD0 if bytes.len() >= 2 => {
            let pressure = ((bytes[1] & 0x7F) as f32 / 127.0).clamp(0.0, 1.0);
            Some(GuiMidiEvent::Aftertouch(pressure))
        }
        0xE0 if bytes.len() >= 3 => {
            let lsb = (bytes[1] & 0x7F) as i32;
            let msb = (bytes[2] & 0x7F) as i32;
            let raw14 = (msb << 7) | lsb;
            let semitones = ((raw14 - 8192) as f32 / 8192.0 * 2.0).clamp(-2.0, 2.0);
            Some(GuiMidiEvent::PitchBend(semitones))
        }
        _ => None,
    }
}

/// Inicializa el enrutador MIDI USB en caliente (`Hot-Plug`) y conecta el puerto configurado
/// (o el primer controlador MIDI físico detectado en el sistema).
pub fn init_hardware_midi_router(gui_midi_tx: Sender<GuiMidiEvent>) {
    if std::env::var("CI").is_ok() || std::env::var("GITHUB_ACTIONS").is_ok() {
        return;
    }
    if let Ok(mut guard) = router_mutex().lock() {
        guard.tx = Some(gui_midi_tx);
    }
    let cfg = load_config();
    let _ = connect_hardware_midi_port(cfg.midi_input.as_deref());
}

/// Conecta en caliente (`Hot-Plug`) el puerto de entrada MIDI solicitado sin reiniciar la aplicación.
/// Si `preferred_port` es `None`, conecta automáticamente al primer puerto MIDI físico disponible.
pub fn connect_hardware_midi_port(preferred_port: Option<&str>) -> Option<String> {
    if std::env::var("CI").is_ok() || std::env::var("GITHUB_ACTIONS").is_ok() {
        return None;
    }
    let mut guard = router_mutex().lock().ok()?;
    let tx = guard.tx.clone()?;

    // Cerrar conexión previa si existía
    guard._conn = None;
    guard.active_port_name = None;

    let midi_in = MidiInput::new("tepui-live-midi-router").ok()?;
    let ports = midi_in.ports();
    if ports.is_empty() {
        return None;
    }

    let chosen_port = if let Some(pref) = preferred_port.filter(|s| !s.trim().is_empty()) {
        ports
            .iter()
            .find(|p| midi_in.port_name(p).map(|n| n == pref).unwrap_or(false))
            .or_else(|| ports.first())?
    } else {
        ports.first()?
    };

    let chosen_name = midi_in.port_name(chosen_port).ok()?;
    let conn_res = midi_in.connect(
        chosen_port,
        "tepui-usb-midi-in",
        move |_stamp_us, message, _| {
            if let Some(ev) = parse_hardware_midi_packet(message) {
                HARDWARE_MIDI_EVENTS.fetch_add(1, Ordering::Relaxed);
                let _ = tx.try_send(ev);
            }
        },
        (),
    );

    if let Ok(conn) = conn_res {
        println!(
            "[TEPUI USB MIDI] Controlador MIDI conectado en vivo: {}",
            chosen_name
        );
        guard.active_port_name = Some(chosen_name.clone());
        guard._conn = Some(conn);
        Some(chosen_name)
    } else {
        None
    }
}

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
    // 1. Intentar cargar de %LOCALAPPDATA%\Tepui\standalone_config.json (ruta canónica)
    let appdata = get_config_path();
    if appdata.exists() {
        if let Ok(content) = fs::read_to_string(&appdata) {
            if let Ok(mut cfg) = serde_json::from_str::<StandaloneAudioConfig>(&content) {
                if cfg.input_device.is_some() {
                    cfg.input_device = None;
                    let _ = save_config(&cfg);
                }
                return cfg;
            }
        }
    }
    // 2. Fallback: intentar cargar del directorio local
    let local = PathBuf::from("standalone_config.json");
    if local.exists() {
        if let Ok(content) = fs::read_to_string(&local) {
            if let Ok(mut cfg) = serde_json::from_str::<StandaloneAudioConfig>(&content) {
                if cfg.input_device.is_some() {
                    cfg.input_device = None;
                    let _ = save_config(&cfg);
                }
                return cfg;
            }
        }
    }
    StandaloneAudioConfig::default()
}

/// Guarda la configuración de audio seleccionada en disco garantizando sincronización total.
pub fn save_config(config: &StandaloneAudioConfig) -> std::io::Result<()> {
    let mut clean_cfg = config.clone();
    // Sintetizador puro (0 canales de entrada): nunca persistir dispositivo de entrada de audio
    clean_cfg.input_device = None;
    let json = serde_json::to_string_pretty(&clean_cfg)?;

    // 1. Guardar en %LOCALAPPDATA%\Tepui\standalone_config.json
    let appdata_path = get_config_path();
    if let Some(parent) = appdata_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&appdata_path, &json);

    // 2. Guardar también en el directorio local si ya existe
    let local_path = PathBuf::from("standalone_config.json");
    if local_path.exists() {
        let _ = fs::write(&local_path, &json);
    }

    println!(
        "[TEPUI CONFIG] Configuración guardada en disco exitosamente: {:?}",
        clean_cfg
    );
    Ok(())
}

static CACHED_AUDIO_STATUS: OnceLock<Mutex<Option<AudioSystemStatus>>> = OnceLock::new();

fn audio_status_cache() -> &'static Mutex<Option<AudioSystemStatus>> {
    CACHED_AUDIO_STATUS.get_or_init(|| Mutex::new(None))
}

/// Retorna el estado de audio y MIDI usando caché en memoria para no bloquear el hilo GUI
/// durante la carga inicial del HTML en WebView2; si no existe caché, la inicializa una sola vez.
pub fn enumerate_audio_system_cached(active_config: &StandaloneAudioConfig) -> AudioSystemStatus {
    if let Ok(guard) = audio_status_cache().lock() {
        if let Some(ref cached) = *guard {
            let mut copy = cached.clone();
            copy.is_standalone = is_standalone();
            copy.active_sample_rate = active_config.sample_rate;
            copy.active_period_size = active_config.period_size;
            if active_config.output_device.is_some() {
                copy.active_output_device = active_config.output_device.clone();
            }
            if active_config.input_device.is_some() {
                copy.active_input_device = active_config.input_device.clone();
            }
            if active_config.midi_input.is_some() {
                copy.active_midi_input = active_config.midi_input.clone();
            }
            return copy;
        }
    }
    enumerate_audio_system(active_config)
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
    // Sintetizador puro (main_input_channels: None): nunca activar un dispositivo de entrada por defecto
    let active_in: Option<String> = None;
    let active_midi = active_config
        .midi_input
        .clone()
        .or_else(active_hardware_midi_port)
        .or_else(|| midi_inputs.first().cloned());

    let status = AudioSystemStatus {
        is_standalone: standalone,
        active_output_device: active_out,
        active_input_device: active_in,
        active_sample_rate: active_config.sample_rate,
        active_period_size: active_config.period_size,
        active_midi_input: active_midi,
        output_devices,
        input_devices,
        midi_inputs,
    };
    if let Ok(mut guard) = audio_status_cache().lock() {
        *guard = Some(status.clone());
    }
    status
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

    #[cfg(windows)]
    {
        let script = format!(
            "Start-Sleep -Milliseconds 600; Set-Location -LiteralPath '{}'; Start-Process -FilePath '{}'",
            cwd.to_string_lossy().replace('\'', "''"),
            exe.to_string_lossy().replace('\'', "''")
        );

        std::process::Command::new("powershell")
            .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
            .spawn()
            .map_err(|e| format!("No se pudo programar reinicio: {}", e))?;
    }

    #[cfg(not(windows))]
    {
        std::process::Command::new(&exe)
            .current_dir(&cwd)
            .spawn()
            .map_err(|e| format!("No se pudo programar reinicio: {}", e))?;
    }

    // Salir de inmediato para liberar todos los handles de audio y la ventana nativa
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hardware_midi_packets_all_status_bytes() {
        // NoteOn Ch1 Note 60 Vel 100
        let ev1 = parse_hardware_midi_packet(&[0x90, 60, 100]).unwrap();
        match ev1 {
            GuiMidiEvent::NoteOn(n, v) => {
                assert_eq!(n, 60);
                assert!((v - (100.0 / 127.0)).abs() < 1e-4);
            }
            _ => panic!("Expected NoteOn"),
        }

        // NoteOn Vel 0 -> NoteOff
        assert_eq!(
            parse_hardware_midi_packet(&[0x92, 64, 0]),
            Some(GuiMidiEvent::NoteOff(64))
        );

        // Explicit NoteOff
        assert_eq!(
            parse_hardware_midi_packet(&[0x80, 67, 64]),
            Some(GuiMidiEvent::NoteOff(67))
        );

        // CC1 ModWheel
        if let Some(GuiMidiEvent::ModWheel(mw)) = parse_hardware_midi_packet(&[0xB0, 1, 127]) {
            assert!((mw - 1.0).abs() < 1e-4);
        } else {
            panic!("Expected ModWheel");
        }

        // CC64 Sustain Pedal
        assert_eq!(
            parse_hardware_midi_packet(&[0xB0, 64, 127]),
            Some(GuiMidiEvent::SustainPedal(true))
        );
        assert_eq!(
            parse_hardware_midi_packet(&[0xB0, 64, 0]),
            Some(GuiMidiEvent::SustainPedal(false))
        );

        // CC74 Filter Brightness
        if let Some(GuiMidiEvent::BrightnessCC74(b)) = parse_hardware_midi_packet(&[0xB0, 74, 64]) {
            assert!((b - (64.0 / 127.0)).abs() < 1e-3);
        } else {
            panic!("Expected BrightnessCC74");
        }

        // General CC (e.g. CC#21 for MIDI Learn / Hardware Knob)
        if let Some(GuiMidiEvent::MidiCC(cc, val)) = parse_hardware_midi_packet(&[0xB0, 21, 96]) {
            assert_eq!(cc, 21);
            assert!((val - (96.0 / 127.0)).abs() < 1e-3);
        } else {
            panic!("Expected MidiCC(21, ..)");
        }

        // Polyphonic Aftertouch (0xA0)
        if let Some(GuiMidiEvent::PolyAftertouch(note, pat)) =
            parse_hardware_midi_packet(&[0xA0, 60, 110])
        {
            assert_eq!(note, 60);
            assert!((pat - (110.0 / 127.0)).abs() < 1e-3);
        } else {
            panic!("Expected PolyAftertouch");
        }

        // Channel Pressure / Aftertouch
        if let Some(GuiMidiEvent::Aftertouch(at)) = parse_hardware_midi_packet(&[0xD0, 100]) {
            assert!((at - (100.0 / 127.0)).abs() < 1e-3);
        } else {
            panic!("Expected Aftertouch");
        }

        // PitchBend Center (8192 = LSB 0x00, MSB 0x40) -> 0.0 semitones
        if let Some(GuiMidiEvent::PitchBend(pb)) = parse_hardware_midi_packet(&[0xE0, 0x00, 0x40]) {
            assert!(pb.abs() < 1e-3);
        } else {
            panic!("Expected PitchBend");
        }
    }

    #[test]
    fn test_standalone_config_strips_input_device_to_prevent_wasapi_0ch_crash() {
        let dirty_cfg = StandaloneAudioConfig {
            output_device: None,
            input_device: Some("CABLE Output (VB-Audio Virtual Cable)".to_string()),
            sample_rate: 48000,
            period_size: 1056,
            midi_input: None,
        };
        save_config(&dirty_cfg).expect("save_config should succeed");
        let loaded = load_config();
        assert!(
            loaded.input_device.is_none(),
            "load_config/save_config must strip input_device so 0-input WASAPI never crashes"
        );
    }
}
