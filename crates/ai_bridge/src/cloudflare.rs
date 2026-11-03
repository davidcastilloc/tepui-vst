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
/// con binding nativo a Cloudflare Workers AI (`env.AI`).
pub const DEFAULT_CF_EDGE_WORKER_URL: &str =
    "https://tepui-ai-synth-gateway.pc911itservices.workers.dev";

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

/// Consulta en vivo el Worker Edge desplegado en Cloudflare (`https://tepui-ai-synth-gateway.pc911itservices.workers.dev/v1/preset`)
/// que utiliza el binding nativo `env.AI` (`@cf/meta/llama-3.3-70b-instruct-fp8-fast`).
pub fn try_fetch_cloudflare_edge_worker_patch(prompt: &str) -> Option<AiSynthesizerPatchSpec> {
    if std::env::var("TEPUI_OFFLINE_ONLY")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        return None;
    }

    let base_url = std::env::var("TEPUI_CF_EDGE_WORKER_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_CF_EDGE_WORKER_URL.to_string());
    let endpoint = format!("{}/v1/preset", base_url.trim_end_matches('/'));
    let body = serde_json::json!({ "prompt": prompt.trim() }).to_string();

    let mut cmd = Command::new("curl");
    configure_silent_command(&mut cmd);
    let output = cmd
        .args([
            "-sS",
            "--max-time",
            "28",
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
    if resp_json.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        return None;
    }
    let spec_val = resp_json.get("spec")?;
    let mut spec: AiSynthesizerPatchSpec = serde_json::from_value(spec_val.clone()).ok()?;
    sanitize_patch_spec(&mut spec);
    Some(spec)
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
/// o la API directa de Workers AI; si no hay red, retorna el diseño determinista local NPU/DSP.
/// El tercer elemento de la tupla indica `true` cuando provino en vivo de Cloudflare Workers AI.
pub fn design_patch_spec_with_cloudflare_live(
    prompt: &str,
) -> (AiSynthesizerPatchSpec, &'static str, bool) {
    if let Some(edge_spec) = try_fetch_cloudflare_edge_worker_patch(prompt) {
        return (edge_spec, "Cloudflare-Workers-AI", true);
    }
    if let Some(cf_spec) = try_fetch_cloudflare_workers_ai_patch(prompt) {
        return (cf_spec, "Cloudflare-Workers-AI-JSON", true);
    }
    let (local_spec, backend) = design_patch_spec_from_prompt(prompt);
    (local_spec, backend, false)
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
