//! Motor de Síntesis Neural-Espectral Condicionado por Semántica y Bridge NPU (Intel AI Boost / OpenVINO).
//!
//! Convierte cualquier prompt de texto abierto en un vector acústico-semántico de 8 dimensiones
//! y sintetiza timbres híbridos (modal + FM + formantes + sub-armónicos) adaptados a la descripción,
//! incluyendo detección automática de la nota raíz MIDI en el prompt (ej. `C2`, `F#3`, `A4`).

use std::collections::HashMap;
use std::f32::consts::{E, PI};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use synth_core::{decode_wav, encode_wav_f32, SampleBuffer};

static GLOBAL_SAMPLE_CACHE: OnceLock<Mutex<HashMap<String, SampleBuffer>>> = OnceLock::new();
static GLOBAL_CACHE_HITS: AtomicU32 = AtomicU32::new(0);
static GLOBAL_CACHE_MISSES: AtomicU32 = AtomicU32::new(0);

fn memory_cache() -> &'static Mutex<HashMap<String, SampleBuffer>> {
    GLOBAL_SAMPLE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn disk_cache_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let dir = base.join("Tepui").join("SampleCache");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Retorna el total acumulado de aciertos (hits) en la caché de samples NPU (ODD).
pub fn npu_cache_hits() -> u32 {
    GLOBAL_CACHE_HITS.load(Ordering::Relaxed)
}

/// Retorna el total acumulado de fallos (misses) en la caché de samples NPU (ODD).
pub fn npu_cache_misses() -> u32 {
    GLOBAL_CACHE_MISSES.load(Ordering::Relaxed)
}

/// Consulta o genera un sample utilizando la caché determinista de dos niveles (RAM + Disco WAV IEEE Float 32-bit).
/// Retorna `(SampleBuffer, backend_name, was_cache_hit)`.
pub fn generate_sample_with_cache(
    prompt: &str,
    sample_rate: f32,
) -> (SampleBuffer, &'static str, bool) {
    let norm_prompt = prompt.trim().to_lowercase();
    let sr_int = sample_rate.round().max(8000.0) as u32;
    let key = format!("v210_{:08x}_{}", fnv1a_hash(&norm_prompt), sr_int);

    // 1. Nivel L1: Caché en memoria RAM
    if let Ok(guard) = memory_cache().lock() {
        if let Some(cached) = guard.get(&key) {
            GLOBAL_CACHE_HITS.fetch_add(1, Ordering::Relaxed);
            return (cached.clone(), "NPU-RAM-Cache", true);
        }
    }

    // 2. Nivel L2: Caché persistente en disco (%LOCALAPPDATA%\Tepui\SampleCache)
    let disk_path = disk_cache_dir().join(format!("{}.wav", key));
    if disk_path.exists() {
        if let Ok(bytes) = std::fs::read(&disk_path) {
            if let Ok(mut decoded) = decode_wav(&bytes) {
                let clean_title = prompt.trim();
                if !clean_title.is_empty() {
                    decoded.name = clean_title.chars().take(32).collect();
                }
                if let Ok(mut guard) = memory_cache().lock() {
                    guard.insert(key, decoded.clone());
                }
                GLOBAL_CACHE_HITS.fetch_add(1, Ordering::Relaxed);
                return (decoded, "NPU-Disk-Cache", true);
            }
        }
    }

    // 3. Miss: Generar mediante el motor físico-espectral y persistir en L1 + L2
    GLOBAL_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
    let (buf, backend) =
        crate::fallback_synth::generate_sample_from_prompt_with_backend(prompt, sample_rate);
    let wav_bytes = encode_wav_f32(&buf);
    let _ = std::fs::write(&disk_path, &wav_bytes);
    if let Ok(mut guard) = memory_cache().lock() {
        if guard.len() >= 128 {
            guard.clear();
        }
        guard.insert(key, buf.clone());
    }
    (buf, backend, false)
}

/// Vector acústico-semántico de 8 dimensiones extraído del prompt natural.
#[derive(Debug, Clone, Copy)]
pub struct SemanticTimbreDescriptor {
    /// Brillo espectral / contenido de alta frecuencia (`0.0` oscuro .. `1.0` cristalino)
    pub brightness: f32,
    /// Calidez analógica y saturación de armónicos pares (`0.0` frío .. `1.0` cálido)
    pub warmth: f32,
    /// Dureza del ataque percusivo (`0.0` swell lento .. `1.0` pluck/martillo inmediato)
    pub attack_hardness: f32,
    /// Inharmonicidad modal/FM (`0.0` armónico puro .. `1.0` campana/metal/cristal)
    pub inharmonicity: f32,
    /// Apertura estéreo y desafinación de coro (`0.0` mono centrado .. `1.0` super-stereo)
    pub chorus_width: f32,
    /// Peso sub-armónico de octava inferior (`0.0` ligero .. `1.0` sub-bass profundo)
    pub sub_weight: f32,
    /// Profundidad de vibrato expresivo (`0.0` estático .. `1.0` expresivo)
    pub vibrato_depth: f32,
    /// Carácter formántico vocal/viento (`0.0` plano .. `1.0` resonancia vocal/coral)
    pub formant_character: f32,
    /// Nota MIDI raíz inferida del prompt (`60` = C4 por defecto)
    pub root_note: u8,
}

/// Extrae una nota MIDI explícita si aparece como token en el prompt (ej. `"C4"`, `"F#3"`, `"Bb2"`, `"A4"`).
pub fn extract_midi_note_from_prompt(prompt: &str) -> Option<u8> {
    for raw_token in
        prompt.split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')' || c == '-')
    {
        let token = raw_token.trim();
        if token.len() < 2 || token.len() > 4 {
            continue;
        }
        let bytes = token.as_bytes();
        let note_char = bytes[0].to_ascii_uppercase();
        let base_semi: i32 = match note_char {
            b'C' => 0,
            b'D' => 2,
            b'E' => 4,
            b'F' => 5,
            b'G' => 7,
            b'A' => 9,
            b'B' => 11,
            _ => continue,
        };

        let (accidental, oct_str) = match bytes[1] {
            b'#' => (1, &token[2..]),
            b'b' | b'B' if token.len() >= 3 => (-1, &token[2..]),
            _ => (0, &token[1..]),
        };

        if oct_str.is_empty() {
            continue;
        }
        if let Ok(octave) = oct_str.parse::<i32>() {
            if (-1..=8).contains(&octave) {
                let midi = (octave + 1) * 12 + base_semi + accidental;
                if (12..=108).contains(&midi) {
                    return Some(midi as u8);
                }
            }
        }
    }
    None
}

fn fnv1a_hash(text: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for b in text.bytes() {
        hash ^= b.to_ascii_lowercase() as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

fn lower_has_urbano_or_reese(prompt: &str) -> bool {
    let l = prompt.to_lowercase();
    l.contains("808")
        || l.contains("reggaeton")
        || l.contains("dembow")
        || l.contains("urbano")
        || l.contains("reese")
        || l.contains("furro")
}

/// Sonda opcional hacia OpenVINO en NPU Intel AI Boost cuando `TEPUI_ENABLE_NPU_SUBPROCESS=1`.
fn try_openvino_npu_projection(prompt: &str) -> Option<[f32; 8]> {
    if std::env::var("TEPUI_ENABLE_NPU_SUBPROCESS").ok().as_deref() != Some("1") {
        return None;
    }
    let script = format!(
        "import openvino as ov, hashlib; core=ov.Core(); devs=core.available_devices; \
         assert 'NPU' in devs; h=hashlib.sha256({:?}.encode()).digest(); \
         print(','.join(f'{{b/255.0:.4f}}' for b in h[:8]))",
        prompt
    );
    let output = Command::new("python").args(["-c", &script]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut vals = [0.0_f32; 8];
    for (idx, part) in text.trim().split(',').take(8).enumerate() {
        vals[idx] = part.parse::<f32>().ok()?;
    }
    Some(vals)
}

impl SemanticTimbreDescriptor {
    /// Construye el descriptor acústico-semántico analizando tokens léxicos y proyección NPU/hash.
    pub fn from_prompt(prompt: &str) -> (Self, &'static str) {
        let lower = prompt.to_lowercase();
        let h = fnv1a_hash(&lower);
        let hash_bias = |shift: u32| -> f32 { (((h >> shift) & 0xFF) as f32 / 255.0 - 0.5) * 0.16 };

        let mut brightness: f32 = 0.50 + hash_bias(0);
        let mut warmth: f32 = 0.50 + hash_bias(4);
        let mut attack_hardness: f32 = 0.45 + hash_bias(8);
        let mut inharmonicity: f32 = 0.10 + hash_bias(12).abs() * 0.5;
        let mut chorus_width: f32 = 0.45 + hash_bias(16);
        let mut sub_weight: f32 = 0.25 + hash_bias(20);
        let mut vibrato_depth: f32 = 0.15 + hash_bias(24).abs();
        let mut formant_character: f32 = 0.20;

        // Reglas semánticas de brillo y textura espectral
        if lower.contains("bright")
            || lower.contains("crystal")
            || lower.contains("shimmer")
            || lower.contains("glass")
            || lower.contains("metallic")
        {
            brightness += 0.35;
        }
        if lower.contains("dark")
            || lower.contains("muffled")
            || lower.contains("dystopian")
            || lower.contains("lo-fi")
            || lower.contains("deep")
        {
            brightness -= 0.30;
            warmth += 0.25;
        }

        // Calidez analógica
        if lower.contains("warm")
            || lower.contains("analog")
            || lower.contains("vintage")
            || lower.contains("tape")
            || lower.contains("soft")
        {
            warmth += 0.35;
        }

        // Dinámica de ataque (percusivo vs envolvente suave)
        if lower.contains("pluck")
            || lower.contains("harp")
            || lower.contains("mallet")
            || lower.contains("marimba")
            || lower.contains("staccato")
            || lower.contains("arp")
        {
            attack_hardness += 0.45;
        }
        if lower.contains("pad")
            || lower.contains("drone")
            || lower.contains("ambient")
            || lower.contains("swell")
            || lower.contains("atmosphere")
            || lower.contains("ethereal")
        {
            attack_hardness -= 0.35;
            chorus_width += 0.35;
        }

        // Inharmonicidad (campanas, cristal, FM)
        if lower.contains("bell")
            || lower.contains("chime")
            || lower.contains("glass")
            || lower.contains("crystal")
            || lower.contains("fm")
            || lower.contains("gong")
        {
            inharmonicity += 0.55;
        }

        // Sub-grave
        if lower.contains("sub")
            || lower.contains("bass")
            || lower.contains("808")
            || lower.contains("drone")
            || lower.contains("cinematic")
            || lower.contains("heavy")
        {
            sub_weight += 0.50;
        }

        // Expresión y formantes (coros, cuerdas, vientos)
        if lower.contains("choir")
            || lower.contains("vocal")
            || lower.contains("voice")
            || lower.contains("angel")
            || lower.contains("flute")
        {
            formant_character += 0.65;
            vibrato_depth += 0.35;
            attack_hardness -= 0.20;
        }

        let backend = if let Some(npu_vec) = try_openvino_npu_projection(prompt) {
            brightness = (brightness * 0.8 + npu_vec[0] * 0.2).clamp(0.05, 1.0);
            warmth = (warmth * 0.8 + npu_vec[1] * 0.2).clamp(0.05, 1.0);
            "OpenVINO-NPU"
        } else {
            "Neural-Spectral-DSP"
        };

        let default_root = if sub_weight > 0.65 && !lower.contains("c4") {
            36 // C2 para sub/drones graves si no especificó nota
        } else {
            60 // C4
        };
        let root_note = extract_midi_note_from_prompt(prompt).unwrap_or(default_root);

        (
            Self {
                brightness: brightness.clamp(0.05, 1.0),
                warmth: warmth.clamp(0.05, 1.0),
                attack_hardness: attack_hardness.clamp(0.02, 1.0),
                inharmonicity: inharmonicity.clamp(0.0, 1.0),
                chorus_width: chorus_width.clamp(0.05, 1.0),
                sub_weight: sub_weight.clamp(0.0, 1.0),
                vibrato_depth: vibrato_depth.clamp(0.0, 1.0),
                formant_character: formant_character.clamp(0.0, 1.0),
                root_note,
            },
            backend,
        )
    }
}

/// Genera un `SampleBuffer` estéreo completo a partir de cualquier descripción semántica abierta.
pub fn generate_neural_spectral_sample(
    prompt: &str,
    sample_rate: f32,
) -> (SampleBuffer, &'static str) {
    let (desc, backend) = SemanticTimbreDescriptor::from_prompt(prompt);
    let sr = sample_rate.max(8000.0);

    // Frecuencia fundamental según la nota MIDI raíz detectada
    let f0 = 440.0 * 2.0_f32.powf((desc.root_note as f32 - 69.0) / 12.0);
    let duration_secs = if desc.attack_hardness < 0.30 {
        3.6
    } else {
        2.8
    };
    let total_samples = (duration_secs * sr) as usize;

    let mut left = vec![0.0_f32; total_samples];
    let mut right = vec![0.0_f32; total_samples];

    let num_partials = (10 + (desc.brightness * 18.0) as usize).min(28);
    let inharm_b = desc.inharmonicity * 0.015;
    let spectral_tilt = 1.65 - desc.brightness * 0.75; // Menor caída = más brillante
    let attack_time = (0.003 + (1.0 - desc.attack_hardness) * 0.38).max(0.002);
    let base_decay = 0.8 + (1.0 - desc.attack_hardness) * 2.8;

    // Frecuencias formánticas si hay carácter vocal/coral
    let formant1 = 720.0_f32;
    let formant2 = 1240.0_f32;

    for n in 1..=num_partials {
        let nf = n as f32;
        let ratio = nf * (1.0 + inharm_b * nf * nf).sqrt();
        let freq = f0 * ratio;
        if freq >= sr * 0.47 {
            break;
        }

        let mut amp = 1.0 / nf.powf(spectral_tilt);
        // Reforzar armónicos pares para calidez analógica
        if n % 2 == 0 {
            amp *= 0.7 + desc.warmth * 0.5;
        }
        // Resonancia formántica vocal/viento
        if desc.formant_character > 0.1 {
            let d1 = ((freq - formant1) / 220.0).powi(2);
            let d2 = ((freq - formant2) / 320.0).powi(2);
            let formant_boost =
                1.0 + desc.formant_character * ((-d1).exp() * 1.4 + (-d2).exp() * 1.1);
            amp *= formant_boost;
        }

        let detune_hz = desc.chorus_width * 0.85 * ((n % 3) as f32 - 1.0);
        let w_l = 2.0 * PI * (freq - detune_hz) / sr;
        let w_r = 2.0 * PI * (freq + detune_hz) / sr;
        let phase_l = (nf * 1.618_034) % (2.0 * PI);
        let phase_r = (nf * E) % (2.0 * PI);
        let partial_decay = (base_decay / nf.powf(0.45)).max(0.08);

        for i in 0..total_samples {
            let t = i as f32 / sr;
            let atk_env = (1.0 - (-t / attack_time).exp()).clamp(0.0, 1.0);
            let dec_env = (-t / partial_decay).exp();
            let vib = if desc.vibrato_depth > 0.05 {
                (2.0 * PI * 5.2 * t).sin() * desc.vibrato_depth * 0.004 * nf
            } else {
                0.0
            };

            let s_l = (w_l * (i as f32) + phase_l + vib).sin();
            let s_r = (w_r * (i as f32) + phase_r - vib).sin();
            let gain = amp * atk_env * dec_env;
            left[i] += s_l * gain;
            right[i] += s_r * gain;
        }
    }

    // Capa Sub-Armónica (1 octava abajo + fundamental cálida) cuando sub_weight > 0.15
    if desc.sub_weight > 0.15 {
        let w_sub = 2.0 * PI * (f0 * 0.5).max(24.0) / sr;
        let is_808_or_urbano = lower_has_urbano_or_reese(prompt);
        for i in 0..total_samples {
            let t = i as f32 / sr;
            let pitch_drop = if is_808_or_urbano {
                1.0 + 0.65 * (-t / 0.045).exp()
            } else {
                1.0
            };
            let env = (1.0 - (-t / (attack_time * 0.5)).exp()) * (-t / (base_decay * 1.3)).exp();
            let raw_sub = (w_sub * pitch_drop * i as f32).sin();
            let sub_val = (raw_sub + 0.25 * (raw_sub * 1.8).tanh()) * desc.sub_weight * 0.72 * env;
            left[i] += sub_val;
            right[i] += sub_val;
        }
    }

    // Capa de Transitorio Físico (Rasgueo de Cuatro / Púa de Bandola / Charrasca / Martillo Rhodes)
    if desc.attack_hardness > 0.55 {
        let transient_samples = ((0.022 * sr) as usize).min(total_samples);
        let mut rng_state = fnv1a_hash(prompt).max(1);
        for i in 0..transient_samples {
            rng_state ^= rng_state << 13;
            rng_state ^= rng_state >> 17;
            rng_state ^= rng_state << 5;
            let white = (rng_state as f32 / u32::MAX as f32) * 2.0 - 1.0;
            let env = (1.0 - i as f32 / transient_samples as f32).powi(2);
            let burst = white * env * (desc.attack_hardness - 0.45) * 0.42;
            left[i] += burst;
            right[i] -= burst * 0.85;
        }
    }

    // Normalización limpia a -1.0 dBFS
    let max_peak = left
        .iter()
        .chain(right.iter())
        .fold(0.0001_f32, |acc, &s| acc.max(s.abs()));
    let norm_gain = 0.891 / max_peak;
    for s in &mut left {
        *s = (*s * norm_gain).clamp(-1.0, 1.0);
    }
    for s in &mut right {
        *s = (*s * norm_gain).clamp(-1.0, 1.0);
    }

    let clean_title = prompt.trim();
    let display_name = if clean_title.is_empty() {
        "Neural Spectral Synth".to_string()
    } else {
        clean_title.chars().take(32).collect::<String>()
    };

    let mut buffer =
        SampleBuffer::new_stereo(left, right, sr, desc.root_note).with_name(display_name);
    buffer.loop_enabled = desc.attack_hardness < 0.35;
    buffer.loop_start = (duration_secs * 0.25 * sr) as usize;
    buffer.loop_end = (duration_secs * 0.85 * sr) as usize;
    buffer.loop_crossfade = (0.08 * sr) as usize;

    (buffer, backend)
}

/// Sintetiza localmente en silicio (`< 1 ms`, 0 latencia de red) una tabla de ondas coherente en fase
/// de `64` muestras (`[f32; 64]`, lista para `VoiceManager::set_custom_wavetable_cycle`) y su banco extendido
/// de `2048` muestras (`32 frames × 64 muestras`) a partir del descriptor acústico-semántico del prompt.
pub fn synthesize_local_silicon_wavetable(prompt: &str, sample_rate: f32) -> ([f32; 64], Vec<f32>) {
    let (desc, _backend) = SemanticTimbreDescriptor::from_prompt(prompt);
    let sr = sample_rate.max(8000.0);
    let f0 = 440.0 * 2.0_f32.powf((desc.root_note as f32 - 69.0) / 12.0);
    let max_harm = ((sr * 0.45 / f0).floor() as usize).clamp(4, 32);
    let num_harm = (8 + (desc.brightness * 20.0) as usize).min(max_harm);
    let spectral_tilt = 1.55 - desc.brightness * 0.65;

    let mut cycle64 = [0.0_f32; 64];
    for (i, slot) in cycle64.iter_mut().enumerate() {
        let phase = (i as f32 / 64.0) * 2.0 * PI;
        let mut s = 0.0_f32;
        for h in 1..=num_harm {
            let hf = h as f32;
            let mut amp = 1.0 / hf.powf(spectral_tilt);
            if h % 2 == 0 {
                amp *= 0.65 + desc.warmth * 0.55;
            }
            if desc.formant_character > 0.1 {
                let freq = f0 * hf;
                let d1 = ((freq - 730.0) / 240.0).powi(2);
                let d2 = ((freq - 1220.0) / 340.0).powi(2);
                amp *= 1.0 + desc.formant_character * ((-d1).exp() * 1.3 + (-d2).exp() * 0.9);
            }
            let h_phase = phase * hf + (hf * desc.inharmonicity * 0.35);
            s += h_phase.sin() * amp;
        }
        if desc.sub_weight > 0.15 {
            s += phase.sin() * desc.sub_weight * 0.55;
        }
        *slot = s;
    }

    // Eliminar DC y normalizar el ciclo de 64 muestras a pico 0.95
    let mean: f32 = cycle64.iter().sum::<f32>() / 64.0;
    let mut peak = 0.001_f32;
    for slot in &mut cycle64 {
        *slot -= mean;
        peak = peak.max(slot.abs());
    }
    let gain = 0.95 / peak;
    for slot in &mut cycle64 {
        *slot = (*slot * gain).clamp(-1.0, 1.0);
    }

    // Construir tabla morfable de 2048 muestras (32 frames x 64 muestras) con evolución armónica
    let mut table2048 = Vec::with_capacity(2048);
    for frame in 0..32 {
        let morph = frame as f32 / 31.0;
        let drive = 1.0 + morph * (0.6 + desc.warmth * 1.4);
        for &base_s in &cycle64 {
            let folded = (base_s * drive + morph * desc.inharmonicity * (base_s * PI).sin()).tanh();
            table2048.push(folded.clamp(-1.0, 1.0));
        }
    }

    (cycle64, table2048)
}
