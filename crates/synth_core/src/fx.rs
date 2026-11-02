//! Rack de Efectos Estéreo en Tiempo Real (`StereoFxRack`):
//! Chorus Analógico en Cuadratura + Space Stereo Cross-Delay + Reverberación Algorítmica "Cañón del Tepui" (Schroeder/Freeverb 4-Comb + 2-AllPass).
//!
//! 100% prealocado en inicialización (`0` asignaciones de memoria en el hilo de audio)
//! y protegido contra números subnormales (denormales) e inestabilidades numéricas.

use std::f32::consts::PI;

const CHORUS_BUF_LEN: usize = 8192; // Potencia de 2 (~42 ms a 192 kHz)
const CHORUS_MASK: usize = CHORUS_BUF_LEN - 1;

const DELAY_BUF_LEN: usize = 131_072; // Potencia de 2 (~682 ms a 192 kHz)
const DELAY_MASK: usize = DELAY_BUF_LEN - 1;

const COMB_BASE_TUNINGS_44K: [usize; 4] = [1116, 1188, 1277, 1356];
const ALLPASS_BASE_TUNINGS_44K: [usize; 2] = [556, 441];
const STEREO_SPREAD_44K: usize = 23;
const MAX_SR_SCALE: usize = 5; // Soporta hasta >192 kHz sin realocar

#[inline(always)]
fn flush_denormal(x: f32) -> f32 {
    if !x.is_finite() || x.abs() < 1.0e-15 {
        0.0
    } else {
        x
    }
}

#[derive(Debug, Clone)]
struct CombFilter {
    buf: Vec<f32>,
    active_len: usize,
    idx: usize,
    filter_store: f32,
}

impl CombFilter {
    fn new(base_len_44k: usize, sample_rate: f32) -> Self {
        let max_len = (base_len_44k * MAX_SR_SCALE).max(64);
        let active_len =
            ((base_len_44k as f32 * (sample_rate / 44100.0)).round() as usize).clamp(32, max_len);
        Self {
            buf: vec![0.0; max_len],
            active_len,
            idx: 0,
            filter_store: 0.0,
        }
    }

    fn set_sample_rate(&mut self, base_len_44k: usize, sample_rate: f32) {
        let max_len = self.buf.len();
        self.active_len =
            ((base_len_44k as f32 * (sample_rate / 44100.0)).round() as usize).clamp(32, max_len);
        if self.idx >= self.active_len {
            self.idx = 0;
        }
    }

    fn reset(&mut self) {
        self.buf.fill(0.0);
        self.idx = 0;
        self.filter_store = 0.0;
    }

    #[inline(always)]
    fn process(&mut self, input: f32, feedback: f32, damp: f32) -> f32 {
        let output = flush_denormal(self.buf[self.idx]);
        self.filter_store = flush_denormal(output * (1.0 - damp) + self.filter_store * damp);
        self.buf[self.idx] = flush_denormal(input + self.filter_store * feedback);
        self.idx += 1;
        if self.idx >= self.active_len {
            self.idx = 0;
        }
        output
    }
}

#[derive(Debug, Clone)]
struct AllPassFilter {
    buf: Vec<f32>,
    active_len: usize,
    idx: usize,
}

impl AllPassFilter {
    fn new(base_len_44k: usize, sample_rate: f32) -> Self {
        let max_len = (base_len_44k * MAX_SR_SCALE).max(32);
        let active_len =
            ((base_len_44k as f32 * (sample_rate / 44100.0)).round() as usize).clamp(16, max_len);
        Self {
            buf: vec![0.0; max_len],
            active_len,
            idx: 0,
        }
    }

    fn set_sample_rate(&mut self, base_len_44k: usize, sample_rate: f32) {
        let max_len = self.buf.len();
        self.active_len =
            ((base_len_44k as f32 * (sample_rate / 44100.0)).round() as usize).clamp(16, max_len);
        if self.idx >= self.active_len {
            self.idx = 0;
        }
    }

    fn reset(&mut self) {
        self.buf.fill(0.0);
        self.idx = 0;
    }

    #[inline(always)]
    fn process(&mut self, input: f32) -> f32 {
        let buf_out = flush_denormal(self.buf[self.idx]);
        let output = -input + buf_out;
        self.buf[self.idx] = flush_denormal(input + buf_out * 0.5);
        self.idx += 1;
        if self.idx >= self.active_len {
            self.idx = 0;
        }
        flush_denormal(output)
    }
}

#[derive(Debug, Clone)]
pub struct StereoFxRack {
    sample_rate: f32,
    target_mix: f32,
    current_mix: f32,

    chorus_buf_l: Vec<f32>,
    chorus_buf_r: Vec<f32>,
    chorus_write: usize,
    lfo_phase: f32,

    delay_buf_l: Vec<f32>,
    delay_buf_r: Vec<f32>,
    delay_write: usize,
    lp_state_l: f32,
    lp_state_r: f32,

    comb_l: [CombFilter; 4],
    comb_r: [CombFilter; 4],
    allpass_l: [AllPassFilter; 2],
    allpass_r: [AllPassFilter; 2],
    reverb_tail_energy: f32,
}

impl Default for StereoFxRack {
    fn default() -> Self {
        Self::new(44100.0)
    }
}

impl StereoFxRack {
    /// Crea un nuevo `StereoFxRack` prealocando las líneas de retardo circulares y el tanque de reverberación en el heap.
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8000.0);
        Self {
            sample_rate: sr,
            target_mix: 0.15,
            current_mix: 0.15,
            chorus_buf_l: vec![0.0; CHORUS_BUF_LEN],
            chorus_buf_r: vec![0.0; CHORUS_BUF_LEN],
            chorus_write: 0,
            lfo_phase: 0.0,
            delay_buf_l: vec![0.0; DELAY_BUF_LEN],
            delay_buf_r: vec![0.0; DELAY_BUF_LEN],
            delay_write: 0,
            lp_state_l: 0.0,
            lp_state_r: 0.0,
            comb_l: [
                CombFilter::new(COMB_BASE_TUNINGS_44K[0], sr),
                CombFilter::new(COMB_BASE_TUNINGS_44K[1], sr),
                CombFilter::new(COMB_BASE_TUNINGS_44K[2], sr),
                CombFilter::new(COMB_BASE_TUNINGS_44K[3], sr),
            ],
            comb_r: [
                CombFilter::new(COMB_BASE_TUNINGS_44K[0] + STEREO_SPREAD_44K, sr),
                CombFilter::new(COMB_BASE_TUNINGS_44K[1] + STEREO_SPREAD_44K, sr),
                CombFilter::new(COMB_BASE_TUNINGS_44K[2] + STEREO_SPREAD_44K, sr),
                CombFilter::new(COMB_BASE_TUNINGS_44K[3] + STEREO_SPREAD_44K, sr),
            ],
            allpass_l: [
                AllPassFilter::new(ALLPASS_BASE_TUNINGS_44K[0], sr),
                AllPassFilter::new(ALLPASS_BASE_TUNINGS_44K[1], sr),
            ],
            allpass_r: [
                AllPassFilter::new(ALLPASS_BASE_TUNINGS_44K[0] + STEREO_SPREAD_44K, sr),
                AllPassFilter::new(ALLPASS_BASE_TUNINGS_44K[1] + STEREO_SPREAD_44K, sr),
            ],
            reverb_tail_energy: 0.0,
        }
    }

    /// Actualiza la frecuencia de muestreo del procesador sin realocar memoria.
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        if sample_rate > 0.0 {
            let sr = sample_rate.max(8000.0);
            self.sample_rate = sr;
            for (i, &base_len) in COMB_BASE_TUNINGS_44K.iter().enumerate() {
                self.comb_l[i].set_sample_rate(base_len, sr);
                self.comb_r[i].set_sample_rate(base_len + STEREO_SPREAD_44K, sr);
            }
            for (i, &base_len) in ALLPASS_BASE_TUNINGS_44K.iter().enumerate() {
                self.allpass_l[i].set_sample_rate(base_len, sr);
                self.allpass_r[i].set_sample_rate(base_len + STEREO_SPREAD_44K, sr);
            }
        }
    }

    /// Establece el nivel de mezcla húmeda/seca (`0.0` = Bypass 100% Dry, `1.0` = Máxima dimensión espacial).
    #[inline]
    pub fn set_mix(&mut self, mix: f32) {
        self.target_mix = if mix.is_finite() {
            mix.clamp(0.0, 1.0)
        } else {
            0.0
        };
    }

    /// Retorna el valor objetivo de mezcla actual.
    #[inline]
    pub fn mix(&self) -> f32 {
        self.target_mix
    }

    /// Limpia todas las líneas de retardo e integradores (ej. en `Plugin::reset()`).
    pub fn reset(&mut self) {
        self.chorus_buf_l.fill(0.0);
        self.chorus_buf_r.fill(0.0);
        self.delay_buf_l.fill(0.0);
        self.delay_buf_r.fill(0.0);
        self.lp_state_l = 0.0;
        self.lp_state_r = 0.0;
        for c in &mut self.comb_l {
            c.reset();
        }
        for c in &mut self.comb_r {
            c.reset();
        }
        for a in &mut self.allpass_l {
            a.reset();
        }
        for a in &mut self.allpass_r {
            a.reset();
        }
        self.reverb_tail_energy = 0.0;
    }

    #[inline(always)]
    fn read_ring_interpolated(
        buf: &[f32],
        mask: usize,
        write_idx: usize,
        delay_samples: f32,
    ) -> f32 {
        let clamped_delay = delay_samples.clamp(1.0, (mask - 2) as f32);
        let int_delay = clamped_delay.floor() as usize;
        let frac = clamped_delay - (int_delay as f32);

        let idx0 = write_idx.wrapping_sub(int_delay) & mask;
        let idx1 = write_idx.wrapping_sub(int_delay + 1) & mask;
        buf[idx0] * (1.0 - frac) + buf[idx1] * frac
    }

    /// Procesa un bloque estéreo in-place con 0 asignaciones de memoria.
    pub fn process_block(&mut self, left: &mut [f32], right: &mut [f32]) {
        let frames = left.len().min(right.len());
        if frames == 0 {
            return;
        }

        // Bypass ultrarrápido cuando el efecto está completamente cerrado y las colas están silenciosas
        if self.target_mix <= 0.0005
            && self.current_mix <= 0.0005
            && self.lp_state_l.abs() < 1.0e-6
            && self.lp_state_r.abs() < 1.0e-6
            && self.reverb_tail_energy < 1.0e-6
        {
            self.current_mix = 0.0;
            return;
        }

        let sr = self.sample_rate.max(8000.0);
        let lfo_inc = (2.0 * PI * 0.65) / sr; // 0.65 Hz Chorus LFO
        let base_chorus_samples = 0.012 * sr; // 12 ms centro
        let chorus_depth_samples = 0.0035 * sr; // ±3.5 ms modulación

        let delay_l_samples = (0.187 * sr).min((DELAY_BUF_LEN - 8) as f32); // 187 ms L
        let delay_r_samples = (0.263 * sr).min((DELAY_BUF_LEN - 8) as f32); // 263 ms R (ratio áureo espacial)
        let feedback = 0.34_f32;
        let damp = 0.35_f32; // Filtro paso-bajo cálido en las repeticiones

        let rev_feedback = 0.84_f32; // Decaimiento acústico tipo Cañón de Tepui / Sala de Concierto
        let rev_damp = 0.28_f32;

        for i in 0..frames {
            // Suavizado por muestra del parámetro mix para evitar zipper noise
            self.current_mix += (self.target_mix - self.current_mix) * 0.005;
            let mix = self.current_mix;

            let dry_l = flush_denormal(left[i]);
            let dry_r = flush_denormal(right[i]);

            // 1. Chorus Analógico Estéreo en Cuadratura (L = sin, R = cos)
            let mod_l = self.lfo_phase.sin();
            let mod_r = self.lfo_phase.cos();
            self.lfo_phase += lfo_inc;
            if self.lfo_phase >= 2.0 * PI {
                self.lfo_phase -= 2.0 * PI;
            }

            let cw = self.chorus_write;
            self.chorus_buf_l[cw] = dry_l;
            self.chorus_buf_r[cw] = dry_r;

            let ch_l = Self::read_ring_interpolated(
                &self.chorus_buf_l,
                CHORUS_MASK,
                cw,
                base_chorus_samples + mod_l * chorus_depth_samples,
            );
            let ch_r = Self::read_ring_interpolated(
                &self.chorus_buf_r,
                CHORUS_MASK,
                cw,
                base_chorus_samples + mod_r * chorus_depth_samples,
            );
            self.chorus_write = (cw + 1) & CHORUS_MASK;

            // 2. Space Cross-Delay Difuso con Damping Analógico
            let dw = self.delay_write;
            let tap_l =
                Self::read_ring_interpolated(&self.delay_buf_l, DELAY_MASK, dw, delay_l_samples);
            let tap_r =
                Self::read_ring_interpolated(&self.delay_buf_r, DELAY_MASK, dw, delay_r_samples);

            self.lp_state_l = flush_denormal(tap_l * (1.0 - damp) + self.lp_state_l * damp);
            self.lp_state_r = flush_denormal(tap_r * (1.0 - damp) + self.lp_state_r * damp);

            // Realimentación cruzada ping-pong suave para abrir la imagen estéreo
            let fb_in_l = flush_denormal((dry_l + ch_l * 0.5) * 0.5 + self.lp_state_r * feedback);
            let fb_in_r = flush_denormal((dry_r + ch_r * 0.5) * 0.5 + self.lp_state_l * feedback);

            self.delay_buf_l[dw] = fb_in_l;
            self.delay_buf_r[dw] = fb_in_r;
            self.delay_write = (dw + 1) & DELAY_MASK;

            // 3. Tanque de Reverberación Algorítmica "Cañón del Tepui" (4 Comb Paralelos + 2 AllPass en Serie)
            let rev_in_l = (dry_l + ch_l * 0.35 + self.lp_state_l * 0.25) * 0.18;
            let rev_in_r = (dry_r + ch_r * 0.35 + self.lp_state_r * 0.25) * 0.18;

            let mut sum_comb_l = 0.0_f32;
            let mut sum_comb_r = 0.0_f32;
            for c in &mut self.comb_l {
                sum_comb_l += c.process(rev_in_l, rev_feedback, rev_damp);
            }
            for c in &mut self.comb_r {
                sum_comb_r += c.process(rev_in_r, rev_feedback, rev_damp);
            }

            let ap0_l = self.allpass_l[0].process(sum_comb_l);
            let diff_l = self.allpass_l[1].process(ap0_l);
            let ap0_r = self.allpass_r[0].process(sum_comb_r);
            let diff_r = self.allpass_r[1].process(ap0_r);

            self.reverb_tail_energy = flush_denormal(
                self.reverb_tail_energy * 0.995 + (diff_l.abs() + diff_r.abs()) * 0.005,
            );

            let wet_l = ch_l * 0.42 + self.lp_state_l * 0.48 + diff_l * 0.58;
            let wet_r = ch_r * 0.42 + self.lp_state_r * 0.48 + diff_r * 0.58;

            left[i] = dry_l * (1.0 - mix * 0.30) + wet_l * mix;
            right[i] = dry_r * (1.0 - mix * 0.30) + wet_r * mix;
        }
    }
}
