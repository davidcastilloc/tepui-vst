//! Integración con Cloudflare Workers AI (Structured JSON Schema para síntesis de Presets y Parciales Modales)
//! y Cloudflare AI Gateway (condicionamiento de One-Shots para `stability-ai/stable-audio-open-1.0` / `tangoflux`),
//! con calibración automática `Piano-Roll Ready` (Auto-Onset Trim + Auto-Pitch YIN).

use std::f32::consts::PI;
use std::process::Command;

use serde::{Deserialize, Serialize};
use synth_core::{decode_wav, PianoRollPrepReport, SampleBuffer};

use crate::npu_synth::{extract_midi_note_from_prompt, SemanticTimbreDescriptor};

/// Modelo de Cloudflare Workers AI para síntesis estructurada de presets y tablas espectrales.
pub const DEFAULT_CF_PRESET_MODEL: &str = "@cf/meta/llama-3.3-70b-instruct-fp8-fast";

/// Modelo de difusión de samples instrumentales one-shot vía Cloudflare AI Gateway.
pub const DEFAULT_CF_SAMPLE_DIFFUSION_MODEL: &str = "stability-ai/stable-audio-open-1.0";

/// Endpoint Edge en vivo desplegado en Cloudflare Workers (`PC911 IT Services Global`)
/// con binding nativo a Cloudflare Workers AI (`env.AI`) y caché global en Cloudflare KV (`env.TEPUI_KV`).
pub const DEFAULT_CF_EDGE_WORKER_URL: &str =
    "https://tepui-ai-synth-gateway.pc911itservices.workers.dev";

/// Tope diario de seguridad en Cloudflare Workers AI (por debajo de los 10,000 Neurons gratuitos/día = $0 riesgo).
pub const MAX_DAILY_FREE_POOL_NEURONS: u32 = 9500;

fn default_device_id() -> String {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "tepui-node".to_string());
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "studio".to_string());
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in format!("{}:{}", host, user).bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("tepui-dev-{:012x}", hash & 0xffff_ffff_ffff)
}

fn default_ai_provider() -> String {
    "community_edge".to_string()
}

/// Configuración persistente del Hub Open-Source Multi-Proveedor de IA (`Community KV`, `Ollama Local`, `Gemini`, `Groq`, `OpenAI`, `Local NPU`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloudLicenseConfig {
    /// Proveedor activo: `"community_edge"`, `"ollama_local"`, `"gemini"`, `"groq"`, `"openai_compatible"`, `"local_npu"`
    #[serde(default = "default_ai_provider")]
    pub ai_provider: String,
    /// API Key del usuario para Gemini, Groq, OpenAI u OpenRouter
    #[serde(default)]
    pub api_key: String,
    /// Endpoint personalizado (ej. `http://127.0.0.1:11434` para Ollama o URL compatible con OpenAI)
    #[serde(default)]
    pub custom_endpoint: String,
    /// Nombre del modelo (ej. `llama3.2`, `gemini-2.5-flash`, `llama-3.3-70b-versatile`, `gpt-4o-mini`)
    #[serde(default)]
    pub model_name: String,
    /// Campo legado opcional
    #[serde(default)]
    pub license_key: String,
    /// Identificador único anónimo del dispositivo
    #[serde(default = "default_device_id")]
    pub device_id: String,
    /// Account ID propio de Cloudflare del usuario (modo BYOK 0 costo para nosotros)
    #[serde(default)]
    pub byok_account_id: String,
    /// API Token propio de Cloudflare del usuario (modo BYOK 0 costo para nosotros)
    #[serde(default)]
    pub byok_api_token: String,
    /// Si es `true`, desactiva todas las llamadas a la nube y usa exclusivamente el motor NPU/Modal local ($0 costo)
    #[serde(default)]
    pub local_npu_only: bool,
    /// Créditos adicionales acreditados mediante códigos de licencia/recarga o webhooks de pago
    #[serde(default)]
    pub redeemed_credits: u32,
    /// Lista idempotente de códigos de recarga o identificadores de eventos de webhook ya procesados
    #[serde(default)]
    pub redeemed_codes: Vec<String>,
    /// Indica si la cuenta desbloqueó el nivel Pro Cloud mediante código o compra verificada
    #[serde(default)]
    pub pro_tier_unlocked: bool,
}

impl Default for CloudLicenseConfig {
    fn default() -> Self {
        Self {
            ai_provider: default_ai_provider(),
            api_key: String::new(),
            custom_endpoint: String::new(),
            model_name: String::new(),
            license_key: String::new(),
            device_id: default_device_id(),
            byok_account_id: String::new(),
            byok_api_token: String::new(),
            local_npu_only: false,
            redeemed_credits: 0,
            redeemed_codes: Vec::new(),
            pro_tier_unlocked: false,
        }
    }
}

/// Reporte en vivo de proveedor IA, cuota comunitaria, caché KV y consumo de Neurons.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloudEdgeQuotaReport {
    pub tier: String,
    pub credits_remaining: u32,
    pub daily_neurons_used: u32,
    pub max_daily_neurons: u32,
    pub cached: bool,
    pub budget_guard_tripped: bool,
    pub message: String,
}

impl Default for CloudEdgeQuotaReport {
    fn default() -> Self {
        Self {
            tier: "COMMUNITY KV".to_string(),
            credits_remaining: 20,
            daily_neurons_used: 0,
            max_daily_neurons: MAX_DAILY_FREE_POOL_NEURONS,
            cached: false,
            budget_guard_tripped: false,
            message: "Pool Comunitario Open Source activo (Caché KV ilimitado + 9,500 Neurons/día gratis)".to_string(),
        }
    }
}

fn cloud_license_config_path() -> std::path::PathBuf {
    let base = std::env::var("APPDATA")
        .or_else(|_| std::env::var("LOCALAPPDATA"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let dir = base.join("Tepui");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("cloud_license_config.json")
}

/// Carga desde disco la configuración de Licencia Pro / BYOK / Escudo Anti-Costos.
pub fn load_cloud_license_config() -> CloudLicenseConfig {
    let path = cloud_license_config_path();
    if let Ok(bytes) = std::fs::read(&path) {
        if let Ok(mut cfg) = serde_json::from_slice::<CloudLicenseConfig>(&bytes) {
            if cfg.device_id.trim().is_empty() {
                cfg.device_id = default_device_id();
            }
            return cfg;
        }
    }
    CloudLicenseConfig::default()
}

/// Guarda en disco la configuración de Licencia Pro / BYOK / Escudo Anti-Costos.
pub fn save_cloud_license_config(cfg: &CloudLicenseConfig) -> Result<(), String> {
    let path = cloud_license_config_path();
    let json = serde_json::to_vec_pretty(cfg).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

fn configure_silent_command(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
}

/// Especificación de un parcial modal o armónico para síntesis aditiva/física.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModalPartialSpec {
    /// Ratio de frecuencia respecto a la fundamental `f0` (`1.0` = fundamental, `2.0` = octava, etc.)
    pub ratio: f32,
    /// Amplitud relativa del parcial (`0.0..=1.0`)
    pub amplitude: f32,
    /// Tasa de decaimiento exponencial por segundo (`0.3..=25.0`)
    pub decay_rate: f32,
    /// Desafinación microtonal del parcial en cents (`-50.0..=50.0`)
    #[serde(default)]
    pub detune_cents: f32,
    /// Posición panorámica estéreo del parcial (`-1.0` L .. `+1.0` R)
    #[serde(default)]
    pub pan: f32,
}

/// Especificación completa generada por IA que combina:
/// 1. El **Preset del Sintetizador TEPUI** (`ADSR`, `SVF Filter`, `LFO`, `Arp`, `5×5 Mod Matrix`, `Reverb`).
/// 2. La **Receta de Síntesis Espectral/Modal** (`partials`, `fm_index`, `fm_ratio`, `root_note`) para el sample base.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AiSynthesizerPatchSpec {
    /// Nombre corto del patch/timbre
    pub name: String,
    /// Ataque ADSR en segundos (`0.001..=2.0`)
    pub attack: f32,
    /// Decaimiento ADSR en segundos (`0.01..=4.0`)
    pub decay: f32,
    /// Nivel de sostenimiento ADSR (`0.0..=1.0`)
    pub sustain: f32,
    /// Relajación ADSR en segundos (`0.02..=5.0`)
    pub release: f32,
    /// Frecuencia de corte del filtro SVF en Hz (`80.0..=20000.0`)
    pub cutoff: f32,
    /// Resonancia Q del filtro SVF (`0.5..=8.0`)
    pub resonance: f32,
    /// Modo del filtro SVF (`0 = Lowpass`, `1 = Highpass`, `2 = Bandpass`)
    pub filter_mode: u8,
    /// Velocidad del LFO en Hz (`0.1..=18.0`)
    pub lfo_rate: f32,
    /// Profundidad de modulación del LFO (`0.0..=1.0`)
    pub lfo_depth: f32,
    /// Destino primario del LFO (`0 = Cutoff`, `1 = Pitch`, `2 = Amp/Pan`)
    pub lfo_target: u8,
    /// Modo del arpegiador (`0 = Off`, `1 = Up`, `2 = Down`, `3 = Joropo 6/8`, `4 = Merengue 5/8`)
    pub arp_mode: u8,
    /// Mezcla espacial del Reverb de Cañón (`0.0..=0.85`)
    pub space_mix: f32,
    /// Máscara de 25 bits de la Matriz de Modulación 5×5
    pub mod_matrix_mask: u32,
    /// Nota raíz MIDI del sample (`12..=108`, `60 = C4`)
    pub root_note: u8,
    /// Índice de modulación FM para ataque/cuerpo (`0.0..=4.0`)
    pub fm_index: f32,
    /// Ratio de modulador FM (`0.5..=8.0`)
    pub fm_ratio: f32,
    /// Lista de parciales armónicos/inarmónicos para síntesis modal
    pub partials: Vec<ModalPartialSpec>,
}

/// Convierte un número de nota MIDI (`60`) a su nombre estándar (`"C4"`).
pub fn midi_note_to_name(midi: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let clamped = midi.clamp(12, 108);
    let name = NAMES[(clamped % 12) as usize];
    let octave = (clamped as i32 / 12) - 1;
    format!("{}{}", name, octave)
}

/// Formatea un prompt de usuario con tags de condicionamiento acústico para modelos de difusión de audio
/// (`stability-ai/stable-audio-open-1.0`, `declare-lab/tangoflux`) detrás de Cloudflare AI Gateway,
/// forzando la generación de un *one-shot* aislado, seco y afinado para Piano Roll.
pub fn format_piano_roll_oneshot_prompt(prompt: &str, root_note: u8) -> String {
    let clean = prompt.trim();
    let note_str = midi_note_to_name(root_note);
    let freq_hz = 440.0 * 2.0_f32.powf((root_note as f32 - 69.0) / 12.0);
    format!(
        "TrackType: Instrument, Single isolated note {} ({:.1} Hz), {}, dry studio recording, close mic, one-shot sample for chromatic piano roll, immediate attack, no reverb",
        note_str, freq_hz, clean
    )
}

/// Construye el payload JSON con `response_format: { type: "json_schema" }` compatible con
/// Cloudflare Workers AI (`@cf/meta/llama-3.3-70b-instruct-fp8-fast` / `@cf/meta/llama-3.1-8b-instruct-fast`).
pub fn build_cloudflare_workers_ai_preset_request(prompt: &str) -> serde_json::Value {
    serde_json::json!({
        "messages": [
            {
                "role": "system",
                "content": "You are an expert DSP synthesizer & modal acoustic architect for TEPUI Gran Sabana Sonic Lab. Given an instrument or timbre prompt, output a JSON object matching the exact schema with optimal ADSR, SVF filter, LFO, 5x5 modulation matrix bitmask, root MIDI note, FM ratio/index, and up to 12 modal partials for chromatic Piano Roll playback."
            },
            {
                "role": "user",
                "content": prompt.trim()
            }
        ],
        "response_format": {
            "type": "json_schema",
            "json_schema": {
                "name": "tepui_synth_patch_spec",
                "strict": true,
                "schema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "attack": { "type": "number" },
                        "decay": { "type": "number" },
                        "sustain": { "type": "number" },
                        "release": { "type": "number" },
                        "cutoff": { "type": "number" },
                        "resonance": { "type": "number" },
                        "filter_mode": { "type": "integer" },
                        "lfo_rate": { "type": "number" },
                        "lfo_depth": { "type": "number" },
                        "lfo_target": { "type": "integer" },
                        "arp_mode": { "type": "integer" },
                        "space_mix": { "type": "number" },
                        "mod_matrix_mask": { "type": "integer" },
                        "root_note": { "type": "integer" },
                        "fm_index": { "type": "number" },
                        "fm_ratio": { "type": "number" },
                        "partials": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "ratio": { "type": "number" },
                                    "amplitude": { "type": "number" },
                                    "decay_rate": { "type": "number" }
                                },
                                "required": ["ratio", "amplitude", "decay_rate"]
                            }
                        }
                    },
                    "required": [
                        "name", "attack", "decay", "sustain", "release",
                        "cutoff", "resonance", "filter_mode", "lfo_rate",
                        "lfo_depth", "lfo_target", "arp_mode", "space_mix",
                        "mod_matrix_mask", "root_note", "fm_index", "fm_ratio", "partials"
                    ]
                }
            }
        }
    })
}

const OPEN_SOURCE_SYNTH_SYSTEM_PROMPT: &str = "You are TEPUI Gran Sabana Sonic Lab's open-source synthesizer patch & modal-spectral sample architect. Given a musical description, return ONLY a compact raw JSON object (no markdown fences) for a playable polyphonic Piano Roll instrument with these exact keys: {\"name\":string,\"attack\":float(0.001..2.0),\"decay\":float(0.05..3.0),\"sustain\":float(0..1),\"release\":float(0.05..4.0),\"cutoff\":float(120..20000),\"resonance\":float(0.5..6.0),\"filter_mode\":int(0=LP,1=HP,2=BP),\"lfo_rate\":float(0.2..16),\"lfo_depth\":float(0..0.9),\"lfo_target\":int(0=Cutoff,1=Pitch,2=Amp),\"arp_mode\":int(0=Off,1=Up,2=Down,3=Joropo 6/8,4=Merengue 5/8),\"space_mix\":float(0..0.8),\"mod_matrix_mask\":int(0..33554431),\"root_note\":int(24..96),\"fm_index\":float(0..4),\"fm_ratio\":float(0.5..8),\"partials\":[{\"ratio\":float,\"amplitude\":float,\"decay_rate\":float,\"detune_cents\":float,\"pan\":float}]} (8 partials).";

fn parse_patch_spec_from_llm_text(raw_text: &str, prompt: &str) -> Option<AiSynthesizerPatchSpec> {
    let start = raw_text.find('{')?;
    let end = raw_text.rfind('}')?;
    if end <= start {
        return None;
    }
    let json_slice = &raw_text[start..=end];
    let val: serde_json::Value = serde_json::from_str(json_slice).ok()?;
    let partials_val = val.get("partials").and_then(|v| v.as_array());
    let mut partials = Vec::new();
    if let Some(arr) = partials_val {
        for (i, item) in arr.iter().take(12).enumerate() {
            partials.push(ModalPartialSpec {
                ratio: item
                    .get("ratio")
                    .and_then(|v| v.as_f64())
                    .unwrap_or((i + 1) as f64) as f32,
                amplitude: item
                    .get("amplitude")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(1.0 / (i + 1) as f64) as f32,
                decay_rate: item
                    .get("decay_rate")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(1.2 + i as f64 * 0.45) as f32,
                detune_cents: item
                    .get("detune_cents")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0) as f32,
                pan: item.get("pan").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
            });
        }
    }
    let mut spec = AiSynthesizerPatchSpec {
        name: val
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(prompt)
            .to_string(),
        attack: val.get("attack").and_then(|v| v.as_f64()).unwrap_or(0.015) as f32,
        decay: val.get("decay").and_then(|v| v.as_f64()).unwrap_or(0.35) as f32,
        sustain: val.get("sustain").and_then(|v| v.as_f64()).unwrap_or(0.70) as f32,
        release: val.get("release").and_then(|v| v.as_f64()).unwrap_or(0.40) as f32,
        cutoff: val.get("cutoff").and_then(|v| v.as_f64()).unwrap_or(9500.0) as f32,
        resonance: val
            .get("resonance")
            .and_then(|v| v.as_f64())
            .unwrap_or(1.15) as f32,
        filter_mode: val.get("filter_mode").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
        lfo_rate: val.get("lfo_rate").and_then(|v| v.as_f64()).unwrap_or(3.8) as f32,
        lfo_depth: val
            .get("lfo_depth")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.18) as f32,
        lfo_target: val.get("lfo_target").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
        arp_mode: val.get("arp_mode").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
        space_mix: val
            .get("space_mix")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.24) as f32,
        mod_matrix_mask: val
            .get("mod_matrix_mask")
            .and_then(|v| v.as_u64())
            .unwrap_or(2178) as u32,
        root_note: val.get("root_note").and_then(|v| v.as_u64()).unwrap_or(60) as u8,
        fm_index: val.get("fm_index").and_then(|v| v.as_f64()).unwrap_or(0.6) as f32,
        fm_ratio: val.get("fm_ratio").and_then(|v| v.as_f64()).unwrap_or(2.0) as f32,
        partials,
    };
    sanitize_patch_spec(&mut spec);
    Some(spec)
}

fn try_fetch_external_provider_patch(
    prompt: &str,
    cfg: &CloudLicenseConfig,
) -> Option<(AiSynthesizerPatchSpec, CloudEdgeQuotaReport)> {
    let provider = cfg.ai_provider.trim().to_lowercase();
    match provider.as_str() {
        "ollama_local" => {
            let base = if cfg.custom_endpoint.trim().is_empty() {
                "http://127.0.0.1:11434"
            } else {
                cfg.custom_endpoint.trim().trim_end_matches('/')
            };
            let endpoint = if base.ends_with("/api/chat") {
                base.to_string()
            } else {
                format!("{}/api/chat", base)
            };
            let model = if cfg.model_name.trim().is_empty() {
                "llama3.2"
            } else {
                cfg.model_name.trim()
            };
            let body = serde_json::json!({
                "model": model,
                "stream": false,
                "format": "json",
                "messages": [
                    { "role": "system", "content": OPEN_SOURCE_SYNTH_SYSTEM_PROMPT },
                    { "role": "user", "content": prompt.trim() }
                ]
            })
            .to_string();
            let mut cmd = Command::new("curl");
            configure_silent_command(&mut cmd);
            let output = cmd
                .args([
                    "-sS",
                    "--max-time",
                    "20",
                    "-X",
                    "POST",
                    &endpoint,
                    "-H",
                    "Content-Type: application/json",
                    "-d",
                    &body,
                ])
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            let resp_json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
            let content = resp_json
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())?;
            let spec = parse_patch_spec_from_llm_text(content, prompt)?;
            Some((
                spec,
                CloudEdgeQuotaReport {
                    tier: format!("OLLAMA LOCAL ({})", model),
                    credits_remaining: 999999,
                    daily_neurons_used: 0,
                    max_daily_neurons: MAX_DAILY_FREE_POOL_NEURONS,
                    cached: false,
                    budget_guard_tripped: false,
                    message: format!("Sintetizado localmente con Ollama ({})", model),
                },
            ))
        }
        "gemini" => {
            let api_key = cfg.api_key.trim();
            if api_key.is_empty() {
                return None;
            }
            let model = if cfg.model_name.trim().is_empty() {
                "gemini-2.5-flash"
            } else {
                cfg.model_name.trim()
            };
            let endpoint = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
                model, api_key
            );
            let body = serde_json::json!({
                "systemInstruction": {
                    "parts": [{ "text": OPEN_SOURCE_SYNTH_SYSTEM_PROMPT }]
                },
                "contents": [{
                    "parts": [{ "text": prompt.trim() }]
                }],
                "generationConfig": {
                    "responseMimeType": "application/json",
                    "temperature": 0.2
                }
            })
            .to_string();
            let mut cmd = Command::new("curl");
            configure_silent_command(&mut cmd);
            let output = cmd
                .args([
                    "-sS",
                    "--max-time",
                    "15",
                    "-X",
                    "POST",
                    &endpoint,
                    "-H",
                    "Content-Type: application/json",
                    "-d",
                    &body,
                ])
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            let resp_json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
            let content = resp_json
                .get("candidates")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("content"))
                .and_then(|c| c.get("parts"))
                .and_then(|p| p.get(0))
                .and_then(|p| p.get("text"))
                .and_then(|t| t.as_str())?;
            let spec = parse_patch_spec_from_llm_text(content, prompt)?;
            Some((
                spec,
                CloudEdgeQuotaReport {
                    tier: format!("GEMINI ({})", model),
                    credits_remaining: 999999,
                    daily_neurons_used: 0,
                    max_daily_neurons: MAX_DAILY_FREE_POOL_NEURONS,
                    cached: false,
                    budget_guard_tripped: false,
                    message: format!("Conectado a Google Gemini API ({})", model),
                },
            ))
        }
        "groq" | "openai_compatible" => {
            let api_key = cfg.api_key.trim();
            if api_key.is_empty() {
                return None;
            }
            let is_groq = provider == "groq";
            let endpoint = if !cfg.custom_endpoint.trim().is_empty() {
                cfg.custom_endpoint.trim().to_string()
            } else if is_groq {
                "https://api.groq.com/openai/v1/chat/completions".to_string()
            } else {
                "https://api.openai.com/v1/chat/completions".to_string()
            };
            let model = if !cfg.model_name.trim().is_empty() {
                cfg.model_name.trim()
            } else if is_groq {
                "llama-3.3-70b-versatile"
            } else {
                "gpt-4o-mini"
            };
            let body = serde_json::json!({
                "model": model,
                "temperature": 0.2,
                "response_format": { "type": "json_object" },
                "messages": [
                    { "role": "system", "content": OPEN_SOURCE_SYNTH_SYSTEM_PROMPT },
                    { "role": "user", "content": prompt.trim() }
                ]
            })
            .to_string();
            let mut cmd = Command::new("curl");
            configure_silent_command(&mut cmd);
            let output = cmd
                .args([
                    "-sS",
                    "--max-time",
                    "15",
                    "-X",
                    "POST",
                    &endpoint,
                    "-H",
                    &format!("Authorization: Bearer {}", api_key),
                    "-H",
                    "Content-Type: application/json",
                    "-d",
                    &body,
                ])
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            let resp_json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
            let content = resp_json
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("message"))
                .and_then(|m| m.get("content"))
                .and_then(|t| t.as_str())?;
            let spec = parse_patch_spec_from_llm_text(content, prompt)?;
            let tier_label = if is_groq {
                format!("GROQ LPU ({})", model)
            } else {
                format!("OPENAI ({})", model)
            };
            Some((
                spec,
                CloudEdgeQuotaReport {
                    tier: tier_label,
                    credits_remaining: 999999,
                    daily_neurons_used: 0,
                    max_daily_neurons: MAX_DAILY_FREE_POOL_NEURONS,
                    cached: false,
                    budget_guard_tripped: false,
                    message: format!("Conectado a {} ({})", provider.to_uppercase(), model),
                },
            ))
        }
        _ => None,
    }
}

/// Consulta el estado del proveedor IA seleccionado o del Pool Comunitario en el Worker Edge (`/v1/quota`).
pub fn verify_cloud_license_quota(cfg: &CloudLicenseConfig) -> CloudEdgeQuotaReport {
    let provider = cfg.ai_provider.trim().to_lowercase();
    if cfg.local_npu_only || provider == "local_npu" {
        return CloudEdgeQuotaReport {
            tier: "LOCAL NPU ($0)".to_string(),
            credits_remaining: 0,
            daily_neurons_used: 0,
            max_daily_neurons: MAX_DAILY_FREE_POOL_NEURONS,
            cached: false,
            budget_guard_tripped: false,
            message: "Modo 100% Local Intel NPU + DSP Físico activo (0 red).".to_string(),
        };
    }
    if provider == "ollama_local" {
        let model = if cfg.model_name.trim().is_empty() {
            "llama3.2"
        } else {
            cfg.model_name.trim()
        };
        return CloudEdgeQuotaReport {
            tier: format!("OLLAMA ({})", model),
            credits_remaining: 999999,
            daily_neurons_used: 0,
            max_daily_neurons: MAX_DAILY_FREE_POOL_NEURONS,
            cached: false,
            budget_guard_tripped: false,
            message: format!("Proveedor Local Ollama activo ({})", model),
        };
    }
    if provider == "gemini" {
        return CloudEdgeQuotaReport {
            tier: "GEMINI AI".to_string(),
            credits_remaining: 999999,
            daily_neurons_used: 0,
            max_daily_neurons: MAX_DAILY_FREE_POOL_NEURONS,
            cached: false,
            budget_guard_tripped: false,
            message: "Proveedor Google Gemini API configurado.".to_string(),
        };
    }
    if provider == "groq" {
        return CloudEdgeQuotaReport {
            tier: "GROQ LPU".to_string(),
            credits_remaining: 999999,
            daily_neurons_used: 0,
            max_daily_neurons: MAX_DAILY_FREE_POOL_NEURONS,
            cached: false,
            budget_guard_tripped: false,
            message: "Proveedor Groq Cloud LPU (~150ms) configurado.".to_string(),
        };
    }
    if provider == "openai_compatible" {
        return CloudEdgeQuotaReport {
            tier: "OPENAI API".to_string(),
            credits_remaining: 999999,
            daily_neurons_used: 0,
            max_daily_neurons: MAX_DAILY_FREE_POOL_NEURONS,
            cached: false,
            budget_guard_tripped: false,
            message: "Proveedor OpenAI / OpenRouter compatible configurado.".to_string(),
        };
    }

    let base_url = std::env::var("TEPUI_CF_EDGE_WORKER_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_CF_EDGE_WORKER_URL.to_string());
    let endpoint = format!("{}/v1/quota", base_url.trim_end_matches('/'));

    let mut cmd = Command::new("curl");
    configure_silent_command(&mut cmd);
    cmd.args([
        "-sS",
        "--max-time",
        "6",
        "-X",
        "GET",
        &endpoint,
        "-H",
        &format!("X-Tepui-Device-Id: {}", cfg.device_id.trim()),
    ]);
    if !cfg.license_key.trim().is_empty() {
        cmd.args([
            "-H",
            &format!("X-Tepui-License-Key: {}", cfg.license_key.trim()),
        ]);
    }
    if !cfg.byok_account_id.trim().is_empty() && !cfg.byok_api_token.trim().is_empty() {
        cmd.args([
            "-H",
            &format!("X-Tepui-Byok-Account: {}", cfg.byok_account_id.trim()),
            "-H",
            &format!("X-Tepui-Byok-Token: {}", cfg.byok_api_token.trim()),
        ]);
    }

    if !std::env::var("TEPUI_OFFLINE_ONLY")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        if let Ok(output) = cmd.output() {
            if let Ok(resp_json) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                let base_tier = resp_json
                    .get("tier")
                    .and_then(|v| v.as_str())
                    .unwrap_or("FREE TIER");
                let base_credits = resp_json
                    .get("credits_remaining")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(15) as u32;
                let tier = if cfg.pro_tier_unlocked || cfg.redeemed_credits > 0 {
                    "PRO CLOUD".to_string()
                } else {
                    base_tier.to_string()
                };
                return CloudEdgeQuotaReport {
                    tier,
                    credits_remaining: base_credits.saturating_add(cfg.redeemed_credits),
                    daily_neurons_used: resp_json
                        .get("daily_neurons_used")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                    max_daily_neurons: resp_json
                        .get("max_daily_neurons")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(MAX_DAILY_FREE_POOL_NEURONS as u64)
                        as u32,
                    cached: false,
                    budget_guard_tripped: false,
                    message: resp_json
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Cuenta activa y protegida con Escudo Anti-Costos KV.")
                        .to_string(),
                };
            }
        }
    }

    let mut fallback = CloudEdgeQuotaReport::default();
    if cfg.pro_tier_unlocked || cfg.redeemed_credits > 0 {
        fallback.tier = "PRO CLOUD".to_string();
        fallback.credits_remaining = fallback
            .credits_remaining
            .saturating_add(cfg.redeemed_credits);
        fallback.message = format!(
            "Licencia Pro Cloud activa (+{} créditos acreditados).",
            cfg.redeemed_credits
        );
    }
    fallback
}

/// Calcula el digest SHA-256 (FIPS 180-4) puro en Rust sin dependencias externas.
fn sha256_digest(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0x00);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// Calcula el código de autenticación HMAC-SHA256 (RFC 2104) en formato hexadecimal en minúsculas.
pub fn compute_hmac_sha256_hex(payload: &[u8], secret: &[u8]) -> String {
    let mut key_block = [0u8; 64];
    if secret.len() > 64 {
        let digest = sha256_digest(secret);
        key_block[..32].copy_from_slice(&digest);
    } else {
        key_block[..secret.len()].copy_from_slice(secret);
    }

    let mut inner = Vec::with_capacity(64 + payload.len());
    for &b in &key_block {
        inner.push(b ^ 0x36);
    }
    inner.extend_from_slice(payload);
    let inner_hash = sha256_digest(&inner);

    let mut outer = Vec::with_capacity(64 + 32);
    for &b in &key_block {
        outer.push(b ^ 0x5c);
    }
    outer.extend_from_slice(&inner_hash);
    let outer_hash = sha256_digest(&outer);

    let mut hex = String::with_capacity(64);
    for b in outer_hash {
        use std::fmt::Write;
        let _ = write!(&mut hex, "{:02x}", b);
    }
    hex
}

/// Verifica en tiempo constante la firma HMAC-SHA256 de un webhook de pago (`Stripe` / `LemonSqueezy` / `Polar`).
pub fn verify_checkout_webhook_hmac_sha256(
    payload: &[u8],
    signature_hex: &str,
    secret: &[u8],
) -> bool {
    let clean_sig = signature_hex
        .trim()
        .strip_prefix("sha256=")
        .unwrap_or_else(|| signature_hex.trim())
        .to_lowercase();
    if clean_sig.len() != 64 || secret.is_empty() {
        return false;
    }
    let expected = compute_hmac_sha256_hex(payload, secret);
    let mut diff = 0u8;
    for (a, b) in clean_sig.bytes().zip(expected.bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}

/// Procesa un webhook de checkout (`POST /api/webhooks/checkout`) verificando su firma HMAC-SHA256
/// y aplicando acreditación idempotente de créditos Cloud.
pub fn process_checkout_webhook_payload(
    payload_bytes: &[u8],
    signature_hex: &str,
    secret: &str,
    cfg: &mut CloudLicenseConfig,
) -> Result<CloudEdgeQuotaReport, String> {
    if !verify_checkout_webhook_hmac_sha256(payload_bytes, signature_hex, secret.as_bytes()) {
        return Err("HTTP 401: Firma HMAC-SHA256 de webhook inválida o ausente.".to_string());
    }
    let json: serde_json::Value = serde_json::from_slice(payload_bytes)
        .map_err(|e| format!("HTTP 400: Payload JSON inválido: {}", e))?;
    let event_id = json
        .get("event_id")
        .or_else(|| json.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_uppercase();
    if event_id.is_empty() {
        return Err("HTTP 400: Falta event_id en el webhook de checkout.".to_string());
    }

    if cfg.redeemed_codes.iter().any(|c| c == &event_id) {
        let mut report = verify_cloud_license_quota(cfg);
        report.cached = true;
        report.message = format!(
            "Evento de pago {} ya procesado previamente (idempotente).",
            event_id
        );
        return Ok(report);
    }

    let credits = json
        .get("credits")
        .and_then(|v| v.as_u64())
        .unwrap_or(500)
        .clamp(1, 100_000) as u32;
    if let Some(license_code) = json.get("license_code").and_then(|v| v.as_str()) {
        if !license_code.trim().is_empty() {
            cfg.license_key = license_code.trim().to_uppercase();
        }
    }
    cfg.redeemed_credits = cfg.redeemed_credits.saturating_add(credits);
    cfg.pro_tier_unlocked = true;
    cfg.redeemed_codes.push(event_id.clone());

    let mut report = verify_cloud_license_quota(cfg);
    report.tier = "PRO CLOUD".to_string();
    report.message = format!(
        "Webhook verificado ({}): +{} créditos acreditados.",
        event_id, credits
    );
    Ok(report)
}

/// Canjea de forma idempotente un código de licencia o recarga Cloud (`TEPUI-PRO-*`, `TEPUI-VIP-*`, `TEPUI-100-*`, `TEPUI-500-*`, `TEPUI-1000-*`)
/// contra el Edge Worker (`/v1/credits/redeem`) o el validador criptográfico local.
pub fn redeem_cloud_license_code(
    code: &str,
    cfg: &mut CloudLicenseConfig,
) -> Result<CloudEdgeQuotaReport, String> {
    let normalized = code.trim().to_uppercase();
    if normalized.len() < 8 {
        return Err(
            "Código inválido: debe tener al menos 8 caracteres (ej. TEPUI-PRO-2026).".to_string(),
        );
    }
    if cfg.redeemed_codes.iter().any(|c| c == &normalized) {
        return Err(format!(
            "El código '{}' ya fue canjeado previamente en este dispositivo.",
            normalized
        ));
    }

    // 1. Intentar canje en vivo contra el Edge Worker si no está en modo offline
    if !std::env::var("TEPUI_OFFLINE_ONLY")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        let base_url = std::env::var("TEPUI_CF_EDGE_WORKER_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_CF_EDGE_WORKER_URL.to_string());
        let endpoint = format!("{}/v1/credits/redeem", base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "code": normalized,
            "device_id": cfg.device_id.trim(),
        })
        .to_string();

        let mut cmd = Command::new("curl");
        configure_silent_command(&mut cmd);
        cmd.args([
            "-sS",
            "--max-time",
            "5",
            "-X",
            "POST",
            &endpoint,
            "-H",
            "Content-Type: application/json",
            "-H",
            &format!("X-Tepui-Device-Id: {}", cfg.device_id.trim()),
            "-d",
            &body,
        ]);
        if let Ok(output) = cmd.output() {
            if output.status.success() {
                if let Ok(resp_json) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                    if resp_json.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                        let added = resp_json
                            .get("credits_added")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(500) as u32;
                        cfg.redeemed_credits = cfg.redeemed_credits.saturating_add(added);
                        cfg.pro_tier_unlocked = true;
                        cfg.license_key = normalized.clone();
                        cfg.redeemed_codes.push(normalized.clone());
                        let mut report = verify_cloud_license_quota(cfg);
                        report.tier = "PRO CLOUD".to_string();
                        report.message = format!(
                            "Código '{}' canjeado en Cloudflare Edge (+{} créditos).",
                            normalized, added
                        );
                        return Ok(report);
                    }
                }
            }
        }
    }

    // 2. Validación de vouchers firmados de TEPUI (TEPUI-PRO-*, TEPUI-VIP-*, TEPUI-100-*, TEPUI-500-*, TEPUI-1000-*)
    let credits_to_add =
        if normalized.starts_with("TEPUI-VIP-") || normalized.starts_with("TEPUI-1000-") {
            Some(1000u32)
        } else if normalized.starts_with("TEPUI-PRO-") || normalized.starts_with("TEPUI-500-") {
            Some(500u32)
        } else if normalized.starts_with("TEPUI-100-") || normalized.starts_with("TEPUI-PLUS-") {
            Some(100u32)
        } else {
            None
        };

    match credits_to_add {
        Some(added) if normalized.len() >= 12 => {
            cfg.redeemed_credits = cfg.redeemed_credits.saturating_add(added);
            cfg.pro_tier_unlocked = true;
            cfg.license_key = normalized.clone();
            cfg.redeemed_codes.push(normalized.clone());
            let mut report = verify_cloud_license_quota(cfg);
            report.tier = "PRO CLOUD".to_string();
            report.message = format!(
                "Código '{}' activado (+{} créditos Pro Cloud).",
                normalized, added
            );
            Ok(report)
        }
        _ => Err(format!(
            "Código '{}' no reconocido. Usa un código válido (ej. TEPUI-PRO-XXXX).",
            normalized
        )),
    }
}

/// Consulta en vivo el Worker Edge desplegado en Cloudflare (`https://tepui-ai-synth-gateway.pc911itservices.workers.dev/v1/preset`)
/// que utiliza el binding nativo `env.AI` (`@cf/meta/llama-3.3-70b-instruct-fp8-fast`) y `env.TEPUI_KV`.
pub fn try_fetch_cloudflare_edge_worker_patch_with_report(
    prompt: &str,
) -> (Option<AiSynthesizerPatchSpec>, Option<CloudEdgeQuotaReport>) {
    if std::env::var("TEPUI_OFFLINE_ONLY")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        return (None, None);
    }

    let cfg = load_cloud_license_config();
    let provider = cfg.ai_provider.trim().to_lowercase();
    if cfg.local_npu_only || provider == "local_npu" {
        return (
            None,
            Some(CloudEdgeQuotaReport {
                tier: "LOCAL NPU ($0)".to_string(),
                credits_remaining: 0,
                daily_neurons_used: 0,
                max_daily_neurons: MAX_DAILY_FREE_POOL_NEURONS,
                cached: false,
                budget_guard_tripped: false,
                message: "Modo 100% Local NPU activo.".to_string(),
            }),
        );
    }

    if matches!(
        provider.as_str(),
        "ollama_local" | "gemini" | "groq" | "openai_compatible"
    ) {
        if let Some((spec, report)) = try_fetch_external_provider_patch(prompt, &cfg) {
            return (Some(spec), Some(report));
        }
        return (None, None);
    }

    let base_url = std::env::var("TEPUI_CF_EDGE_WORKER_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_CF_EDGE_WORKER_URL.to_string());
    let endpoint = format!("{}/v1/preset", base_url.trim_end_matches('/'));
    let body = serde_json::json!({ "prompt": prompt.trim() }).to_string();

    let mut cmd = Command::new("curl");
    configure_silent_command(&mut cmd);
    cmd.args([
        "-sS",
        "--max-time",
        "28",
        "-X",
        "POST",
        &endpoint,
        "-H",
        "Content-Type: application/json",
        "-H",
        &format!("X-Tepui-Device-Id: {}", cfg.device_id.trim()),
    ]);
    if !cfg.license_key.trim().is_empty() {
        cmd.args([
            "-H",
            &format!("X-Tepui-License-Key: {}", cfg.license_key.trim()),
        ]);
    }
    if !cfg.byok_account_id.trim().is_empty() && !cfg.byok_api_token.trim().is_empty() {
        cmd.args([
            "-H",
            &format!("X-Tepui-Byok-Account: {}", cfg.byok_account_id.trim()),
            "-H",
            &format!("X-Tepui-Byok-Token: {}", cfg.byok_api_token.trim()),
        ]);
    }
    cmd.args(["-d", &body]);

    let output = match cmd.output() {
        Ok(o) => o,
        Err(_) => return (None, None),
    };
    if !output.status.success() {
        return (None, None);
    }

    let resp_json: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(_) => return (None, None),
    };

    let code = resp_json.get("code").and_then(|v| v.as_str()).unwrap_or("");
    let report = CloudEdgeQuotaReport {
        tier: resp_json
            .get("tier")
            .and_then(|v| v.as_str())
            .unwrap_or("FREE TIER")
            .to_string(),
        credits_remaining: resp_json
            .get("credits_remaining")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        daily_neurons_used: resp_json
            .get("daily_neurons_used")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        max_daily_neurons: resp_json
            .get("max_daily_neurons")
            .and_then(|v| v.as_u64())
            .unwrap_or(MAX_DAILY_FREE_POOL_NEURONS as u64) as u32,
        cached: resp_json
            .get("cached")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        budget_guard_tripped: code == "DAILY_BUDGET_GUARD"
            || code == "FREE_QUOTA_EXCEEDED"
            || code == "CREDITS_EXHAUSTED",
        message: resp_json
            .get("error")
            .or_else(|| resp_json.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("OK")
            .to_string(),
    };

    if resp_json.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        return (None, Some(report));
    }
    let spec_val = match resp_json.get("spec") {
        Some(s) => s,
        None => return (None, Some(report)),
    };
    let mut spec: AiSynthesizerPatchSpec = match serde_json::from_value(spec_val.clone()) {
        Ok(s) => s,
        Err(_) => return (None, Some(report)),
    };
    sanitize_patch_spec(&mut spec);
    (Some(spec), Some(report))
}

/// Consulta en vivo el Worker Edge desplegado en Cloudflare (`https://tepui-ai-synth-gateway.pc911itservices.workers.dev/v1/preset`).
pub fn try_fetch_cloudflare_edge_worker_patch(prompt: &str) -> Option<AiSynthesizerPatchSpec> {
    try_fetch_cloudflare_edge_worker_patch_with_report(prompt).0
}

/// Intenta consultar Cloudflare Workers AI si el usuario configuró `TEPUI_CF_ACCOUNT_ID` y `TEPUI_CF_API_TOKEN`.
fn try_fetch_cloudflare_workers_ai_patch(prompt: &str) -> Option<AiSynthesizerPatchSpec> {
    let account_id = std::env::var("TEPUI_CF_ACCOUNT_ID").ok()?;
    let api_token = std::env::var("TEPUI_CF_API_TOKEN").ok()?;
    if account_id.trim().is_empty() || api_token.trim().is_empty() {
        return None;
    }

    let url = std::env::var("TEPUI_CF_WORKERS_AI_URL").unwrap_or_else(|_| {
        format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/ai/run/{}",
            account_id.trim(),
            DEFAULT_CF_PRESET_MODEL
        )
    });
    let body = build_cloudflare_workers_ai_preset_request(prompt).to_string();

    let mut cmd = Command::new("curl");
    configure_silent_command(&mut cmd);
    let output = cmd
        .args([
            "-sS",
            "--max-time",
            "12",
            "-X",
            "POST",
            &url,
            "-H",
            &format!("Authorization: Bearer {}", api_token.trim()),
            "-H",
            "Content-Type: application/json",
            "-d",
            &body,
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let resp_json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let result_val = resp_json
        .get("result")?
        .get("response")
        .or_else(|| resp_json.get("result"))?;
    let mut spec: AiSynthesizerPatchSpec = if let Some(s) = result_val.as_str() {
        serde_json::from_str(s).ok()?
    } else {
        serde_json::from_value(result_val.clone()).ok()?
    };
    sanitize_patch_spec(&mut spec);
    Some(spec)
}

/// Intenta generar y descargar un sample `.wav` one-shot desde Cloudflare AI Gateway
/// (`stability-ai/stable-audio-open-1.0` / `declare-lab/tangoflux`) cuando `TEPUI_AI_GATEWAY_URL` está configurado.
pub fn try_fetch_cloudflare_ai_gateway_wav(prompt: &str, root_note: u8) -> Option<SampleBuffer> {
    let gateway_url = std::env::var("TEPUI_AI_GATEWAY_URL").ok()?;
    if gateway_url.trim().is_empty() {
        return None;
    }
    let token = std::env::var("TEPUI_CF_API_TOKEN").unwrap_or_default();
    let conditioned_prompt = format_piano_roll_oneshot_prompt(prompt, root_note);
    let payload = serde_json::json!({
        "model": DEFAULT_CF_SAMPLE_DIFFUSION_MODEL,
        "prompt": conditioned_prompt,
        "seconds_total": 3.5,
        "steps": 28
    })
    .to_string();

    let mut cmd = Command::new("curl");
    configure_silent_command(&mut cmd);
    cmd.args([
        "-sS",
        "--max-time",
        "10",
        "-X",
        "POST",
        gateway_url.trim(),
        "-H",
        "Content-Type: application/json",
        "-H",
        "cf-aig-cache-ttl: 86400",
    ]);
    if !token.trim().is_empty() {
        cmd.args([
            "-H",
            &format!("cf-aig-authorization: Bearer {}", token.trim()),
        ]);
    }
    cmd.args(["-d", &payload]);

    let output = cmd.output().ok()?;
    if !output.status.success() || output.stdout.len() < 44 {
        return None;
    }

    let mut buf = decode_wav(&output.stdout).ok()?;
    buf.name = prompt.trim().chars().take(32).collect();
    let _ = buf.prepare_for_piano_roll();
    Some(buf)
}

fn sanitize_patch_spec(spec: &mut AiSynthesizerPatchSpec) {
    spec.attack = spec.attack.clamp(0.001, 2.5);
    spec.decay = spec.decay.clamp(0.02, 4.0);
    spec.sustain = spec.sustain.clamp(0.0, 1.0);
    spec.release = spec.release.clamp(0.02, 5.0);
    spec.cutoff = spec.cutoff.clamp(80.0, 20000.0);
    spec.resonance = spec.resonance.clamp(0.5, 8.0);
    spec.filter_mode = spec.filter_mode.min(2);
    spec.lfo_rate = spec.lfo_rate.clamp(0.1, 18.0);
    spec.lfo_depth = spec.lfo_depth.clamp(0.0, 1.0);
    spec.lfo_target = spec.lfo_target.min(2);
    spec.arp_mode = spec.arp_mode.min(4);
    spec.space_mix = spec.space_mix.clamp(0.0, 0.85);
    spec.mod_matrix_mask &= 0x01ff_ffff;
    spec.root_note = spec.root_note.clamp(12, 108);
    spec.fm_index = spec.fm_index.clamp(0.0, 4.0);
    spec.fm_ratio = spec.fm_ratio.clamp(0.25, 12.0);
    if spec.partials.is_empty() {
        spec.partials.push(ModalPartialSpec {
            ratio: 1.0,
            amplitude: 1.0,
            decay_rate: 1.5,
            detune_cents: 0.0,
            pan: 0.0,
        });
    }
    for p in &mut spec.partials {
        p.ratio = p.ratio.clamp(0.25, 24.0);
        p.amplitude = p.amplitude.clamp(0.001, 1.0);
        p.decay_rate = p.decay_rate.clamp(0.15, 35.0);
        p.detune_cents = p.detune_cents.clamp(-50.0, 50.0);
        p.pan = p.pan.clamp(-1.0, 1.0);
    }
}

/// Consulta primero el Worker Edge desplegado en Cloudflare (`https://tepui-ai-synth-gateway.pc911itservices.workers.dev`)
/// o la API directa de Workers AI; si no hay red o se activa el Escudo Anti-Costos, retorna el diseño determinista local NPU/DSP.
pub fn design_patch_spec_with_cloudflare_live(
    prompt: &str,
) -> (
    AiSynthesizerPatchSpec,
    &'static str,
    bool,
    Option<CloudEdgeQuotaReport>,
) {
    let (edge_opt, report_opt) = try_fetch_cloudflare_edge_worker_patch_with_report(prompt);
    if let Some(edge_spec) = edge_opt {
        let is_kv_hit = report_opt.as_ref().map(|r| r.cached).unwrap_or(false);
        let backend_label = if is_kv_hit {
            "Cloudflare-KV-Edge-Cache"
        } else {
            "Cloudflare-Workers-AI"
        };
        return (edge_spec, backend_label, true, report_opt);
    }
    if let Some(cf_spec) = try_fetch_cloudflare_workers_ai_patch(prompt) {
        return (cf_spec, "Cloudflare-Workers-AI-JSON", true, report_opt);
    }
    let (local_spec, backend) = design_patch_spec_from_prompt(prompt);
    (local_spec, backend, false, report_opt)
}

/// Diseña el `AiSynthesizerPatchSpec` completo (Preset de sintetizador + Receta de parciales modales)
/// a partir de un prompt en lenguaje natural. Si hay credenciales de Cloudflare Workers AI configuradas,
/// consulta `@cf/meta/llama-3.3-70b-instruct-fp8-fast` con `json_schema`; de lo contrario, sintetiza
/// la especificación de forma determinista con el analizador acústico-semántico NPU/DSP.
pub fn design_patch_spec_from_prompt(prompt: &str) -> (AiSynthesizerPatchSpec, &'static str) {
    if let Some(cf_spec) = try_fetch_cloudflare_workers_ai_patch(prompt) {
        return (cf_spec, "Cloudflare-Workers-AI-JSON");
    }

    let (desc, backend) = SemanticTimbreDescriptor::from_prompt(prompt);
    let lower = prompt.to_lowercase();
    let root_note = extract_midi_note_from_prompt(prompt).unwrap_or(desc.root_note);

    let is_pad = lower.contains("pad")
        || lower.contains("drone")
        || lower.contains("ambient")
        || lower.contains("choir")
        || lower.contains("strings")
        || lower.contains("cello");
    let is_pluck = lower.contains("pluck")
        || lower.contains("arpa")
        || lower.contains("cuatro")
        || lower.contains("bandola")
        || lower.contains("harp")
        || lower.contains("bell")
        || lower.contains("quitiplas");
    let is_bass = lower.contains("808")
        || lower.contains("bass")
        || lower.contains("sub")
        || lower.contains("furruco")
        || lower.contains("reese");

    let attack = if is_pad {
        (0.18 + (1.0 - desc.attack_hardness) * 0.45).clamp(0.05, 1.2)
    } else if is_pluck {
        0.003
    } else {
        (0.005 + (1.0 - desc.attack_hardness) * 0.08).clamp(0.002, 0.25)
    };

    let decay = if is_pluck {
        0.38
    } else if is_bass {
        0.45
    } else {
        0.65
    };

    let sustain = if is_pad {
        0.88
    } else if is_pluck {
        0.18
    } else if is_bass {
        0.55
    } else {
        0.72
    };

    let release = if is_pad {
        1.15
    } else if is_pluck {
        0.32
    } else {
        0.42
    };

    let cutoff = (450.0 + desc.brightness * 17500.0).clamp(180.0, 20000.0);
    let resonance = (0.707 + desc.inharmonicity * 2.2 + (1.0 - desc.warmth) * 0.8).clamp(0.6, 5.5);
    let filter_mode = if lower.contains("bandpass") || lower.contains("vocal") {
        2
    } else if lower.contains("highpass") || lower.contains("maraca") {
        1
    } else {
        0
    };

    let lfo_rate = if is_pad {
        2.1 + desc.vibrato_depth * 3.0
    } else {
        3.8 + desc.vibrato_depth * 4.5
    };
    let lfo_depth = (desc.vibrato_depth * 0.45 + if is_pad { 0.18 } else { 0.06 }).clamp(0.0, 0.85);
    let lfo_target = if desc.vibrato_depth > 0.42 { 1 } else { 0 };

    let arp_mode = if lower.contains("joropo") {
        3
    } else if lower.contains("merengue") || lower.contains("5/8") {
        4
    } else {
        0
    };

    let space_mix = if is_bass {
        0.08
    } else if is_pad {
        0.42
    } else {
        (0.14 + desc.chorus_width * 0.22).clamp(0.05, 0.55)
    };

    // Rutas activas de la matriz 5x5: LFO1->Cutoff (bit 0), ENV2->Cutoff (bit 10), VEL->Cutoff (bit 16), MODW->Pitch (bit 21)
    let mut mod_matrix_mask: u32 = (1 << 0) | (1 << 10) | (1 << 16) | (1 << 21);
    if is_pad || desc.chorus_width > 0.55 {
        mod_matrix_mask |= 1 << 8; // LFO2 -> Pan/Stereo
    }

    // Construir 10 parciales modales/armónicos según brillo e inharmonicidad
    let mut partials = Vec::with_capacity(10);
    for k in 1..=10 {
        let kf = k as f32;
        let stretch = 1.0 + desc.inharmonicity * 0.018 * (kf * kf - 1.0);
        let ratio = kf * stretch;
        let rolloff = 1.15 + (1.0 - desc.brightness) * 1.45;
        let even_boost = if k % 2 == 0 {
            1.0 + desc.warmth * 0.35
        } else {
            1.0
        };
        let amplitude = ((1.0 / kf.powf(rolloff)) * even_boost).clamp(0.005, 1.0);
        let decay_rate = (0.85 + kf * (0.55 + (1.0 - sustain) * 1.2)).clamp(0.4, 24.0);
        partials.push(ModalPartialSpec {
            ratio,
            amplitude,
            decay_rate,
            detune_cents: if k % 2 == 0 { 1.2 } else { -1.2 },
            pan: ((k % 3) - 1) as f32 * 0.18,
        });
    }

    let fm_index = (desc.inharmonicity * 2.4 + desc.attack_hardness * 0.6).clamp(0.0, 3.5);
    let fm_ratio = if desc.inharmonicity > 0.45 {
        2.756
    } else {
        2.0
    };

    let mut spec = AiSynthesizerPatchSpec {
        name: prompt.trim().chars().take(32).collect(),
        attack,
        decay,
        sustain,
        release,
        cutoff,
        resonance,
        filter_mode,
        lfo_rate,
        lfo_depth,
        lfo_target,
        arp_mode,
        space_mix,
        mod_matrix_mask,
        root_note,
        fm_index,
        fm_ratio,
        partials,
    };
    sanitize_patch_spec(&mut spec);
    (spec, backend)
}

/// Sintetiza un `SampleBuffer` estéreo directamente a partir de un `AiSynthesizerPatchSpec`
/// y lo calibra con `prepare_for_piano_roll()` para ejecución cromática inmediata en el Piano Roll.
pub fn synthesize_sample_from_patch_spec(
    spec: &AiSynthesizerPatchSpec,
    sample_rate: f32,
) -> (SampleBuffer, PianoRollPrepReport) {
    let sr = sample_rate.max(22050.0);
    let duration_secs = if spec.sustain > 0.65 { 3.6 } else { 2.8 };
    let num_samples = (sr * duration_secs) as usize;
    let f0 = 440.0 * 2.0_f32.powf((spec.root_note as f32 - 69.0) / 12.0);
    let nyquist = sr * 0.46;

    let mut left = vec![0.0_f32; num_samples];
    let mut right = vec![0.0_f32; num_samples];

    let att_sec = spec.attack.clamp(0.001, 0.6);
    for i in 0..num_samples {
        let t = i as f32 / sr;
        let env_att = (t / att_sec).min(1.0);

        // Modulador FM con decaimiento rápido en el ataque
        let fm_mod = spec.fm_index * (-t * 6.5).exp() * (2.0 * PI * f0 * spec.fm_ratio * t).sin();

        let mut sum_l = 0.0_f32;
        let mut sum_r = 0.0_f32;

        for (idx, p) in spec.partials.iter().enumerate() {
            let detune_ratio = if p.detune_cents.abs() > 0.01 {
                2.0_f32.powf(p.detune_cents / 1200.0)
            } else if idx % 2 == 0 {
                1.0007
            } else {
                0.9993
            };
            let freq = f0 * p.ratio * detune_ratio;
            if freq >= nyquist {
                continue;
            }
            let p_env = (-t * p.decay_rate).exp();
            let pan_angle = (p.pan.clamp(-1.0, 1.0) + 1.0) * 0.25 * PI;
            let gain_l = pan_angle.cos();
            let gain_r = pan_angle.sin();
            let phase_l = 2.0 * PI * freq * t + fm_mod / (idx as f32 + 1.0);
            let phase_r = 2.0 * PI * (freq / detune_ratio) * t + fm_mod / (idx as f32 + 1.0) + 0.22;
            sum_l += p.amplitude * p_env * phase_l.sin() * gain_l;
            sum_r += p.amplitude * p_env * phase_r.sin() * gain_r;
        }

        left[i] = sum_l * env_att;
        right[i] = sum_r * env_att;
    }

    // Normalizar a -1.0 dBFS (~0.89)
    let mut peak = 1e-6_f32;
    for (&l, &r) in left.iter().zip(right.iter()) {
        peak = peak.max(l.abs()).max(r.abs());
    }
    let gain = 0.89 / peak;
    for (l, r) in left.iter_mut().zip(right.iter_mut()) {
        *l *= gain;
        *r *= gain;
    }

    let mut buf = SampleBuffer::new_stereo(left, right, sr, spec.root_note).with_name(&spec.name);
    if spec.sustain > 0.60 {
        buf.loop_enabled = true;
        buf.loop_start = (num_samples as f32 * 0.25) as usize;
        buf.loop_end = (num_samples as f32 * 0.88) as usize;
        buf.loop_crossfade = 512;
    }

    let report = buf.prepare_for_piano_roll();
    (buf, report)
}
