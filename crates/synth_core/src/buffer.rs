//! Representación optimizada de buffers de audio para samplers cromáticos.

use std::sync::Arc;

/// Buffer de audio en memoria para reproducción en el sampler.
/// Almacena canales planares (Left / Right) para máxima velocidad de lectura en la CPU.
#[derive(Debug, Clone)]
pub struct SampleBuffer {
    /// Nombre descriptivo del sample o timbre activo
    pub name: String,
    /// Datos de audio del canal izquierdo (o mono)
    pub left: Vec<f32>,
    /// Datos de audio del canal derecho (si es mono, apunta al mismo o vacío)
    pub right: Vec<f32>,
    /// Tasa de muestreo original del archivo de audio (ej. 44100.0 o 48000.0)
    pub sample_rate: f32,
    /// Nota MIDI de afinación original (60 = C4)
    pub root_note: u8,
    /// Desplazamiento fino en semitonos o cents (afinación precisa)
    pub tune_cents: f32,
    /// Punto inicial de reproducción (en muestras)
    pub start_sample: usize,
    /// Punto final de reproducción (en muestras, 0 = final del archivo)
    pub end_sample: usize,
    /// Habilitar bucle de sostenimiento
    pub loop_enabled: bool,
    /// Inicio del bucle (en muestras)
    pub loop_start: usize,
    /// Fin del bucle (en muestras)
    pub loop_end: usize,
    /// Longitud del crossfade en el bucle para eliminar clicks
    pub loop_crossfade: usize,
}

impl Default for SampleBuffer {
    fn default() -> Self {
        Self {
            name: "Grand Piano C4".to_string(),
            left: Vec::new(),
            right: Vec::new(),
            sample_rate: 44100.0,
            root_note: 60, // C4
            tune_cents: 0.0,
            start_sample: 0,
            end_sample: 0,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            loop_crossfade: 256,
        }
    }
}

impl SampleBuffer {
    /// Crea un nuevo buffer de audio estéreo a partir de canales planares.
    /// Incluye 2 muestras de guarda (sentinel padding) al final de cada canal
    /// para permitir interpolación Hermite de 4 puntos sin bifurcaciones condicionales.
    pub fn new_stereo(
        mut left: Vec<f32>,
        mut right: Vec<f32>,
        sample_rate: f32,
        root_note: u8,
    ) -> Self {
        let len = left.len().min(right.len());
        left.truncate(len);
        right.truncate(len);
        left.extend_from_slice(&[0.0, 0.0]);
        right.extend_from_slice(&[0.0, 0.0]);
        Self {
            name: "Custom Sample".to_string(),
            left,
            right,
            sample_rate,
            root_note,
            tune_cents: 0.0,
            start_sample: 0,
            end_sample: len,
            loop_enabled: false,
            loop_start: 0,
            loop_end: len,
            loop_crossfade: 256,
        }
    }

    /// Crea un buffer mono duplicando el canal a estéreo e incorporando 2 muestras de guarda (sentinel padding).
    pub fn new_mono(mut mono: Vec<f32>, sample_rate: f32, root_note: u8) -> Self {
        let len = mono.len();
        mono.extend_from_slice(&[0.0, 0.0]);
        Self {
            name: "Custom Sample".to_string(),
            left: mono.clone(),
            right: mono,
            sample_rate,
            root_note,
            tune_cents: 0.0,
            start_sample: 0,
            end_sample: len,
            loop_enabled: false,
            loop_start: 0,
            loop_end: len,
            loop_crossfade: 256,
        }
    }

    /// Alias de construcción mono con sentinel padding integrado.
    pub fn from_mono(mono: Vec<f32>, sample_rate: f32, root_note: u8) -> Self {
        Self::new_mono(mono, sample_rate, root_note)
    }

    /// Asigna un nombre descriptivo al buffer de muestra.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Retorna la cantidad total de muestras grabadas en el buffer (excluyendo el sentinel padding),
    /// independientemente de `end_sample`.
    #[inline(always)]
    pub fn raw_len(&self) -> usize {
        self.left.len().saturating_sub(2)
    }

    /// Retorna la cantidad de muestras audibles del sample (excluyendo el sentinel padding).
    #[inline(always)]
    pub fn len(&self) -> usize {
        let padded_len = self.raw_len();
        if self.end_sample > 0 {
            self.end_sample.min(padded_len)
        } else {
            padded_len
        }
    }

    /// Indica si el buffer no contiene muestras audibles.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Retorna la duración en segundos del sample audible.
    pub fn duration_seconds(&self) -> f32 {
        if self.sample_rate > 0.0 {
            self.len() as f32 / self.sample_rate
        } else {
            0.0
        }
    }

    /// Obtiene una muestra leída de forma segura sin peligro de pánico.
    #[inline(always)]
    pub fn get_sample(&self, channel: usize, index: usize) -> f32 {
        let chan = if channel == 0 {
            &self.left
        } else {
            &self.right
        };
        if index < chan.len() {
            chan[index]
        } else {
            0.0
        }
    }

    /// Detecta el índice de la primera muestra donde comienza el ataque útil del sonido
    /// superando `threshold_db` (por ejemplo `-42.0` dBFS), conservando un margen de pre-roll
    /// de hasta 8 muestras para no amputar el transitorio inicial.
    pub fn detect_onset_sample(&self, threshold_db: f32) -> usize {
        let total = self.raw_len();
        if total == 0 {
            return 0;
        }
        let linear_thresh = 10.0_f32.powf(threshold_db / 20.0).clamp(1e-5, 0.5);
        for i in 0..total {
            let l = self.left.get(i).copied().unwrap_or(0.0).abs();
            let r = self.right.get(i).copied().unwrap_or(0.0).abs();
            if l.max(r) >= linear_thresh {
                return i.saturating_sub(8);
            }
        }
        0
    }

    /// Detecta la frecuencia fundamental (`f0_hz`), la nota raíz MIDI más cercana (`root_note`)
    /// y la desviación en cents (`detune_cents`) junto con la confianza (`0.0..=1.0`)
    /// empleando el estimador YIN (Cumulative Mean Normalized Difference Function) con interpolación parabólica.
    pub fn detect_fundamental_pitch_hz(&self) -> Option<(f32, u8, f32, f32)> {
        let total = self.raw_len();
        if total < 512 || self.sample_rate < 8000.0 {
            return None;
        }

        let onset = self.detect_onset_sample(-40.0);
        // Saltar los primeros ~15 ms de transitorio percusivo si hay suficiente cola armónica
        let skip = ((self.sample_rate * 0.015) as usize).min(total.saturating_sub(256) / 4);
        let analysis_start = (onset + skip).min(total.saturating_sub(256));
        let avail = total.saturating_sub(analysis_start);
        if avail < 256 {
            return None;
        }

        // Rango de búsqueda: 45 Hz (F#1) .. 1400 Hz (F6)
        let tau_min = ((self.sample_rate / 1400.0).floor() as usize).max(4);
        let tau_max = ((self.sample_rate / 45.0).ceil() as usize)
            .min(avail / 2)
            .max(tau_min + 2);
        let win_len = (avail - tau_max).clamp(128, 1024);

        // Verificar que la ventana tenga energía suficiente
        let mut energy = 0.0f32;
        for i in 0..win_len {
            let idx = analysis_start + i;
            let mono = 0.5 * (self.left[idx] + self.right[idx]);
            energy += mono * mono;
        }
        if (energy / win_len as f32).sqrt() < 1e-4 {
            return None;
        }

        // Función de diferencia d(tau) y CMNDF d'(tau) de YIN
        let mut cmndf = vec![1.0f32; tau_max + 1];
        let mut running_sum = 0.0f32;

        for (tau, cmndf_slot) in cmndf.iter_mut().enumerate().take(tau_max + 1).skip(1) {
            let mut diff = 0.0f32;
            for i in 0..win_len {
                let idx = analysis_start + i;
                let s0 = 0.5 * (self.left[idx] + self.right[idx]);
                let s1 = 0.5 * (self.left[idx + tau] + self.right[idx + tau]);
                let delta = s0 - s1;
                diff += delta * delta;
            }
            running_sum += diff;
            if running_sum > 1e-12 {
                *cmndf_slot = diff * (tau as f32) / running_sum;
            } else {
                *cmndf_slot = 1.0;
            }
        }

        // Buscar el primer valle local debajo del umbral YIN (0.22), o el mínimo global
        let yin_thresh = 0.22f32;
        let mut best_tau = 0usize;
        for tau in tau_min..tau_max {
            if cmndf[tau] < yin_thresh {
                let mut t = tau;
                while t + 1 < tau_max && cmndf[t + 1] < cmndf[t] {
                    t += 1;
                }
                best_tau = t;
                break;
            }
        }

        if best_tau == 0 {
            let mut min_val = f32::MAX;
            for (tau, &val) in cmndf.iter().enumerate().take(tau_max).skip(tau_min) {
                if val < min_val {
                    min_val = val;
                    best_tau = tau;
                }
            }
        }

        if best_tau < tau_min || best_tau >= tau_max {
            return None;
        }

        let confidence = (1.0 - cmndf[best_tau]).clamp(0.0, 1.0);
        if confidence < 0.55 {
            return None;
        }

        // Interpolación parabólica alrededor de best_tau para precisión sub-muestra
        let y0 = cmndf[best_tau - 1];
        let y1 = cmndf[best_tau];
        let y2 = cmndf[best_tau + 1];
        let denom = 2.0 * (2.0 * y1 - y0 - y2);
        let frac = if denom.abs() > 1e-9 {
            ((y0 - y2) / denom).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        let refined_tau = (best_tau as f32 + frac).max(1.0);
        let f0_hz = self.sample_rate / refined_tau;
        if !(32.0..=2100.0).contains(&f0_hz) {
            return None;
        }

        let exact_midi = 69.0 + 12.0 * (f0_hz / 440.0).log2();
        let nearest_midi = exact_midi.round().clamp(12.0, 108.0) as u8;
        let detune_cents = (exact_midi - nearest_midi as f32) * 100.0;

        Some((f0_hz, nearest_midi, detune_cents, confidence))
    }

    /// Prepara un sample (generado por IA o importado) para ejecución cromática inmediata en un Piano Roll:
    /// 1. Recorta físicamente el silencio inicial (`Auto-Onset Trim` a `-42 dBFS`) con micro-fade de 16 muestras.
    /// 2. Detecta la frecuencia fundamental real `f0_hz` (YIN) y calibra `root_note` + `tune_cents` para afinación exacta.
    pub fn prepare_for_piano_roll(&mut self) -> PianoRollPrepReport {
        let orig_len = self.raw_len();
        if orig_len < 64 {
            return PianoRollPrepReport::default();
        }

        // 1. Auto-Onset Trim: eliminar silencio al inicio para respuesta en 0 ms en el Piano Roll
        let onset = self.detect_onset_sample(-42.0);
        let mut trimmed_samples = 0usize;
        if onset > 0 && orig_len.saturating_sub(onset) >= 64 {
            let new_len = orig_len - onset;
            let mut new_left = Vec::with_capacity(new_len + 2);
            let mut new_right = Vec::with_capacity(new_len + 2);
            new_left.extend_from_slice(&self.left[onset..orig_len]);
            new_right.extend_from_slice(&self.right[onset..orig_len]);

            // Micro-fade-in de 16 muestras para evitar clicks al inicio
            let fade_len = new_len.min(16);
            for i in 0..fade_len {
                let g = i as f32 / fade_len as f32;
                new_left[i] *= g;
                new_right[i] *= g;
            }

            // Sentinel padding de 2 muestras para Hermite 4-point
            new_left.extend_from_slice(&[0.0, 0.0]);
            new_right.extend_from_slice(&[0.0, 0.0]);

            self.left = new_left;
            self.right = new_right;
            self.start_sample = 0;
            self.end_sample = new_len;
            self.loop_start = self
                .loop_start
                .saturating_sub(onset)
                .min(new_len.saturating_sub(1));
            self.loop_end = self
                .loop_end
                .saturating_sub(onset)
                .clamp(self.loop_start + 1, new_len);
            trimmed_samples = onset;
        }

        // 2. Detección automática de Pitch Fundamental (YIN) y calibración de root_note + tune_cents
        if let Some((f0_hz, midi_note, detune_cents, confidence)) =
            self.detect_fundamental_pitch_hz()
        {
            self.root_note = midi_note;
            // Compensar desviación fraccionaria en cents para que al tocar en el Piano Roll suene 100% en A4=440Hz
            self.tune_cents = (-detune_cents).clamp(-50.0, 50.0);
            PianoRollPrepReport {
                trimmed_leading_samples: trimmed_samples,
                detected_f0_hz: f0_hz,
                detected_root_note: midi_note,
                fine_tune_cents: self.tune_cents,
                confidence,
                is_pitched: true,
            }
        } else {
            PianoRollPrepReport {
                trimmed_leading_samples: trimmed_samples,
                detected_f0_hz: 0.0,
                detected_root_note: self.root_note,
                fine_tune_cents: self.tune_cents,
                confidence: 0.0,
                is_pitched: false,
            }
        }
    }

    /// Extrae un ciclo armónico único de 64 puntos (`Audio-to-Wavetable Resynthesis`, estilo Vital/Serum)
    /// a partir del contenido de audio del `SampleBuffer`:
    /// 1. Estima el período fundamental exacto (`period_samples`) mediante YIN (`detect_fundamental_pitch_hz`)
    ///    o la nota raíz MIDI asignada.
    /// 2. Localiza la región de estado estacionario con mayor energía armónica (RMS).
    /// 3. Alinea el inicio del ciclo al cruce por cero ascendente (`s[i] <= 0 && s[i+1] > 0`) más cercano.
    /// 4. Remuestrea con interpolación cúbica Hermite de 4 puntos a 64 muestras, elimina componente DC
    ///    y normaliza el pico a `0.95` garantizando continuidad circular libre de clics.
    pub fn extract_wavetable_cycle_64(&self) -> [f32; 64] {
        let total = self.raw_len();
        if total < 16 {
            let mut fallback = [0.0_f32; 64];
            for (i, slot) in fallback.iter_mut().enumerate() {
                *slot = (i as f32 / 64.0 * std::f32::consts::TAU).sin() * 0.9;
            }
            return fallback;
        }

        let f0_hz = self
            .detect_fundamental_pitch_hz()
            .map(|(f0, _, _, _)| f0)
            .unwrap_or_else(|| {
                440.0 * 2.0_f32.powf((self.root_note.clamp(24, 96) as f32 - 69.0) / 12.0)
            });
        let period_f = (self.sample_rate.max(8000.0) / f0_hz.clamp(32.0, 2000.0))
            .clamp(16.0, (total as f32 * 0.45).max(16.0));
        let period_i = (period_f.round() as usize).clamp(16, total.saturating_sub(4).max(16));

        let mono_at = |idx: usize| -> f32 {
            let i = idx.min(total.saturating_sub(1));
            0.5 * (self.left[i] + self.right[i])
        };

        // Buscar ventana de mayor energía RMS en el cuerpo armónico de la muestra
        let onset = self.detect_onset_sample(-40.0);
        let search_start =
            (onset + (self.sample_rate * 0.012) as usize).min(total.saturating_sub(period_i + 4));
        let search_end = (search_start + (total / 2).max(period_i * 2))
            .min(total.saturating_sub(period_i + 4))
            .max(search_start + 1);

        let step = (period_i / 2).max(8);
        let mut best_win_start = search_start;
        let mut best_energy = -1.0_f32;
        let mut pos = search_start;
        while pos < search_end {
            let mut e = 0.0_f32;
            for k in 0..period_i {
                let s = mono_at(pos + k);
                e += s * s;
            }
            if e > best_energy {
                best_energy = e;
                best_win_start = pos;
            }
            pos += step;
        }

        // Alinear al cruce por cero ascendente más cercano a best_win_start
        let zc_limit = (best_win_start + period_i).min(total.saturating_sub(period_i + 3));
        let mut cycle_start = best_win_start;
        for idx in best_win_start..zc_limit {
            let s0 = mono_at(idx);
            let s1 = mono_at(idx + 1);
            if s0 <= 0.0 && s1 > 0.0 {
                cycle_start = idx;
                break;
            }
        }

        // Remuestrear exactamente `period_f` muestras a 64 puntos con Hermite 4p
        let mut cycle = [0.0_f32; 64];
        let mut dc_sum = 0.0_f32;
        for (i, slot) in cycle.iter_mut().enumerate() {
            let src_pos = cycle_start as f32 + (i as f32 / 64.0) * period_f;
            let idx0 = src_pos.floor() as usize;
            let frac = src_pos - idx0 as f32;
            let xm1 = mono_at(idx0.saturating_sub(1));
            let x0 = mono_at(idx0);
            let x1 = mono_at(idx0 + 1);
            let x2 = mono_at(idx0 + 2);
            let val = crate::resampler::hermite_interpolate_4p(xm1, x0, x1, x2, frac);
            *slot = if val.is_finite() { val } else { 0.0 };
            dc_sum += *slot;
        }

        // Eliminar DC offset y normalizar pico a 0.95
        let dc = dc_sum / 64.0;
        let mut max_abs = 0.0_f32;
        for slot in &mut cycle {
            *slot -= dc;
            max_abs = max_abs.max(slot.abs());
        }
        if max_abs > 1e-4 {
            let scale = 0.95 / max_abs;
            for slot in &mut cycle {
                *slot = (*slot * scale).clamp(-1.0, 1.0);
            }
        }

        // Suavizar cierre de fase en los extremos para bucle circular libre de clics
        cycle[0] = (cycle[0] * 0.5 + cycle[63] * 0.25 + cycle[1] * 0.25).clamp(-1.0, 1.0);
        cycle[63] = (cycle[63] * 0.5 + cycle[62] * 0.25 + cycle[0] * 0.25).clamp(-1.0, 1.0);
        cycle
    }
}

/// Reporte de calibración DSP `Piano-Roll Ready` (Auto-Onset Trim + Auto-Pitch YIN).
#[derive(Debug, Clone, Copy, Default)]
pub struct PianoRollPrepReport {
    /// Cantidad de muestras de silencio inicial recortadas para ataque en 0 ms
    pub trimmed_leading_samples: usize,
    /// Frecuencia fundamental detectada en Hz (`0.0` si es percusión no tonal)
    pub detected_f0_hz: f32,
    /// Nota raíz MIDI detectada y asignada al sample
    pub detected_root_note: u8,
    /// Corrección fina aplicada en cents (`-50..+50`)
    pub fine_tune_cents: f32,
    /// Confianza de periodicidad armónica YIN (`0.0..=1.0`)
    pub confidence: f32,
    /// Indica si el sample tiene altura tonal definida para seguimiento cromático
    pub is_pitched: bool,
}

/// Referencia compartida atómicamente al sample activo.
pub type SharedSampleBuffer = Arc<SampleBuffer>;
