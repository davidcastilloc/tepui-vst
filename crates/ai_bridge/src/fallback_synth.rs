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

/// Indica si el prompt corresponde a un generador DSP dedicado de alta fidelidad
/// (Karplus-Strong físico, Reese Bass, Supersaw, Acid 303, FM Pluck, Vocal Formant, 808, Piano Modal).
pub fn is_dedicated_dsp_preset(prompt: &str) -> bool {
    let lower = prompt.trim().to_lowercase();
    lower.is_empty()
        || lower.contains("piano")
        || lower.contains("grand")
        || lower.contains("reese")
        || lower.contains("supersaw")
        || lower.contains("acid")
        || lower.contains("303")
        || lower.contains("crystal glass fm")
        || lower.contains("fm pluck")
        || lower.contains("formant")
        || lower.contains("vocal pad")
        || lower.contains("choir")
        || lower.contains("karplus")
        || lower.contains("nylon guitar")
        || lower.contains("cuatro")
        || lower.contains("camburpinton")
        || lower.contains("camburpintón")
        || lower.contains("arpa llanera")
        || lower.contains("joropo")
        || lower.contains("bandola")
        || lower.contains("maraca")
        || lower.contains("capacho")
        || lower.contains("cumaco")
        || lower.contains("tambor")
        || lower.contains("quitiplas")
        || lower.contains("quitiplás")
        || lower.contains("bambu")
        || lower.contains("bambú")
        || lower.contains("furruco")
        || lower.contains("gaita")
        || lower.contains("brass")
        || lower.contains("horn")
        || lower.contains("trumpet")
        || lower.contains("cyber")
        || (lower.contains("pluck") && lower.contains("lead"))
        || lower.contains("warm analog")
        || (lower.contains("pad") && lower.contains("analog"))
        || lower.contains("fm bell")
        || (lower.contains("bell") && lower.contains("chime"))
        || lower.contains("808")
        || (lower.contains("sub") && lower.contains("bass"))
        || lower.contains("cello")
        || lower.contains("violin")
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
    } else if lower.contains("reese") {
        (
            generate_neuro_reese_bass(sample_rate),
            "Wavetable-Pro-Synth",
        )
    } else if lower.contains("supersaw") {
        (
            generate_supersaw_anthem_lead(sample_rate),
            "Wavetable-Pro-Synth",
        )
    } else if lower.contains("acid") || lower.contains("303") {
        (
            generate_acid_303_squelch(sample_rate),
            "Wavetable-Pro-Synth",
        )
    } else if lower.contains("crystal glass fm") || lower.contains("fm pluck") {
        (generate_fm_pluck_glass(sample_rate), "Wavetable-Pro-Synth")
    } else if lower.contains("formant") || lower.contains("vocal pad") {
        (
            generate_vocal_formant_pad(sample_rate),
            "Wavetable-Pro-Synth",
        )
    } else if lower.contains("cuatro")
        || lower.contains("camburpinton")
        || lower.contains("camburpintón")
        || lower.contains("karplus")
        || lower.contains("nylon guitar")
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

/// Motor físico Extended Karplus-Strong Digital Waveguide con:
/// - Filtro peine de posición de plectro/uña (`pick_pos`)
/// - Amortiguamiento pasabajos de puente (`brightness`)
/// - Dispersión all-pass de rigidez de cuerda (`dispersion`)
/// - Banco resonador modal de caja armónica de madera (3 modos biquad)
#[allow(clippy::too_many_arguments)]
fn karplus_strong_waveguide_stereo(
    sample_rate: f32,
    f0: f32,
    duration_secs: f32,
    pick_pos: f32,
    brightness: f32,
    loop_gain: f32,
    dispersion: f32,
    body_modes_hz: [f32; 3],
    seed_init: u32,
) -> (Vec<f32>, Vec<f32>) {
    let total_samples = (duration_secs * sample_rate) as usize;
    let mut left = vec![0.0_f32; total_samples];
    let mut right = vec![0.0_f32; total_samples];

    // Dos órdenes/cuerdas en estéreo ligeramente desafinadas para realismo acústico
    let freqs = [f0 * 0.9993, f0 * 1.0007];
    for (ch, &freq) in freqs.iter().enumerate() {
        let period = (sample_rate / freq).max(4.0);
        let delay_len = period.floor() as usize;
        let pick_offset = ((pick_pos.clamp(0.08, 0.45) * delay_len as f32) as usize).max(1);

        // Excitación inicial de plectro/uña (ruido rosa filtrado + impulso de ataque)
        let mut raw_exc = vec![0.0_f32; delay_len];
        let mut rng = seed_init.wrapping_add((ch as u32) * 0x9E37_79B9);
        let mut lp_exc = 0.0_f32;
        for (idx, val) in raw_exc.iter_mut().enumerate() {
            rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
            let white = ((rng >> 16) as f32 / 32768.0) - 1.0;
            lp_exc = lp_exc * 0.45 + white * 0.55;
            let pluck_shape = (PI * (idx as f32 / delay_len as f32)).sin();
            *val = lp_exc * 0.75 + pluck_shape * 0.35;
        }

        // Filtro peine de posición de pulsación (suprime múltiplos de 1/pick_pos)
        let mut delay_line = vec![0.0_f32; delay_len];
        for idx in 0..delay_len {
            let comb_idx = (idx + delay_len - pick_offset) % delay_len;
            delay_line[idx] = raw_exc[idx] - raw_exc[comb_idx] * 0.85;
        }

        let mut ptr = 0usize;
        let mut prev_out = 0.0_f32;
        let mut ap_x1 = 0.0_f32;
        let mut ap_y1 = 0.0_f32;

        // Estados de 3 resonadores modales de tapa de madera
        let mut body_y1 = [0.0_f32; 3];
        let mut body_y2 = [0.0_f32; 3];
        let body_r = [0.992_f32, 0.988_f32, 0.982_f32];
        let body_w: [f32; 3] = [
            2.0 * PI * body_modes_hz[0] / sample_rate,
            2.0 * PI * body_modes_hz[1] / sample_rate,
            2.0 * PI * body_modes_hz[2] / sample_rate,
        ];

        let out_buf = if ch == 0 { &mut left } else { &mut right };

        for sample_slot in out_buf.iter_mut() {
            let next_ptr = (ptr + 1) % delay_len;
            let y0 = delay_line[ptr];
            let y1 = delay_line[next_ptr];

            // Promedio de dos puntos + amortiguamiento de puente ajustable por brillo
            let avg = 0.5 * (y0 + y1);
            let damped = ((1.0 - brightness) * prev_out + brightness * avg) * loop_gain;
            prev_out = damped;

            // Filtro all-pass de dispersión de rigidez de cuerda
            let dispersed = dispersion * damped + ap_x1 - dispersion * ap_y1;
            ap_x1 = damped;
            ap_y1 = dispersed;

            delay_line[ptr] = dispersed;
            ptr = next_ptr;

            // Resonancia modal de la caja de madera (3 bandas)
            let mut wood_sum = 0.0_f32;
            for m in 0..3 {
                let r = body_r[m];
                let b_out =
                    (1.0 - r) * y0 + 2.0 * r * body_w[m].cos() * body_y1[m] - r * r * body_y2[m];
                body_y2[m] = body_y1[m];
                body_y1[m] = b_out;
                wood_sum += b_out * (0.36 - 0.07 * m as f32);
            }

            *sample_slot = y0 * 0.72 + wood_sum * 0.48;
        }
    }

    // Normalización limpia a -1.0 dBFS (0.891)
    let mut peak = 0.0001_f32;
    for &s in left.iter().chain(right.iter()) {
        peak = peak.max(s.abs());
    }
    let gain = 0.891 / peak;
    for s in left.iter_mut() {
        *s = (*s * gain * 1.08).tanh() * 0.89;
    }
    for s in right.iter_mut() {
        *s = (*s * gain * 1.08).tanh() * 0.89;
    }

    (left, right)
}

/// Genera un Cuatro Venezolano / Guitarra Acústica de Nylon mediante guía de onda digital Karplus-Strong.
fn generate_cuatro_venezolano(sample_rate: f32) -> SampleBuffer {
    let (left, right) = karplus_strong_waveguide_stereo(
        sample_rate,
        261.6256, // C4
        2.6,
        0.26,   // Pulsación cálida sobre boca de cedro
        0.72,   // Amortiguamiento de cuerda de nylon
        0.9962, // Sostenimiento natural de caja pequeña
        0.12,   // Dispersión suave de nylon
        [145.0, 315.0, 740.0],
        0x56_45_4E_34,
    );
    let mut buf = SampleBuffer::new_stereo(left, right, sample_rate, 60);
    buf.name = "Cuatro Venezolano".to_string();
    buf.loop_enabled = false;
    buf
}

/// Genera un Arpa Acústica mediante guía de onda digital Karplus-Strong con ataque cristalino.
fn generate_arpa_llanera(sample_rate: f32) -> SampleBuffer {
    let (left, right) = karplus_strong_waveguide_stereo(
        sample_rate,
        261.6256, // C4
        3.0,
        0.14,   // Pulsación brillante cerca del puente
        0.88,   // Alta tensión cristalina
        0.9978, // Sostenimiento largo de arpa
        0.22,   // Ligera inharmonicidad de cuerda tensa
        [120.0, 280.0, 890.0],
        0x41_52_50_41,
    );
    let mut buf = SampleBuffer::new_stereo(left, right, sample_rate, 60);
    buf.name = "Arpa Llanera Venezolana".to_string();
    buf.loop_enabled = false;
    buf
}

/// Genera una Bandola / Cuerda Pulsada con plectro mediante guía de onda digital Karplus-Strong en A3 (220 Hz).
pub fn generate_bandola_llanera(sample_rate: f32) -> SampleBuffer {
    let (left, right) = karplus_strong_waveguide_stereo(
        sample_rate,
        220.0, // A3
        2.8,
        0.17, // Ataque firme de pajuela/plectro
        0.82, // Brillo medio-alto con cuerpo profundo
        0.9970,
        0.18,
        [118.0, 265.0, 680.0],
        0xBA_4D_01_A3,
    );
    let mut buf = SampleBuffer::new_stereo(left, right, sample_rate, 57); // A3
    buf.name = "Bandola Llanera Venezolana".to_string();
    buf.loop_enabled = false;
    buf
}

/// Genera un Neuro Reese Bass de estudio (5 sierras desafinadas + sub-oscilador bloqueado en fase + barrido notch/lowpass en C2 = 65.41 Hz).
pub fn generate_neuro_reese_bass(sample_rate: f32) -> SampleBuffer {
    let f0 = 65.4064; // C2 (root note 36)
    let duration_secs = 3.2;
    let total_samples = (duration_secs * sample_rate) as usize;
    let mut left = vec![0.0_f32; total_samples];
    let mut right = vec![0.0_f32; total_samples];

    let detunes_hz = [-0.42_f32, -0.19, 0.0, 0.19, 0.42];
    let pan_l = [0.90_f32, 0.72, 0.70, 0.35, 0.18];
    let pan_r = [0.18_f32, 0.35, 0.70, 0.72, 0.90];
    let mut phases = [0.0_f32, 0.23, 0.51, 0.77, 0.12];
    let mut sub_phase = 0.0_f32;
    let mut lp_l = 0.0_f32;
    let mut lp_r = 0.0_f32;

    for i in 0..total_samples {
        let t = i as f32 / sample_rate;
        let amp_env = (t / 0.015).min(1.0);
        sub_phase = (sub_phase + f0 / sample_rate).fract();
        let sub_sin = (2.0 * PI * sub_phase).sin() * 0.58;

        let mut raw_l = sub_sin;
        let mut raw_r = sub_sin;
        for v in 0..5 {
            let freq = f0 + detunes_hz[v];
            phases[v] = (phases[v] + freq / sample_rate).fract();
            let saw = 2.0 * phases[v] - 1.0;
            // Wavefolding neuro sutil
            let folded = (saw * 1.65).sin();
            raw_l += folded * pan_l[v] * 0.28;
            raw_r += folded * pan_r[v] * 0.28;
        }

        // Movimiento orgánico del filtro Reese (LFO de 0.55 Hz + mordida inicial)
        let cutoff_mod = 0.22 + 0.16 * (2.0 * PI * 0.55 * t).sin() + 0.25 * (-5.0 * t).exp();
        let alpha = cutoff_mod.clamp(0.06, 0.65);
        lp_l += alpha * (raw_l - lp_l);
        lp_r += alpha * (raw_r - lp_r);

        left[i] = (lp_l * amp_env * 1.45).tanh() * 0.89;
        right[i] = (lp_r * amp_env * 1.45).tanh() * 0.89;
    }

    let mut buf = SampleBuffer::new_stereo(left, right, sample_rate, 36);
    buf.name = "Neuro Reese Bass".to_string();
    buf.loop_enabled = true;
    buf.loop_start = (0.6 * sample_rate) as usize;
    buf.loop_end = (2.8 * sample_rate) as usize;
    buf.loop_crossfade = (0.18 * sample_rate) as usize;
    buf
}

/// Genera un Hyper Supersaw Anthem Lead de 7 voces estéreo (estilo Vital / Serum / JP-8000 en C4 = 261.63 Hz).
pub fn generate_supersaw_anthem_lead(sample_rate: f32) -> SampleBuffer {
    let f0 = 261.6256; // C4 (root note 60)
    let duration_secs = 3.2;
    let total_samples = (duration_secs * sample_rate) as usize;
    let mut left = vec![0.0_f32; total_samples];
    let mut right = vec![0.0_f32; total_samples];

    let ratios = [0.9890_f32, 0.9938, 0.9980, 1.0000, 1.0020, 1.0062, 1.0110];
    let mut phases = [0.05_f32, 0.19, 0.37, 0.50, 0.68, 0.81, 0.94];
    let mut sub_phase = 0.0_f32;

    for i in 0..total_samples {
        let t = i as f32 / sample_rate;
        let env = (t / 0.008).min(1.0) * (0.78 + 0.22 * (-2.2 * t).exp());
        sub_phase = (sub_phase + (f0 * 0.5) / sample_rate).fract();
        let sub_osc = (2.0 * PI * sub_phase).sin() * 0.22;

        let mut sig_l = sub_osc;
        let mut sig_r = sub_osc;
        for v in 0..7 {
            phases[v] = (phases[v] + (f0 * ratios[v]) / sample_rate).fract();
            let saw = 2.0 * phases[v] - 1.0;
            let pan = v as f32 / 6.0;
            let gain_v = if v == 3 { 0.34 } else { 0.21 };
            sig_l += saw * (1.0 - pan * 0.65) * gain_v;
            sig_r += saw * (0.35 + pan * 0.65) * gain_v;
        }

        left[i] = (sig_l * env * 1.15).tanh() * 0.89;
        right[i] = (sig_r * env * 1.15).tanh() * 0.89;
    }

    let mut buf = SampleBuffer::new_stereo(left, right, sample_rate, 60);
    buf.name = "Hyper Supersaw Lead".to_string();
    buf.loop_enabled = true;
    buf.loop_start = (0.65 * sample_rate) as usize;
    buf.loop_end = (2.75 * sample_rate) as usize;
    buf.loop_crossfade = (0.16 * sample_rate) as usize;
    buf
}

/// Genera un Resonant Acid 303 Squelch Bass con filtro escalera de diodos resonante y saturación asimétrica en C2 (65.41 Hz).
pub fn generate_acid_303_squelch(sample_rate: f32) -> SampleBuffer {
    let f0 = 65.4064; // C2 (root note 36)
    let duration_secs = 2.0;
    let total_samples = (duration_secs * sample_rate) as usize;
    let mut left = vec![0.0_f32; total_samples];
    let mut right = vec![0.0_f32; total_samples];

    let mut phase = 0.0_f32;
    let mut ic1 = 0.0_f32;
    let mut ic2 = 0.0_f32;

    for i in 0..total_samples {
        let t = i as f32 / sample_rate;
        let amp_env = (t / 0.003).min(1.0) * (-1.65 * t).exp();
        // Barrido exponencial clásico de cutoff TB-303 (Accent + Env Mod)
        let cutoff_hz = 165.0 + 2350.0 * (-11.5 * t).exp() + 280.0 * (-2.8 * t).exp();
        let g = (PI * (cutoff_hz.min(sample_rate * 0.42)) / sample_rate).tan();
        let k = 0.22_f32; // Alta resonancia líquida (Q ~ 4.5)

        phase = (phase + f0 / sample_rate).fract();
        let saw_sq = (2.0 * phase - 1.0) * 0.65 + if phase < 0.5 { 0.35 } else { -0.35 };

        // Filtro SVF TPT 2-polos con saturación no lineal en el bucle de resonancia
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = saw_sq - ic2;
        let v1 = a1 * ic1 + a2 * v3;
        let v2 = ic2 + a2 * ic1 + a3 * v3;
        ic1 = (2.0 * v1 - ic1).clamp(-2.5, 2.5);
        ic2 = (2.0 * v2 - ic2).clamp(-2.5, 2.5);

        let acid_sig = ((v2 + v1 * 0.55) * 1.85).tanh() * amp_env * 0.89;
        left[i] = acid_sig;
        right[i] = (acid_sig * 0.96 + (v1 * 0.25 * amp_env)).clamp(-0.95, 0.95);
    }

    let mut buf = SampleBuffer::new_stereo(left, right, sample_rate, 36);
    buf.name = "Resonant Acid 303".to_string();
    buf.loop_enabled = false;
    buf
}

/// Genera un Crystal Glass FM Pluck de 3 operadores con ataque cristalino DX/Serum en C4 (261.63 Hz).
pub fn generate_fm_pluck_glass(sample_rate: f32) -> SampleBuffer {
    let f0 = 261.6256; // C4 (root note 60)
    let duration_secs = 2.4;
    let total_samples = (duration_secs * sample_rate) as usize;
    let mut left = vec![0.0_f32; total_samples];
    let mut right = vec![0.0_f32; total_samples];

    for i in 0..total_samples {
        let t = i as f32 / sample_rate;
        let car_env = (t / 0.002).min(1.0) * (-2.3 * t).exp();
        let mod1_env = (-9.5 * t).exp() * 2.6;
        let mod2_env = (-28.0 * t).exp() * 1.45;

        let mod2 = (2.0 * PI * (f0 * 7.0) * t).sin() * mod2_env;
        let mod1 = (2.0 * PI * (f0 * 2.0) * t + mod2).sin() * mod1_env;

        let car_l = (2.0 * PI * (f0 * 0.9992) * t + mod1).sin()
            + (2.0 * PI * (f0 * 2.0) * t).sin() * 0.22 * car_env;
        let car_r = (2.0 * PI * (f0 * 1.0008) * t + mod1 * 0.96).sin()
            + (2.0 * PI * (f0 * 2.001) * t + 0.4).sin() * 0.22 * car_env;

        left[i] = (car_l * car_env * 0.85).tanh() * 0.89;
        right[i] = (car_r * car_env * 0.85).tanh() * 0.89;
    }

    let mut buf = SampleBuffer::new_stereo(left, right, sample_rate, 60);
    buf.name = "Crystal Glass FM Pluck".to_string();
    buf.loop_enabled = false;
    buf
}

/// Genera un Ethereal Formant Vocal Pad con banco de formantes vocálicos (AAH <-> OOH) y coro estéreo en C4 (261.63 Hz).
pub fn generate_vocal_formant_pad(sample_rate: f32) -> SampleBuffer {
    let f0 = 261.6256; // C4 (root note 60)
    let duration_secs = 3.8;
    let total_samples = (duration_secs * sample_rate) as usize;
    let mut left = vec![0.0_f32; total_samples];
    let mut right = vec![0.0_f32; total_samples];

    for i in 0..total_samples {
        let t = i as f32 / sample_rate;
        let swell = (1.0 - (-3.5 * t).exp()).min(1.0) * (0.85 + 0.15 * (-0.4 * t).exp());
        let morph = 0.5 + 0.5 * (2.0 * PI * 0.35 * t).sin();

        // Interpolación entre formantes vocálicas AAH (730, 1090, 2440 Hz) y OOH (320, 890, 2250 Hz)
        let f1 = 320.0 + morph * 410.0;
        let f2 = 890.0 + morph * 200.0;
        let f3 = 2250.0 + morph * 190.0;

        let vib = 1.0 + 0.0035 * (2.0 * PI * 5.1 * t).sin();
        let mut sig_l = 0.0_f32;
        let mut sig_r = 0.0_f32;

        for h in 1..=22 {
            let h_f = h as f32;
            let freq = f0 * vib * h_f;
            if freq >= sample_rate * 0.46 {
                break;
            }
            let g1 = (-((freq - f1).powi(2)) / (2.0 * 95.0 * 95.0)).exp();
            let g2 = (-((freq - f2).powi(2)) / (2.0 * 140.0 * 140.0)).exp() * 0.85;
            let g3 = (-((freq - f3).powi(2)) / (2.0 * 220.0 * 220.0)).exp() * 0.55;
            let formant_gain = (g1 + g2 + g3 + 0.12 / h_f).min(1.35) / h_f.powf(0.55);

            sig_l += (2.0 * PI * (freq * 0.9985) * t + h_f * 0.37).sin() * formant_gain;
            sig_r += (2.0 * PI * (freq * 1.0015) * t + h_f * 0.83).sin() * formant_gain;
        }

        left[i] = (sig_l * swell * 0.38).tanh() * 0.88;
        right[i] = (sig_r * swell * 0.38).tanh() * 0.88;
    }

    let mut buf = SampleBuffer::new_stereo(left, right, sample_rate, 60);
    buf.name = "Ethereal Formant Vocal Pad".to_string();
    buf.loop_enabled = true;
    buf.loop_start = (0.85 * sample_rate) as usize;
    buf.loop_end = (3.30 * sample_rate) as usize;
    buf.loop_crossfade = (0.22 * sample_rate) as usize;
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
