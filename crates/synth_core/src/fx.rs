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
    bpm: f32,
    arp_mode: crate::arp::ArpMode,

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

    // Vital / Serum FX: Tube Drive Distortion + OTT Upward/Downward Multiband Compressor + Multi-Mode Bus (Canyon / 6-Stage Phaser / Analog Tape)
    drive_amount: f32,
    current_drive: f32,
    ott_amount: f32,
    current_ott: f32,
    ott_env_l: f32,
    ott_env_r: f32,
    air_lp_l: f32,
    air_lp_r: f32,
    fx_mode: u8, // 0 = Canyon Chorus/Reverb, 1 = 6-Stage Stereo Phaser, 2 = Analog Tape Wow/Flutter
    phaser_ap_l: [f32; 6],
    phaser_ap_r: [f32; 6],
    phaser_fb_l: f32,
    phaser_fb_r: f32,
    flutter_phase: f32,
    vital_fx_frames: u32,
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
            bpm: 120.0,
            arp_mode: crate::arp::ArpMode::Off,
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
            drive_amount: 0.0,
            current_drive: 0.0,
            ott_amount: 0.0,
            current_ott: 0.0,
            ott_env_l: 0.0,
            ott_env_r: 0.0,
            air_lp_l: 0.0,
            air_lp_r: 0.0,
            fx_mode: 0,
            phaser_ap_l: [0.0; 6],
            phaser_ap_r: [0.0; 6],
            phaser_fb_l: 0.0,
            phaser_fb_r: 0.0,
            flutter_phase: 0.0,
            vital_fx_frames: 0,
        }
    }

    /// Configura el modo de carácter del bus espacial (`0` = Canyon, `1` = 6-Stage Stereo Phaser, `2` = Analog Tape Flutter).
    #[inline]
    pub fn set_fx_mode(&mut self, mode: u8) {
        self.fx_mode = mode.min(2);
    }

    #[inline]
    pub fn fx_mode(&self) -> u8 {
        self.fx_mode
    }

    /// Configura la saturación de tubo / distorsión armónica (`0.0..=1.0`) estilo Serum/Vital.
    #[inline]
    pub fn set_drive(&mut self, drive: f32) {
        self.drive_amount = if drive.is_finite() {
            drive.clamp(0.0, 1.0)
        } else {
            0.0
        };
    }

    #[inline]
    pub fn drive(&self) -> f32 {
        self.drive_amount
    }

    /// Configura la profundidad del compresor multibanda ascendente/descendente OTT (`0.0..=1.0`).
    #[inline]
    pub fn set_ott(&mut self, ott: f32) {
        self.ott_amount = if ott.is_finite() {
            ott.clamp(0.0, 1.0)
        } else {
            0.0
        };
    }

    #[inline]
    pub fn ott(&self) -> f32 {
        self.ott_amount
    }

    #[inline]
    pub fn vital_fx_frames(&self) -> u32 {
        self.vital_fx_frames
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

    /// Sincroniza las subdivisiones del delay estéreo ping-pong al tempo (BPM) y al compás activo (`6/8 Joropo`, `5/8 Merengue`, `4/4`).
    #[inline]
    pub fn set_tempo_sync(&mut self, bpm: f32, arp_mode: crate::arp::ArpMode) {
        if bpm.is_finite() && bpm > 20.0 {
            self.bpm = bpm.clamp(40.0, 260.0);
        }
        self.arp_mode = arp_mode;
    }

    /// Retorna los tiempos de retardo actuales `(left_ms, right_ms)` en milisegundos según la sincronización de tempo y compás (ODD).
    #[inline]
    pub fn delay_times_ms(&self) -> (f32, f32) {
        let beat_sec = 60.0 / self.bpm.clamp(40.0, 260.0);
        let (sec_l, sec_r) = match self.arp_mode {
            crate::arp::ArpMode::Joropo => (beat_sec * (1.0 / 3.0), beat_sec * (2.0 / 3.0)),
            crate::arp::ArpMode::Merengue => (beat_sec * 0.40, beat_sec * 0.60),
            _ => (beat_sec * 0.375, beat_sec * 0.525),
        };
        (sec_l * 1000.0, sec_r * 1000.0)
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
        self.phaser_ap_l = [0.0; 6];
        self.phaser_ap_r = [0.0; 6];
        self.phaser_fb_l = 0.0;
        self.phaser_fb_r = 0.0;
        self.flutter_phase = 0.0;
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

        // Bypass ultrarrápido cuando todos los efectos están completamente cerrados y las colas están silenciosas
        if self.target_mix <= 0.0005
            && self.current_mix <= 0.0005
            && self.drive_amount <= 0.0005
            && self.current_drive <= 0.0005
            && self.ott_amount <= 0.0005
            && self.current_ott <= 0.0005
            && self.fx_mode == 0
            && self.lp_state_l.abs() < 1.0e-6
            && self.lp_state_r.abs() < 1.0e-6
            && self.reverb_tail_energy < 1.0e-6
        {
            self.current_mix = 0.0;
            self.current_drive = 0.0;
            self.current_ott = 0.0;
            return;
        }

        if self.drive_amount > 0.001
            || self.current_drive > 0.001
            || self.ott_amount > 0.001
            || self.current_ott > 0.001
            || self.fx_mode > 0
        {
            self.vital_fx_frames = self.vital_fx_frames.wrapping_add(frames as u32);
        }

        let sr = self.sample_rate.max(8000.0);
        let lfo_inc = (2.0 * PI * 0.65) / sr; // 0.65 Hz Chorus / Phaser LFO
        let flutter_inc = (2.0 * PI * 5.4) / sr; // 5.4 Hz Analog Tape Flutter
        let base_chorus_samples = 0.012 * sr; // 12 ms centro
        let chorus_depth_samples = 0.0035 * sr; // ±3.5 ms modulación

        let (delay_l_ms, delay_r_ms) = self.delay_times_ms();
        let delay_l_samples = ((delay_l_ms * 0.001) * sr).clamp(64.0, (DELAY_BUF_LEN - 8) as f32);
        let delay_r_samples = ((delay_r_ms * 0.001) * sr).clamp(64.0, (DELAY_BUF_LEN - 8) as f32);
        let feedback = if self.fx_mode == 2 { 0.44_f32 } else { 0.36_f32 };
        let damp = if self.fx_mode == 2 { 0.62_f32 } else { 0.35_f32 }; // Filtro paso-bajo más cálido en modo Tape

        let rev_feedback = 0.84_f32; // Decaimiento acústico tipo Cañón de Tepui / Sala de Concierto
        let rev_damp = 0.28_f32;

        for i in 0..frames {
            // Suavizado por muestra de mix, drive y ott para evitar zipper noise
            self.current_mix += (self.target_mix - self.current_mix) * 0.005;
            self.current_drive += (self.drive_amount - self.current_drive) * 0.012;
            self.current_ott += (self.ott_amount - self.current_ott) * 0.012;
            let mix = if self.fx_mode > 0 {
                self.current_mix.max(0.28)
            } else {
                self.current_mix
            };

            let mut dry_l = flush_denormal(left[i]);
            let mut dry_r = flush_denormal(right[i]);

            // 0. Etapa Pre-FX Vital/Serum: Tube Drive Distortion + OTT Upward/Downward Multiband Dynamics
            if self.current_drive > 0.001 {
                let d = self.current_drive;
                let pre = 1.0 + d * 4.2;
                let norm = 1.0 / (1.0 + d * 0.75);
                // Saturación asimétrica de válvula (agrega armónicos pares e impares)
                dry_l = ((dry_l * pre + d * 0.08).tanh() - (d * 0.08).tanh()) * norm;
                dry_r = ((dry_r * pre - d * 0.08).tanh() + (d * 0.08).tanh()) * norm;
            }

            if self.current_ott > 0.001 {
                let ott = self.current_ott;
                let abs_l = dry_l.abs();
                let abs_r = dry_r.abs();
                let coef = if abs_l > self.ott_env_l { 0.08 } else { 0.002 };
                self.ott_env_l = flush_denormal(self.ott_env_l + (abs_l - self.ott_env_l) * coef);
                self.ott_env_r = flush_denormal(self.ott_env_r + (abs_r - self.ott_env_r) * coef);

                // Compresión Ascendente (Upward Compression) + Excitador de banda aguda (Air Band)
                let up_l = 1.0 + ott * (0.38 / (self.ott_env_l + 0.08)).clamp(0.0, 2.2);
                let up_r = 1.0 + ott * (0.38 / (self.ott_env_r + 0.08)).clamp(0.0, 2.2);
                self.air_lp_l = flush_denormal(self.air_lp_l * 0.82 + dry_l * 0.18);
                self.air_lp_r = flush_denormal(self.air_lp_r * 0.82 + dry_r * 0.18);
                let air_l = (dry_l - self.air_lp_l) * (ott * 0.65);
                let air_r = (dry_r - self.air_lp_r) * (ott * 0.65);

                dry_l = ((dry_l * up_l + air_l) * (1.0 - ott * 0.15)).tanh();
                dry_r = ((dry_r * up_r + air_r) * (1.0 - ott * 0.15)).tanh();
            }

            // 1. Chorus Analógico Estéreo en Cuadratura (L = sin, R = cos) + Modos Phaser 6-Etapas / Tape Flutter
            let mod_l = self.lfo_phase.sin();
            let mod_r = self.lfo_phase.cos();
            self.lfo_phase += lfo_inc;
            if self.lfo_phase >= 2.0 * PI {
                self.lfo_phase -= 2.0 * PI;
            }
            self.flutter_phase += flutter_inc;
            if self.flutter_phase >= 2.0 * PI {
                self.flutter_phase -= 2.0 * PI;
            }
            let flutter = self.flutter_phase.sin() * 0.32;

            let cw = self.chorus_write;
            self.chorus_buf_l[cw] = dry_l;
            self.chorus_buf_r[cw] = dry_r;

            let tap_mod_l = if self.fx_mode == 2 {
                (mod_l * 1.35 + flutter) * chorus_depth_samples
            } else {
                mod_l * chorus_depth_samples
            };
            let tap_mod_r = if self.fx_mode == 2 {
                (mod_r * 1.35 - flutter) * chorus_depth_samples
            } else {
                mod_r * chorus_depth_samples
            };

            let mut ch_l = Self::read_ring_interpolated(
                &self.chorus_buf_l,
                CHORUS_MASK,
                cw,
                base_chorus_samples + tap_mod_l,
            );
            let mut ch_r = Self::read_ring_interpolated(
                &self.chorus_buf_r,
                CHORUS_MASK,
                cw,
                base_chorus_samples + tap_mod_r,
            );
            self.chorus_write = (cw + 1) & CHORUS_MASK;

            if self.fx_mode == 1 {
                // Modo 1 (PHASER): Cascada All-Pass Estéreo de 6 Etapas con Realimentación Resonante
                let coeff_l = (0.18 + 0.64 * (0.5 + 0.5 * mod_l)).clamp(0.10, 0.88);
                let coeff_r = (0.18 + 0.64 * (0.5 + 0.5 * mod_r)).clamp(0.10, 0.88);
                let mut x_l = dry_l + self.phaser_fb_l * 0.42;
                let mut x_r = dry_r + self.phaser_fb_r * 0.42;
                for st in 0..6 {
                    let prev_l = self.phaser_ap_l[st];
                    let y_l = flush_denormal(-coeff_l * x_l + prev_l);
                    self.phaser_ap_l[st] = flush_denormal(x_l + coeff_l * y_l);
                    x_l = y_l;

                    let prev_r = self.phaser_ap_r[st];
                    let y_r = flush_denormal(-coeff_r * x_r + prev_r);
                    self.phaser_ap_r[st] = flush_denormal(x_r + coeff_r * y_r);
                    x_r = y_r;
                }
                self.phaser_fb_l = x_l.clamp(-1.5, 1.5);
                self.phaser_fb_r = x_r.clamp(-1.5, 1.5);
                ch_l = ch_l * 0.35 + x_l * 0.85;
                ch_r = ch_r * 0.35 + x_r * 0.85;
            } else if self.fx_mode == 2 {
                // Modo 2 (TAPE): Saturación Cálida de Cabezal Magnético + Wow/Flutter
                ch_l = (ch_l * 1.45).tanh() * 0.88;
                ch_r = (ch_r * 1.45).tanh() * 0.88;
            }

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
