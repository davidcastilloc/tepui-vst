//! Filtro de Variable de Estado (State Variable Filter - SVF) con topología TPT (Topology-Preserving Transform).
//! Ofrece estabilidad incondicional y cero retardo de retroalimentación (Zero-Delay Feedback).

use std::f32::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    Lowpass,
    Highpass,
    Bandpass,
    Ladder24dB,
    Comb,
    Formant,
    DualSerialHpLp,
    DualParallelLpBp,
    Bypass,
}

impl FilterMode {
    #[inline]
    pub fn from_index(idx: u8) -> Self {
        match idx {
            0 => Self::Lowpass,
            1 => Self::Highpass,
            2 => Self::Bandpass,
            3 => Self::Bypass,
            4 => Self::Ladder24dB,
            5 => Self::Comb,
            6 => Self::Formant,
            7 => Self::DualSerialHpLp,
            8 => Self::DualParallelLpBp,
            _ => Self::Lowpass,
        }
    }

    #[inline]
    pub fn to_index(self) -> u8 {
        match self {
            Self::Lowpass => 0,
            Self::Highpass => 1,
            Self::Bandpass => 2,
            Self::Bypass => 3,
            Self::Ladder24dB => 4,
            Self::Comb => 5,
            Self::Formant => 6,
            Self::DualSerialHpLp => 7,
            Self::DualParallelLpBp => 8,
        }
    }

    #[inline]
    pub fn is_advanced_model(self) -> bool {
        matches!(
            self,
            Self::Ladder24dB
                | Self::Comb
                | Self::Formant
                | Self::DualSerialHpLp
                | Self::DualParallelLpBp
        )
    }
}

#[derive(Debug, Clone)]
pub struct StateVariableFilter {
    sample_rate: f32,
    cutoff_hz: f32,
    resonance: f32,
    pub mode: FilterMode,

    // Estados internos integradores SVF 2-polos
    ic1eq: f32,
    ic2eq: f32,

    // Estados internos del filtro secundario para topología Dual Serie (HP -> Ladder LP)
    dual_hp_ic1: f32,
    dual_hp_ic2: f32,

    // Estados internos Ladder 24dB/oct (4 polos en cascada tipo Moog)
    ladder_s: [f32; 4],

    // Línea de retardo circular en pila para Comb Filter Karplus-Strong (0 heap allocations)
    comb_buf: [f32; 2048],
    comb_pos: usize,
    comb_lp: f32,
    comb_dirty: bool,

    // Estados y coeficientes de 3 resonadores paralelos para Formant Filter (A-E-I-O-U)
    formant_ic1: [f32; 3],
    formant_ic2: [f32; 3],
    formant_a1: [f32; 3],
    formant_a2: [f32; 3],
    formant_a3: [f32; 3],

    // Coeficientes precalculados SVF
    g: f32,
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
}

impl Default for StateVariableFilter {
    fn default() -> Self {
        let mut filter = Self {
            sample_rate: 44100.0,
            cutoff_hz: 20000.0,
            resonance: 0.707,
            mode: FilterMode::Lowpass,
            ic1eq: 0.0,
            ic2eq: 0.0,
            dual_hp_ic1: 0.0,
            dual_hp_ic2: 0.0,
            ladder_s: [0.0; 4],
            comb_buf: [0.0; 2048],
            comb_pos: 0,
            comb_lp: 0.0,
            comb_dirty: false,
            formant_ic1: [0.0; 3],
            formant_ic2: [0.0; 3],
            formant_a1: [0.0; 3],
            formant_a2: [0.0; 3],
            formant_a3: [0.0; 3],
            g: 0.0,
            k: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
        };
        filter.recalculate_coefficients();
        filter
    }
}

impl StateVariableFilter {
    pub fn new(sample_rate: f32) -> Self {
        let mut filter = Self {
            sample_rate,
            ..Default::default()
        };
        filter.recalculate_coefficients();
        filter
    }

    #[inline]
    fn fast_tanh(x: f32) -> f32 {
        let xc = x.clamp(-3.0, 3.0);
        let x2 = xc * xc;
        xc * (27.0 + x2) / (27.0 + 9.0 * x2)
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        if sample_rate > 0.0 {
            self.sample_rate = sample_rate;
            self.recalculate_coefficients();
        }
    }

    pub fn set_cutoff(&mut self, cutoff_hz: f32) {
        let clamped = cutoff_hz.clamp(20.0, (self.sample_rate * 0.49).min(20000.0));
        if (self.cutoff_hz - clamped).abs() > 0.01 {
            self.cutoff_hz = clamped;
            self.recalculate_coefficients();
        }
    }

    pub fn set_resonance(&mut self, resonance: f32) {
        let clamped = resonance.clamp(0.1, 10.0);
        if (self.resonance - clamped).abs() > 0.001 {
            self.resonance = clamped;
            self.recalculate_coefficients();
        }
    }

    fn recalculate_coefficients(&mut self) {
        // g = tan(pi * cutoff / sample_rate)
        let g = (PI * (self.cutoff_hz / self.sample_rate)).tan();
        let k = 1.0 / self.resonance;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;

        self.g = g;
        self.k = k;
        self.a1 = a1;
        self.a2 = a2;
        self.a3 = a3;

        // Precalcular formantes vocales A-E-I-O-U interpolados por la posición logarítmica de cutoff
        const VOWELS: [[f32; 3]; 5] = [
            [730.0, 1090.0, 2440.0], // A
            [530.0, 1840.0, 2480.0], // E
            [270.0, 2290.0, 3010.0], // I
            [570.0, 840.0, 2410.0],  // O
            [300.0, 870.0, 2240.0],  // U
        ];
        let norm_cut = ((self.cutoff_hz.ln() - 20.0_f32.ln()) / (20000.0_f32.ln() - 20.0_f32.ln()))
            .clamp(0.0, 1.0);
        let v_pos = norm_cut * 4.0;
        let v_idx = (v_pos.floor() as usize).min(3);
        let v_frac = v_pos - v_idx as f32;
        let q_formant = (4.5 + self.resonance * 1.4).clamp(4.0, 18.0);
        let k_f = 1.0 / q_formant;

        for (b, ((a1_slot, a2_slot), a3_slot)) in self
            .formant_a1
            .iter_mut()
            .zip(self.formant_a2.iter_mut())
            .zip(self.formant_a3.iter_mut())
            .enumerate()
        {
            let f_hz = (VOWELS[v_idx][b] * (1.0 - v_frac) + VOWELS[v_idx + 1][b] * v_frac)
                .clamp(120.0, (self.sample_rate * 0.45).min(12000.0));
            let gb = (PI * (f_hz / self.sample_rate)).tan();
            let a1b = 1.0 / (1.0 + gb * (gb + k_f));
            let a2b = gb * a1b;
            let a3b = gb * a2b;
            *a1_slot = a1b;
            *a2_slot = a2b;
            *a3_slot = a3b;
        }
    }

    pub fn reset(&mut self) {
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
        self.dual_hp_ic1 = 0.0;
        self.dual_hp_ic2 = 0.0;
        self.ladder_s = [0.0; 4];
        if self.comb_dirty {
            self.comb_buf = [0.0; 2048];
            self.comb_dirty = false;
        }
        self.comb_pos = 0;
        self.comb_lp = 0.0;
        self.formant_ic1 = [0.0; 3];
        self.formant_ic2 = [0.0; 3];
    }

    /// Retorna el estado interno actual de los integradores `(ic1eq, ic2eq)`.
    #[inline]
    pub fn integrator_states(&self) -> (f32, f32) {
        (self.ic1eq, self.ic2eq)
    }

    /// Procesa una muestra y devuelve la señal filtrada
    #[inline]
    pub fn process_sample(&mut self, input: f32) -> f32 {
        match self.mode {
            FilterMode::Bypass => input,
            FilterMode::Lowpass | FilterMode::Bandpass | FilterMode::Highpass => {
                let v3 = input - self.ic2eq;
                let v1 = self.a1 * self.ic1eq + self.a2 * v3;
                let v2 = self.ic2eq + self.a2 * self.ic1eq + self.a3 * v3;

                self.ic1eq = 2.0 * v1 - self.ic1eq;
                self.ic2eq = 2.0 * v2 - self.ic2eq;

                if self.ic1eq.abs() < 1e-15 {
                    self.ic1eq = 0.0;
                }
                if self.ic2eq.abs() < 1e-15 {
                    self.ic2eq = 0.0;
                }

                match self.mode {
                    FilterMode::Lowpass => v2,
                    FilterMode::Bandpass => v1,
                    FilterMode::Highpass => input - self.k * v1 - v2,
                    _ => v2,
                }
            }
            FilterMode::Ladder24dB => {
                // Topología Ladder 4-polos (24 dB/oct) con saturación no lineal tanh tipo Moog
                let g1 = self.g / (1.0 + self.g);
                let g2 = g1 * g1;
                let g3 = g2 * g1;
                let g4 = g2 * g2;
                let k_moog = ((self.resonance - 0.5) / 9.5).clamp(0.0, 1.0) * 3.85;
                let s_est = g3 * self.ladder_s[0]
                    + g2 * self.ladder_s[1]
                    + g1 * self.ladder_s[2]
                    + self.ladder_s[3];
                let fb = Self::fast_tanh(s_est);
                let u = Self::fast_tanh(input * 1.35 - k_moog * fb) / (1.0 + k_moog * g4);
                let mut x = u;
                for st in &mut self.ladder_s {
                    let v = (x - *st) * g1;
                    let y = v + *st;
                    *st = y + v;
                    if st.abs() < 1e-15 {
                        *st = 0.0;
                    }
                    x = y;
                }
                self.ic1eq = self.ladder_s[1];
                self.ic2eq = x;
                x * (1.0 + k_moog * 0.22)
            }
            FilterMode::Comb => {
                // Filtro Comb Karplus-Strong con interpolación fraccional y amortiguamiento LP en lazo
                self.comb_dirty = true;
                let delay_smp =
                    (self.sample_rate / self.cutoff_hz.clamp(25.0, 8000.0)).clamp(2.0, 2045.0);
                let d_int = delay_smp.floor() as usize;
                let d_frac = delay_smp - d_int as f32;
                let idx0 = (self.comb_pos + 2048 - d_int) & 2047;
                let idx1 = (self.comb_pos + 2048 - d_int - 1) & 2047;
                let delayed = self.comb_buf[idx0] * (1.0 - d_frac) + self.comb_buf[idx1] * d_frac;
                self.comb_lp = delayed * 0.72 + self.comb_lp * 0.28;
                if self.comb_lp.abs() < 1e-15 {
                    self.comb_lp = 0.0;
                }
                let fb = (0.58 + (self.resonance / 10.0) * 0.37).clamp(0.58, 0.96);
                let loop_sig = Self::fast_tanh(input + self.comb_lp * fb);
                self.comb_buf[self.comb_pos] = loop_sig;
                self.comb_pos = (self.comb_pos + 1) & 2047;
                self.ic1eq = delayed;
                self.ic2eq = loop_sig;
                input * 0.42 + delayed * 0.78
            }
            FilterMode::Formant => {
                // Resonador Vocal Paralelo (3 Bandas Formantes F1, F2, F3 A-E-I-O-U)
                const WEIGHTS: [f32; 3] = [1.0, 0.70, 0.42];
                let mut sum = 0.0_f32;
                for (b, &weight) in WEIGHTS.iter().enumerate() {
                    let v3 = input - self.formant_ic2[b];
                    let v1 = self.formant_a1[b] * self.formant_ic1[b] + self.formant_a2[b] * v3;
                    let v2 = self.formant_ic2[b]
                        + self.formant_a2[b] * self.formant_ic1[b]
                        + self.formant_a3[b] * v3;
                    self.formant_ic1[b] = 2.0 * v1 - self.formant_ic1[b];
                    self.formant_ic2[b] = 2.0 * v2 - self.formant_ic2[b];
                    if self.formant_ic1[b].abs() < 1e-15 {
                        self.formant_ic1[b] = 0.0;
                    }
                    if self.formant_ic2[b].abs() < 1e-15 {
                        self.formant_ic2[b] = 0.0;
                    }
                    sum += v1 * weight;
                }
                self.ic1eq = self.formant_ic1[0];
                self.ic2eq = sum;
                Self::fast_tanh(sum * 2.1 + input * 0.12)
            }
            FilterMode::DualSerialHpLp => {
                // Doble Filtro en Serie (DUAL:S): Highpass 2-polos resonante (cutoff * 0.25) -> Ladder 24dB Lowpass
                let g_hp = (self.g * 0.25).clamp(0.001, 0.85);
                let k_hp = (self.k * 0.85).clamp(0.12, 1.5);
                let a1_hp = 1.0 / (1.0 + g_hp * (g_hp + k_hp));
                let a2_hp = g_hp * a1_hp;
                let a3_hp = g_hp * a2_hp;
                let v3_hp = input - self.dual_hp_ic2;
                let v1_hp = a1_hp * self.dual_hp_ic1 + a2_hp * v3_hp;
                let v2_hp = self.dual_hp_ic2 + a2_hp * self.dual_hp_ic1 + a3_hp * v3_hp;
                self.dual_hp_ic1 = 2.0 * v1_hp - self.dual_hp_ic1;
                self.dual_hp_ic2 = 2.0 * v2_hp - self.dual_hp_ic2;
                if self.dual_hp_ic1.abs() < 1e-15 {
                    self.dual_hp_ic1 = 0.0;
                }
                if self.dual_hp_ic2.abs() < 1e-15 {
                    self.dual_hp_ic2 = 0.0;
                }
                let hp_out = input - k_hp * v1_hp - v2_hp;

                // Etapa 2 en serie: Ladder 24dB Lowpass con saturación cálida
                let g1 = self.g / (1.0 + self.g);
                let g2 = g1 * g1;
                let g3 = g2 * g1;
                let g4 = g2 * g2;
                let k_moog = ((self.resonance - 0.5) / 9.5).clamp(0.0, 1.0) * 3.65;
                let s_est = g3 * self.ladder_s[0]
                    + g2 * self.ladder_s[1]
                    + g1 * self.ladder_s[2]
                    + self.ladder_s[3];
                let fb = Self::fast_tanh(s_est);
                let u = Self::fast_tanh(hp_out * 1.45 - k_moog * fb) / (1.0 + k_moog * g4);
                let mut x = u;
                for st in &mut self.ladder_s {
                    let v = (x - *st) * g1;
                    let y = v + *st;
                    *st = y + v;
                    if st.abs() < 1e-15 {
                        *st = 0.0;
                    }
                    x = y;
                }
                self.ic1eq = v1_hp;
                self.ic2eq = x;
                x * (1.0 + k_moog * 0.25)
            }
            FilterMode::DualParallelLpBp => {
                // Doble Filtro en Paralelo (DUAL:P): SVF Lowpass + Resonador Vocal Formant BP
                let v3 = input - self.ic2eq;
                let v1 = self.a1 * self.ic1eq + self.a2 * v3;
                let v2 = self.ic2eq + self.a2 * self.ic1eq + self.a3 * v3;
                self.ic1eq = 2.0 * v1 - self.ic1eq;
                self.ic2eq = 2.0 * v2 - self.ic2eq;
                if self.ic1eq.abs() < 1e-15 {
                    self.ic1eq = 0.0;
                }
                if self.ic2eq.abs() < 1e-15 {
                    self.ic2eq = 0.0;
                }

                const WEIGHTS: [f32; 3] = [1.0, 0.72, 0.45];
                let mut fmt_sum = 0.0_f32;
                for (b, &weight) in WEIGHTS.iter().enumerate() {
                    let fv3 = input - self.formant_ic2[b];
                    let fv1 = self.formant_a1[b] * self.formant_ic1[b] + self.formant_a2[b] * fv3;
                    let fv2 = self.formant_ic2[b]
                        + self.formant_a2[b] * self.formant_ic1[b]
                        + self.formant_a3[b] * fv3;
                    self.formant_ic1[b] = 2.0 * fv1 - self.formant_ic1[b];
                    self.formant_ic2[b] = 2.0 * fv2 - self.formant_ic2[b];
                    if self.formant_ic1[b].abs() < 1e-15 {
                        self.formant_ic1[b] = 0.0;
                    }
                    if self.formant_ic2[b].abs() < 1e-15 {
                        self.formant_ic2[b] = 0.0;
                    }
                    fmt_sum += fv1 * weight;
                }
                Self::fast_tanh(v2 * 0.65 + fmt_sum * 1.35)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_advanced_filter_models_ladder_comb_and_formant_produce_distinct_responses() {
        let sr = 44100.0;
        let mut f_svf = StateVariableFilter::new(sr);
        f_svf.mode = FilterMode::Lowpass;
        f_svf.set_cutoff(1200.0);
        f_svf.set_resonance(2.5);

        let mut f_ladder = StateVariableFilter::new(sr);
        f_ladder.mode = FilterMode::Ladder24dB;
        f_ladder.set_cutoff(1200.0);
        f_ladder.set_resonance(2.5);

        let mut f_comb = StateVariableFilter::new(sr);
        f_comb.mode = FilterMode::Comb;
        f_comb.set_cutoff(440.0);
        f_comb.set_resonance(4.0);

        let mut f_formant = StateVariableFilter::new(sr);
        f_formant.mode = FilterMode::Formant;
        f_formant.set_cutoff(1500.0);
        f_formant.set_resonance(3.0);

        let mut diff_ladder_svf = 0.0_f32;
        let mut diff_comb_svf = 0.0_f32;
        let mut diff_formant_svf = 0.0_f32;

        for i in 0..512 {
            // Señal diente de sierra rica en armónicos
            let phase = (i as f32 * 220.0 / sr).fract();
            let x = phase * 2.0 - 1.0;
            let y_svf = f_svf.process_sample(x);
            let y_lad = f_ladder.process_sample(x);
            let y_cmb = f_comb.process_sample(x);
            let y_fmt = f_formant.process_sample(x);

            assert!(y_lad.is_finite());
            assert!(y_cmb.is_finite());
            assert!(y_fmt.is_finite());

            diff_ladder_svf += (y_lad - y_svf).abs();
            diff_comb_svf += (y_cmb - y_svf).abs();
            diff_formant_svf += (y_fmt - y_svf).abs();
        }

        assert!(
            diff_ladder_svf > 1.0,
            "Ladder 24dB must differ from 12dB SVF"
        );
        assert!(diff_comb_svf > 1.0, "Comb filter must differ from SVF");
        assert!(
            diff_formant_svf > 1.0,
            "Formant filter must differ from SVF"
        );
    }

    #[test]
    fn test_comb_filter_bass_tuning_and_high_sample_rate() {
        let sr = 96000.0;
        let mut f_55hz = StateVariableFilter::new(sr);
        f_55hz.mode = FilterMode::Comb;
        f_55hz.set_cutoff(55.0); // A1 bass fundamental (~1745.45 samples delay at 96kHz)
        f_55hz.set_resonance(6.0);

        let mut f_110hz = StateVariableFilter::new(sr);
        f_110hz.mode = FilterMode::Comb;
        f_110hz.set_cutoff(110.0); // A2 fundamental (~872.72 samples delay at 96kHz)
        f_110hz.set_resonance(6.0);

        let mut diff = 0.0_f32;
        for i in 0..2048 {
            let x = if i < 64 { 1.0 - (i as f32 / 64.0) } else { 0.0 };
            let y55 = f_55hz.process_sample(x);
            let y110 = f_110hz.process_sample(x);
            assert!(y55.is_finite() && y110.is_finite());
            diff += (y55 - y110).abs();
        }
        assert!(
            diff > 5.0,
            "Comb filter at 96kHz must distinguish 55Hz (1745 smp) from 110Hz (872 smp), got diff={}",
            diff
        );
    }

    #[test]
    fn test_dual_serial_and_parallel_filter_topologies_produce_distinct_responses() {
        let sr = 48000.0;
        let mut f_serial = StateVariableFilter::new(sr);
        f_serial.mode = FilterMode::DualSerialHpLp;
        f_serial.set_cutoff(1200.0);
        f_serial.set_resonance(2.8);

        let mut f_parallel = StateVariableFilter::new(sr);
        f_parallel.mode = FilterMode::DualParallelLpBp;
        f_parallel.set_cutoff(1200.0);
        f_parallel.set_resonance(2.8);

        let mut f_lp = StateVariableFilter::new(sr);
        f_lp.mode = FilterMode::Lowpass;
        f_lp.set_cutoff(1200.0);
        f_lp.set_resonance(2.8);

        let mut diff_serial_lp = 0.0_f32;
        let mut diff_parallel_lp = 0.0_f32;
        let mut diff_serial_parallel = 0.0_f32;
        for i in 0..512 {
            let phase = (i as f32 * 180.0 / sr).fract();
            let x = phase * 2.0 - 1.0;
            let ys = f_serial.process_sample(x);
            let yp = f_parallel.process_sample(x);
            let ylp = f_lp.process_sample(x);
            assert!(ys.is_finite() && yp.is_finite());
            diff_serial_lp += (ys - ylp).abs();
            diff_parallel_lp += (yp - ylp).abs();
            diff_serial_parallel += (ys - yp).abs();
        }
        assert!(diff_serial_lp > 1.0, "DualSerialHpLp must differ from SVF LP");
        assert!(
            diff_parallel_lp > 1.0,
            "DualParallelLpBp must differ from SVF LP"
        );
        assert!(
            diff_serial_parallel > 1.0,
            "DualSerialHpLp and DualParallelLpBp must differ from each other"
        );
    }
}
