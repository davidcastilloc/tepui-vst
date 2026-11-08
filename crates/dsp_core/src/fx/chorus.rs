//! Efecto de Chorus Estéreo en cuadratura de fase libre de asignaciones en tiempo de proceso.

use crate::math::denormal::flush_denormal;
use crate::math::interp::lerp;

/// Chorus estéreo con LFO en cuadratura (90 grados de desfase entre canales L y R).
#[derive(Debug, Clone)]
pub struct StereoChorus<const MAX_SAMPLES: usize = 4096> {
    buf_l: [f32; MAX_SAMPLES],
    buf_r: [f32; MAX_SAMPLES],
    write_pos: usize,
    phase: f32,
    sample_rate: f32,
    rate_hz: f32,
    depth_ms: f32,
    mix: f32,
}

impl<const MAX_SAMPLES: usize> Default for StereoChorus<MAX_SAMPLES> {
    fn default() -> Self {
        Self::new(44100.0)
    }
}

impl<const MAX_SAMPLES: usize> StereoChorus<MAX_SAMPLES> {
    /// Crea un nuevo procesador de Chorus estéreo.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            buf_l: [0.0; MAX_SAMPLES],
            buf_r: [0.0; MAX_SAMPLES],
            write_pos: 0,
            phase: 0.0,
            sample_rate: sample_rate.max(1000.0),
            rate_hz: 0.8,
            depth_ms: 4.0,
            mix: 0.35,
        }
    }

    /// Configura velocidad (`rate_hz`), profundidad (`depth_ms`) y mezcla (`mix`).
    pub fn set_params(&mut self, rate_hz: f32, depth_ms: f32, mix: f32) {
        self.rate_hz = rate_hz.clamp(0.05, 10.0);
        self.depth_ms = depth_ms.clamp(0.5, 15.0);
        self.mix = mix.clamp(0.0, 1.0);
    }

    /// Limpia la línea de modulación.
    pub fn reset(&mut self) {
        self.buf_l.fill(0.0);
        self.buf_r.fill(0.0);
        self.write_pos = 0;
        self.phase = 0.0;
    }

    /// Procesa un par estéreo en tiempo real.
    #[inline(always)]
    pub fn process_stereo(&mut self, in_l: f32, in_r: f32) -> (f32, f32) {
        self.buf_l[self.write_pos] = flush_denormal(in_l);
        self.buf_r[self.write_pos] = flush_denormal(in_r);

        let angle = self.phase * 2.0 * std::f32::consts::PI;
        let mod_l = angle.sin();
        let mod_r = angle.cos();
        self.phase = (self.phase + self.rate_hz / self.sample_rate).fract();

        let center_samples = 0.012 * self.sample_rate;
        let depth_samples = (self.depth_ms * 0.001) * self.sample_rate;
        let max_d = (MAX_SAMPLES - 4) as f32;

        let d_l = (center_samples + mod_l * depth_samples).clamp(2.0, max_d);
        let d_r = (center_samples + mod_r * depth_samples).clamp(2.0, max_d);

        let read_interp = |buf: &[f32; MAX_SAMPLES], d: f32| -> f32 {
            let di = d as usize;
            let frac = d - di as f32;
            let i0 = (self.write_pos + MAX_SAMPLES - di) % MAX_SAMPLES;
            let i1 = (i0 + MAX_SAMPLES - 1) % MAX_SAMPLES;
            lerp(buf[i0], buf[i1], frac)
        };

        let wet_l = read_interp(&self.buf_l, d_l);
        let wet_r = read_interp(&self.buf_r, d_r);
        self.write_pos = (self.write_pos + 1) % MAX_SAMPLES;

        (
            in_l * (1.0 - self.mix) + wet_l * self.mix,
            in_r * (1.0 - self.mix) + wet_r * self.mix,
        )
    }
}
