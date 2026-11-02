//! Generador de modelado físico y síntesis modal de piano acústico y timbres instrumentales.

use std::f32::consts::{E, PI};
use synth_core::SampleBuffer;

/// Genera una muestra de piano de cola acústico afinado en C4 (261.63 Hz).
///
/// Utiliza síntesis modal aditiva con inharmonicidad de cuerda de piano real (Euler-Bernoulli):
/// fn = n * f0 * sqrt(1 + B * n^2)
/// e incluye unísono de 3 cuerdas ligeramente desafinadas para el clásico batimento de piano.
pub fn generate_grand_piano_c4(sample_rate: f32) -> SampleBuffer {
    let f0 = 261.6256; // C4
    let duration_secs = 3.5;
    let total_samples = (duration_secs * sample_rate) as usize;
    let b_inharmonicity = 0.0003; // Coeficiente de rigidez de la cuerda de piano

    let mut left = vec![0.0; total_samples];
    let mut right = vec![0.0; total_samples];

    // Armónicos del piano (hasta 30 armónicos audibles)
    let num_harmonics = 30;
    let detune_hz = 0.25; // Ligera desafinación entre cuerdas del coro del piano

    for n in 1..=num_harmonics {
        let n_f = n as f32;
        // Frecuencia inarmónica de cuerda rígida de piano
        let partial_freq = n_f * f0 * (1.0 + b_inharmonicity * n_f * n_f).sqrt();
        if partial_freq >= sample_rate * 0.49 {
            break;
        }

        // Amplitud base (caída espectral de 1/n^1.4)
        let amp_base = 1.0 / n_f.powf(1.35);

        // Tiempo de decaimiento: los armónicos agudos mueren mucho más rápido que los graves
        let decay_time = (2.8 / n_f.powf(0.8)).max(0.08);
        let decay_coeff = (-3.0 / (decay_time * sample_rate)).exp();

        let phase_l1 = (n as f32 * 1.618) % (2.0 * PI);
        let phase_l2 = (n as f32 * E) % (2.0 * PI);
        let phase_r1 = (n as f32 * PI) % (2.0 * PI);

        let freq_l = partial_freq;
        let freq_r = partial_freq + detune_hz * (n % 3 - 1) as f32;

        let w_l = 2.0 * PI * freq_l / sample_rate;
        let w_r = 2.0 * PI * freq_r / sample_rate;

        let mut current_amp = amp_base;
        for i in 0..total_samples {
            let i_f = i as f32;
            let sample_l = (w_l * i_f + phase_l1).sin() * 0.6 + (w_l * i_f + phase_l2).sin() * 0.4;
            let sample_r = (w_r * i_f + phase_r1).sin();

            left[i] += sample_l * current_amp;
            right[i] += sample_r * current_amp;

            current_amp *= decay_coeff;
        }
    }

    // Añadir transitorio de impacto del martillo de fieltro (ruido resonante de 8 ms)
    let hammer_samples = (0.012 * sample_rate) as usize;
    let mut hammer_seed: u32 = 12345;
    for i in 0..hammer_samples.min(total_samples) {
        hammer_seed = hammer_seed.wrapping_mul(1103515245).wrapping_add(12345);
        let noise = ((hammer_seed >> 16) as f32 / 32768.0) - 1.0;
        let env = 1.0 - (i as f32 / hammer_samples as f32);
        let hammer = noise * env * env * 0.35;
        left[i] += hammer;
        right[i] += hammer;
    }

    // Normalización de pico para evitar distorsión (-1.0 dBFS)
    let mut max_peak: f32 = 0.0001;
    for &s in left.iter().chain(right.iter()) {
        let abs = s.abs();
        if abs > max_peak {
            max_peak = abs;
        }
    }

    let target_peak = 0.891; // ~ -1.0 dB
    let gain = target_peak / max_peak;
    for s in left.iter_mut() {
        *s *= gain;
    }
    for s in right.iter_mut() {
        *s *= gain;
    }

    let mut buffer = SampleBuffer::new_stereo(left, right, sample_rate, 60); // 60 = C4
    buffer.name = "Concert Grand Piano".to_string();
    buffer.loop_enabled = false; // Decaimiento acústico natural sin clicks de bucle artificial
    buffer.loop_start = (1.2 * sample_rate) as usize;
    buffer.loop_end = (2.8 * sample_rate) as usize;
    buffer.loop_crossfade = (0.15 * sample_rate) as usize;

    buffer
}

/// Genera una muestra procedural o neural-espectral según el prompt recibido, retornando también el nombre del backend ODD.
pub fn generate_sample_from_prompt_with_backend(
    prompt: &str,
    sample_rate: f32,
) -> (SampleBuffer, &'static str) {
    let lower = prompt.trim().to_lowercase();
    let explicit_note = crate::npu_synth::extract_midi_note_from_prompt(prompt);

    let (mut buf, backend) = if lower.is_empty()
        || lower.contains("piano")
        || lower.contains("grand")
    {
        (generate_grand_piano_c4(sample_rate), "Physical-Modal-Piano")
    } else if lower.contains("cuatro")
        || lower.contains("camburpinton")
        || lower.contains("camburpintón")
    {
        (
            generate_cuatro_venezolano(sample_rate),
            "Physical-Model-Preset",
        )
    } else if lower.contains("arpa llanera") || lower.contains("joropo") {
        (generate_arpa_llanera(sample_rate), "Physical-Model-Preset")
    } else if lower.contains("bandola") {
        (
            generate_bandola_llanera(sample_rate),
            "Physical-Model-Preset",
        )
    } else if lower.contains("maraca") || lower.contains("capacho") {
        (
            generate_maracas_llaneras(sample_rate),
            "Physical-Model-Preset",
        )
    } else if lower.contains("cumaco") || lower.contains("tambor") {
        (generate_tambor_cumaco(sample_rate), "Physical-Model-Preset")
    } else if lower.contains("quitiplas")
        || lower.contains("quitiplás")
        || lower.contains("bambu")
        || lower.contains("bambú")
    {
        (
            generate_quitiplas_barlovento(sample_rate),
            "Physical-Model-Preset",
        )
    } else if lower.contains("furruco") || lower.contains("gaita") {
        (
            generate_furruco_zuliano(sample_rate),
            "Physical-Model-Preset",
        )
    } else if lower.contains("brass") || lower.contains("horn") || lower.contains("trumpet") {
        (generate_vintage_brass(sample_rate), "Physical-Model-Preset")
    } else if lower.contains("cyber") || (lower.contains("pluck") && lower.contains("lead")) {
        (
            generate_cyber_pluck_lead(sample_rate),
            "Physical-Model-Preset",
        )
    } else if lower.contains("warm analog") || (lower.contains("pad") && lower.contains("analog")) {
        (generate_warm_pad(sample_rate), "Physical-Model-Preset")
    } else if lower.contains("fm bell") || (lower.contains("bell") && lower.contains("chime")) {
        (generate_bell(sample_rate), "Physical-Model-Preset")
    } else if lower.contains("808") || (lower.contains("sub") && lower.contains("bass")) {
        (generate_808_bass(sample_rate), "Physical-Model-Preset")
    } else if lower.contains("cello") || lower.contains("violin") {
        (generate_cello(sample_rate), "Physical-Model-Preset")
    } else {
        crate::npu_synth::generate_neural_spectral_sample(prompt, sample_rate)
    };

    if let Some(note) = explicit_note {
        buf.root_note = note;
    }

    (buf, backend)
}

/// Genera una muestra procedural o neural-espectral según el prompt recibido.
pub fn generate_sample_from_prompt(prompt: &str, sample_rate: f32) -> SampleBuffer {
    generate_sample_from_prompt_with_backend(prompt, sample_rate).0
}

/// Genera un pad analógico cálido con unísono estéreo
fn generate_warm_pad(sample_rate: f32) -> SampleBuffer {
    let f0 = 261.6256; // C4
    let duration_secs = 4.0;
    let total_samples = (duration_secs * sample_rate) as usize;
    let mut left = vec![0.0; total_samples];
    let mut right = vec![0.0; total_samples];

    // Detuned sawtooth / triangle chorus
    let detunes = [-0.6, -0.2, 0.2, 0.6];
    for &dt in &detunes {
        let freq = f0 + dt;
        let w = 2.0 * PI * freq / sample_rate;
        for i in 0..total_samples {
            let t = i as f32;
            let saw = 2.0 * ((w * t / (2.0 * PI)).fract() - 0.5);
            let env = (1.0 - (-3.0 * t / (0.8 * sample_rate)).exp()).min(1.0)
                * (-0.5 * t / (duration_secs * sample_rate)).exp();
            left[i] += saw * env * 0.25;
            right[i] += saw * env * 0.25;
        }
    }

    let mut buf = SampleBuffer::new_stereo(left, right, sample_rate, 60);
    buf.name = "Warm Analog Pad".to_string();
    buf.loop_enabled = true;
    buf.loop_start = (1.0 * sample_rate) as usize;
    buf.loop_end = (3.5 * sample_rate) as usize;
    buf.loop_crossfade = (0.2 * sample_rate) as usize;
    buf
}

/// Genera una campana metálica FM (típica de síntesis de percusión tonal)
fn generate_bell(sample_rate: f32) -> SampleBuffer {
    let carrier_f = 523.25; // C5
    let mod_ratio = 2.756;
    let mod_f = carrier_f * mod_ratio;
    let duration_secs = 2.5;
    let total_samples = (duration_secs * sample_rate) as usize;
    let mut left = vec![0.0; total_samples];
    let mut right = vec![0.0; total_samples];

    for i in 0..total_samples {
        let t = i as f32 / sample_rate;
        let mod_env = (-4.0 * t).exp() * 3.5;
        let mod_val = (2.0 * PI * mod_f * t).sin() * mod_env;
        let car_env = (-2.0 * t).exp() * 0.8;
        let out = (2.0 * PI * carrier_f * t + mod_val).sin() * car_env;
        left[i] = out;
        right[i] = out;
    }

    let mut buf = SampleBuffer::new_stereo(left, right, sample_rate, 72);
    buf.name = "Metallic FM Bell".to_string();
    buf
}

/// Genera un sub bass 808 analógico con envolvente de pitch rápida y saturación suave
fn generate_808_bass(sample_rate: f32) -> SampleBuffer {
    let base_f = 65.41; // C2 (root note 36)
    let duration_secs = 2.5;
    let total_samples = (duration_secs * sample_rate) as usize;
    let mut left = vec![0.0; total_samples];
    let mut right = vec![0.0; total_samples];

    let mut phase: f32 = 0.0;
    for i in 0..total_samples {
        let t = i as f32 / sample_rate;
        // Pitch drop inicial característico de 808
        let pitch_env = (-35.0 * t).exp() * 80.0;
        let instant_freq = base_f + pitch_env;
        phase += 2.0 * PI * instant_freq / sample_rate;

        // Amplitud con decaimiento natural
        let amp_env = (-1.2 * t).exp();
        let raw = phase.sin();

        // Saturación armónica suave estilo overdrive a válvulas
        let sat = (raw * 1.5).tanh() * 0.85 * amp_env;
        left[i] = sat;
        right[i] = sat;
    }

    let mut buf = SampleBuffer::new_stereo(left, right, sample_rate, 36); // C2
    buf.name = "Deep 808 Sub Bass".to_string();
    buf.loop_enabled = true;
    buf.loop_start = (0.5 * sample_rate) as usize;
    buf.loop_end = (2.0 * sample_rate) as usize;
    buf.loop_crossfade = (0.1 * sample_rate) as usize;
    buf
}

/// Genera un cello solista con fricción de arco y vibrato cálido
fn generate_cello(sample_rate: f32) -> SampleBuffer {
    let f0 = 130.81; // C3 (root note 48)
    let duration_secs = 3.0;
    let total_samples = (duration_secs * sample_rate) as usize;
    let mut left = vec![0.0; total_samples];
    let mut right = vec![0.0; total_samples];

    let num_harmonics = 12;
    for i in 0..total_samples {
        let t = i as f32 / sample_rate;
        // Vibrato que entra suavemente después de 0.3s
        let vibrato_depth = (1.0 - (-4.0 * t).exp()).max(0.0) * 0.015;
        let vibrato = (2.0 * PI * 5.2 * t).sin() * vibrato_depth;
        let current_f0 = f0 * (1.0 + vibrato);

        // Envolvente de arco (crecimiento suave y sostenimiento)
        let bow_env = (1.0 - (-6.0 * t).exp()).min(1.0) * (-0.4 * t).exp();

        let mut sample: f32 = 0.0;
        for h in 1..=num_harmonics {
            let h_f = h as f32;
            let harmonic_f = current_f0 * h_f;
            if harmonic_f < sample_rate * 0.48 {
                let amp = (1.0 / h_f.powf(1.15)) * (-0.05 * h_f * t).exp();
                sample += (2.0 * PI * harmonic_f * t).sin() * amp;
            }
        }

        let out = sample * bow_env * 0.28;
        left[i] = out;
        right[i] = out;
    }

    let mut buf = SampleBuffer::new_stereo(left, right, sample_rate, 48); // C3
    buf.name = "Solo Acoustic Cello".to_string();
    buf.loop_enabled = true;
    buf.loop_start = (0.8 * sample_rate) as usize;
    buf.loop_end = (2.5 * sample_rate) as usize;
    buf.loop_crossfade = (0.15 * sample_rate) as usize;
    buf
}

/// Genera un ensamble de vientos metales (Vintage Brass Ensemble Horns) con apertura espectral ("swell")
fn generate_vintage_brass(sample_rate: f32) -> SampleBuffer {
    let f0 = 261.6256; // C4 (root note 60)
    let duration_secs = 3.0;
    let total_samples = (duration_secs * sample_rate) as usize;
    let mut left = vec![0.0; total_samples];
    let mut right = vec![0.0; total_samples];

    let num_harmonics = 16;
    for i in 0..total_samples {
        let t = i as f32 / sample_rate;
        // Envolvente de soplo de metal (ataque rápido de 45ms con ligero bloom armónico)
        let amp_env = (1.0 - (-28.0 * t).exp()).min(1.0) * (-0.35 * t).exp();
        let brightness_swell = (1.0 - (-12.0 * t).exp()).min(1.0);

        let mut sig_l: f32 = 0.0;
        let mut sig_r: f32 = 0.0;
        for h in 1..=num_harmonics {
            let h_f = h as f32;
            let freq_l = (f0 - 0.35) * h_f;
            let freq_r = (f0 + 0.35) * h_f;
            if freq_r < sample_rate * 0.48 {
                // Los armónicos medios-altos (3..8) se abren con el soplo del metal
                let formant = (-((h_f - 4.0 * brightness_swell).powi(2)) / 14.0).exp();
                let partial_amp = (formant / h_f.powf(0.65)) * 0.22;
                sig_l += (2.0 * PI * freq_l * t + h_f * 0.3).sin() * partial_amp;
                sig_r += (2.0 * PI * freq_r * t + h_f * 0.7).sin() * partial_amp;
            }
        }

        left[i] = (sig_l * amp_env * 1.3).tanh() * 0.85;
        right[i] = (sig_r * amp_env * 1.3).tanh() * 0.85;
    }

    let mut buf = SampleBuffer::new_stereo(left, right, sample_rate, 60);
    buf.name = "Vintage Brass Horns".to_string();
    buf.loop_enabled = true;
    buf.loop_start = (0.7 * sample_rate) as usize;
    buf.loop_end = (2.4 * sample_rate) as usize;
    buf.loop_crossfade = (0.15 * sample_rate) as usize;
    buf
}

/// Genera un sintetizador Cyberpunk Pluck Lead con ataque percusivo FM y cola supersaw estéreo
fn generate_cyber_pluck_lead(sample_rate: f32) -> SampleBuffer {
    let f0 = 261.6256; // C4 (root note 60)
    let duration_secs = 2.2;
    let total_samples = (duration_secs * sample_rate) as usize;
    let mut left = vec![0.0; total_samples];
    let mut right = vec![0.0; total_samples];

    for i in 0..total_samples {
        let t = i as f32 / sample_rate;
        let pluck_env = (-8.5 * t).exp();
        let sustain_tail = (-1.6 * t).exp() * 0.45;
        let env = (pluck_env * 0.75 + sustain_tail).min(1.0);

        // Modulación FM brillante en el transitorio inicial (primeros 60ms)
        let fm_index = (-22.0 * t).exp() * 2.8;
        let mod_sig = (2.0 * PI * (f0 * 2.0) * t).sin() * fm_index;

        // Supersaw estéreo desafinada
        let saw_l = 2.0 * ((f0 * 0.996 * t).fract() - 0.5) + (2.0 * PI * f0 * t + mod_sig).sin();
        let saw_r =
            2.0 * ((f0 * 1.004 * t).fract() - 0.5) + (2.0 * PI * (f0 * 1.001) * t + mod_sig).sin();

        left[i] = (saw_l * 0.55 * env).tanh() * 0.88;
        right[i] = (saw_r * 0.55 * env).tanh() * 0.88;
    }

    let mut buf = SampleBuffer::new_stereo(left, right, sample_rate, 60);
    buf.name = "Cyberpunk Pluck Lead".to_string();
    buf.loop_enabled = false;
    buf
}

/// Genera un Cuatro Venezolano tradicional (cuerda de nylon y caja resonante de cedro/caoba)
fn generate_cuatro_venezolano(sample_rate: f32) -> SampleBuffer {
    let f0 = 261.6256; // C4 (root note 60)
    let duration_secs = 2.6;
    let total_samples = (duration_secs * sample_rate) as usize;
    let mut left = vec![0.0; total_samples];
    let mut right = vec![0.0; total_samples];

    let mut noise_seed: u32 = 0x56_45_4E_34;
    for i in 0..total_samples {
        let t = i as f32 / sample_rate;
        // Ataque de uña/charrasqueo cálido sobre cuerda de nylon y resonancia de tapa de cedro
        let body_env = (-2.6 * t).exp();
        let pluck_transient = (-28.0 * t).exp();

        noise_seed = noise_seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let strum_noise = (((noise_seed >> 16) as f32 / 32768.0) - 1.0) * (-55.0 * t).exp() * 0.25;

        let mut l_sig = strum_noise;
        let mut r_sig = strum_noise * 0.9;

        for h in 1..=14 {
            let h_f = h as f32;
            let partial_decay = (-t * (1.8 + h_f * 0.55)).exp();
            // Formante cálida de caja de Cuatro (énfasis en armónicos 1..5)
            let wood_resonance = if h <= 4 { 1.15 } else { 0.85 / h_f.powf(1.25) };
            let amp = wood_resonance * partial_decay * (0.7 + 0.3 * pluck_transient);
            let freq = f0 * h_f;
            if freq < sample_rate * 0.48 {
                l_sig += (2.0 * PI * (freq - 0.18) * t + h_f * 0.4).sin() * amp;
                r_sig += (2.0 * PI * (freq + 0.18) * t + h_f * 0.9).sin() * amp;
            }
        }

        left[i] = (l_sig * body_env * 0.52).tanh() * 0.89;
        right[i] = (r_sig * body_env * 0.52).tanh() * 0.89;
    }

    let mut buf = SampleBuffer::new_stereo(left, right, sample_rate, 60);
    buf.name = "Cuatro Venezolano".to_string();
    buf.loop_enabled = false;
    buf
}

/// Genera un Arpa Llanera Venezolana (cuerdas cristalinas con resonancia de caja llanera)
fn generate_arpa_llanera(sample_rate: f32) -> SampleBuffer {
    let f0 = 261.6256; // C4 (root note 60)
    let duration_secs = 3.0;
    let total_samples = (duration_secs * sample_rate) as usize;
    let mut left = vec![0.0; total_samples];
    let mut right = vec![0.0; total_samples];

    for i in 0..total_samples {
        let t = i as f32 / sample_rate;
        let env = (-1.9 * t).exp();
        let snap = (-24.0 * t).exp();

        let mut l_sig = 0.0_f32;
        let mut r_sig = 0.0_f32;
        for h in 1..=18 {
            let h_f = h as f32;
            let freq = f0 * h_f * (1.0 + 0.00018 * h_f * h_f).sqrt();
            if freq < sample_rate * 0.48 {
                let h_decay = (-t * (1.2 + h_f * 0.42)).exp();
                let amp = (1.0 / h_f.powf(1.12)) * h_decay * (0.65 + 0.35 * snap);
                l_sig += (2.0 * PI * (freq - 0.22) * t).sin() * amp;
                r_sig += (2.0 * PI * (freq + 0.22) * t + 0.35 * h_f).sin() * amp;
            }
        }

        left[i] = (l_sig * env * 0.48).tanh() * 0.89;
        right[i] = (r_sig * env * 0.48).tanh() * 0.89;
    }

    let mut buf = SampleBuffer::new_stereo(left, right, sample_rate, 60);
    buf.name = "Arpa Llanera Venezolana".to_string();
    buf.loop_enabled = false;
    buf
}

/// Genera una Bandola Llanera Venezolana (4 órdenes, ataque de pajuela/plectro con jalao y caja periforme profunda en A3 = 220 Hz)
pub fn generate_bandola_llanera(sample_rate: f32) -> SampleBuffer {
    let f0 = 220.0; // A3 (root note 57, primera cuerda/bordón central llanero)
    let duration_secs = 2.8;
    let total_samples = (duration_secs * sample_rate) as usize;
    let mut left = vec![0.0; total_samples];
    let mut right = vec![0.0; total_samples];

    let mut pick_seed: u32 = 0xBA_4D_01_A3;
    for i in 0..total_samples {
        let t = i as f32 / sample_rate;
        let body_env = (-2.3 * t).exp();
        let pajuela_snap = (-38.0 * t).exp();

        pick_seed = pick_seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let pick_click = (((pick_seed >> 16) as f32 / 32768.0) - 1.0) * (-65.0 * t).exp() * 0.30;

        let mut l_sig = pick_click;
        let mut r_sig = pick_click * 0.92;

        for h in 1..=16 {
            let h_f = h as f32;
            let partial_decay = (-t * (1.5 + h_f * 0.48)).exp();
            // Formante nasal y profunda de caja de Bandola Llanera (énfasis en 2do, 3er y 5to armónico del pajueleo)
            let formant = if h == 2 || h == 3 || h == 5 {
                1.28 / h_f.powf(0.85)
            } else {
                0.92 / h_f.powf(1.18)
            };
            let amp = formant * partial_decay * (0.68 + 0.32 * pajuela_snap);
            let freq = f0 * h_f * (1.0 + 0.00022 * h_f * h_f).sqrt();
            if freq < sample_rate * 0.48 {
                l_sig += (2.0 * PI * (freq - 0.15) * t + h_f * 0.25).sin() * amp;
                r_sig += (2.0 * PI * (freq + 0.15) * t + h_f * 0.65).sin() * amp;
            }
        }

        left[i] = (l_sig * body_env * 0.54).tanh() * 0.89;
        right[i] = (r_sig * body_env * 0.54).tanh() * 0.89;
    }

    let mut buf = SampleBuffer::new_stereo(left, right, sample_rate, 57); // A3
    buf.name = "Bandola Llanera Venezolana".to_string();
    buf.loop_enabled = false;
    buf
}

/// Genera Maracas Llaneras Venezolanas / Capachos (impacto granular de semillas de capacho dentro de tapara resonante)
pub fn generate_maracas_llaneras(sample_rate: f32) -> SampleBuffer {
    let duration_secs = 0.85;
    let total_samples = (duration_secs * sample_rate) as usize;
    let mut left = vec![0.0; total_samples];
    let mut right = vec![0.0; total_samples];

    let mut seed_l: u32 = 0xCA_9A_C4_01;
    let mut seed_r: u32 = 0x4A_8A_CA_02;
    let mut bp_l1 = 0.0_f32;
    let mut bp_l2 = 0.0_f32;
    let mut bp_r1 = 0.0_f32;
    let mut bp_r2 = 0.0_f32;

    for i in 0..total_samples {
        let t = i as f32 / sample_rate;
        // Doble golpe característico de floreo de capachos (ataque seco principal + rebote de semillas a los 55ms)
        let main_hit = (t / 0.004).min(1.0) * (-26.0 * t).exp();
        let rebound = if t > 0.048 {
            ((t - 0.048) / 0.005).min(1.0) * (-22.0 * (t - 0.048)).exp() * 0.45
        } else {
            0.0
        };
        let env = (main_hit + rebound).min(1.0);

        seed_l = seed_l.wrapping_mul(1664525).wrapping_add(1013904223);
        seed_r = seed_r.wrapping_mul(22695477).wrapping_add(1);

        let white_l = ((seed_l >> 16) as f32 / 32768.0) - 1.0;
        let white_r = ((seed_r >> 16) as f32 / 32768.0) - 1.0;

        // Filtrado de alta frecuencia + resonancia hueca de tapara (~1.8 kHz y ~4.2 kHz)
        let diff_l = white_l - bp_l1 * 0.72 + bp_l2 * 0.22;
        bp_l2 = bp_l1;
        bp_l1 = white_l;

        let diff_r = white_r - bp_r1 * 0.70 + bp_r2 * 0.24;
        bp_r2 = bp_r1;
        bp_r1 = white_r;

        let gourd_ring = (2.0 * PI * 1480.0 * t).sin() * (-34.0 * t).exp() * 0.22
            + (2.0 * PI * 3120.0 * t).sin() * (-45.0 * t).exp() * 0.18;

        left[i] = ((diff_l * 0.75 + gourd_ring) * env * 1.35).tanh() * 0.88;
        right[i] = ((diff_r * 0.75 + gourd_ring * 0.95) * env * 1.35).tanh() * 0.88;
    }

    let mut buf = SampleBuffer::new_stereo(left, right, sample_rate, 60); // C4
    buf.name = "Maracas Llaneras Capachos".to_string();
    buf.loop_enabled = false;
    buf
}

/// Genera un Tambor Cumaco Venezolano de la Costa (parche de cuero grave en C3 = 130.81 Hz + golpe de laures sobre madera)
pub fn generate_tambor_cumaco(sample_rate: f32) -> SampleBuffer {
    let f0 = 130.8128; // C3 (root note 48)
    let duration_secs = 1.6;
    let total_samples = (duration_secs * sample_rate) as usize;
    let mut left = vec![0.0; total_samples];
    let mut right = vec![0.0; total_samples];

    let mut wood_seed: u32 = 0xC0_4A_C0_77;
    let mut phase_1 = 0.0_f32;
    let mut phase_2 = 0.0_f32;
    let mut phase_3 = 0.0_f32;

    for i in 0..total_samples {
        let t = i as f32 / sample_rate;
        // Caída de tono del cuero al impacto de la mano abierta
        let pitch_drop = 1.0 + 0.65 * (-32.0 * t).exp();
        let f_1 = f0 * pitch_drop;
        // Modos circulares de membrana de tambor (Bessel: 1.00, 1.59, 2.14)
        let f_2 = f0 * 1.593 * (1.0 + 0.35 * (-40.0 * t).exp());
        let f_3 = f0 * 2.136;

        phase_1 += 2.0 * PI * f_1 / sample_rate;
        phase_2 += 2.0 * PI * f_2 / sample_rate;
        phase_3 += 2.0 * PI * f_3 / sample_rate;

        let membrane = phase_1.sin() * (-3.8 * t).exp() * 0.78
            + phase_2.sin() * (-9.5 * t).exp() * 0.35
            + phase_3.sin() * (-16.0 * t).exp() * 0.22;

        // Golpe de "laures" (palos sobre el costado de madera del cumaco)
        wood_seed = wood_seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let noise = ((wood_seed >> 16) as f32 / 32768.0) - 1.0;
        let laures_click =
            ((2.0 * PI * 980.0 * t).sin() * 0.6 + noise * 0.4) * (-55.0 * t).exp() * 0.42;

        left[i] = ((membrane + laures_click * 1.08) * 1.15).tanh() * 0.90;
        right[i] = ((membrane + laures_click * 0.92) * 1.15).tanh() * 0.90;
    }

    let mut buf = SampleBuffer::new_stereo(left, right, sample_rate, 48); // C3
    buf.name = "Tambor Cumaco Venezolano".to_string();
    buf.loop_enabled = false;
    buf
}

/// Genera Quitiplás de Bambú de Barlovento (tubos de bambú percutidos contra el suelo y entrechocando en C4 = 261.63 Hz)
pub fn generate_quitiplas_barlovento(sample_rate: f32) -> SampleBuffer {
    let f0 = 261.6256; // C4 (root note 60)
    let duration_secs = 1.1;
    let total_samples = (duration_secs * sample_rate) as usize;
    let mut left = vec![0.0; total_samples];
    let mut right = vec![0.0; total_samples];

    let mut clack_seed: u32 = 0xB4_81_0B_E7;
    for i in 0..total_samples {
        let t = i as f32 / sample_rate;
        // Primer golpe hueco contra el suelo (Pujao) + entrechoque de bambú a los 42 ms (Prima)
        let ground_env = (-14.0 * t).exp();
        let clack_env = if t > 0.042 {
            (-22.0 * (t - 0.042)).exp() * 0.65
        } else {
            0.0
        };

        clack_seed = clack_seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let bamboo_noise = (((clack_seed >> 16) as f32 / 32768.0) - 1.0) * (-60.0 * t).exp() * 0.28;

        // Modos de tubo cilíndrico semi-cerrado (impares: 1, 3, 5 + quinta del segundo tubo)
        let tube_pujao = (2.0 * PI * f0 * t).sin() * 0.65
            + (2.0 * PI * (f0 * 3.0) * t).sin() * (-24.0 * t).exp() * 0.28
            + (2.0 * PI * (f0 * 5.0) * t).sin() * (-38.0 * t).exp() * 0.14;
        let tube_prima =
            (2.0 * PI * (f0 * 1.498) * t).sin() * 0.55 + (2.0 * PI * (f0 * 4.49) * t).sin() * 0.25;

        let l_raw = tube_pujao * ground_env + tube_prima * clack_env * 0.85 + bamboo_noise;
        let r_raw =
            tube_pujao * ground_env * 0.90 + tube_prima * clack_env * 1.05 + bamboo_noise * 0.95;

        left[i] = (l_raw * 1.25).tanh() * 0.89;
        right[i] = (r_raw * 1.25).tanh() * 0.89;
    }

    let mut buf = SampleBuffer::new_stereo(left, right, sample_rate, 60); // C4
    buf.name = "Quitiplas de Barlovento".to_string();
    buf.loop_enabled = false;
    buf
}

/// Genera un Furruco Zuliano Sub-Bass (fricción de verada encerada sobre cuero grave en C2 = 65.41 Hz)
pub fn generate_furruco_zuliano(sample_rate: f32) -> SampleBuffer {
    let f0 = 65.4064; // C2 (root note 36)
    let duration_secs = 2.8;
    let total_samples = (duration_secs * sample_rate) as usize;
    let mut left = vec![0.0; total_samples];
    let mut right = vec![0.0; total_samples];

    let mut phase = 0.0_f32;
    for i in 0..total_samples {
        let t = i as f32 / sample_rate;
        let attack = (t / 0.035).min(1.0);
        let decay = (-0.75 * t).exp();
        let env = attack * decay;

        // Modulación de fricción stick-slip de la verada encerada
        let stick_slip = 1.0 + 0.018 * (2.0 * PI * 14.0 * t).sin();
        phase += 2.0 * PI * (f0 * stick_slip) / sample_rate;

        let sub_fund = phase.sin() * 0.78;
        let second_harm = (phase * 2.0 + 0.4).sin() * 0.28;
        let third_growl = (phase * 3.0).sin() * (0.14 + 0.08 * (2.0 * PI * 7.0 * t).cos());

        let sig = (sub_fund + second_harm + third_growl) * env;
        left[i] = (sig * 1.22).tanh() * 0.90;
        right[i] = ((sig + (phase * 2.0 - 0.4).sin() * 0.05 * env) * 1.22).tanh() * 0.90;
    }

    let mut buf = SampleBuffer::new_stereo(left, right, sample_rate, 36); // C2
    buf.name = "Furruco Zuliano Sub".to_string();
    buf.loop_enabled = true;
    buf.loop_start = (0.35 * sample_rate) as usize;
    buf.loop_end = (2.10 * sample_rate) as usize;
    buf
}
