//! Línea de retardo estéreo Ping-Pong con interpolación fraccionaria libre de asignaciones en `process`.

use crate::math::denormal::flush_denormal;
use crate::math::interp::lerp;

/// Retardo estéreo con realimentación cruzada (Ping-Pong) prealocado en estructura (`MAX_SAMPLES`).
#[derive(Debug, Clone)]
pub struct StereoDelayLine<const MAX_SAMPLES: usize = 96_000> {
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    write_pos: usize,
    delay_l_samples: f32,
    delay_r_samples: f32,
    feedback: f32,
    mix: f32,
}

impl<const MAX_SAMPLES: usize> Default for StereoDelayLine<MAX_SAMPLES> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_SAMPLES: usize> StereoDelayLine<MAX_SAMPLES> {
    /// Crea una línea de retardo estéreo prealocando su capacidad una única vez fuera del hilo de audio.
    pub fn new() -> Self {
        Self {
            buf_l: vec![0.0; MAX_SAMPLES],
            buf_r: vec![0.0; MAX_SAMPLES],
            write_pos: 0,
            delay_l_samples: 12_000.0,
            delay_r_samples: 18_000.0,
            feedback: 0.35,
            mix: 0.25,
        }
    }

    /// Configura retardos en muestras para L/R, ganancia de realimentación (`[0.0, 0.95]`) y mezcla (`[0.0, 1.0]`).
    pub fn set_params(&mut self, delay_l_samples: f32, delay_r_samples: f32, feedback: f32, mix: f32) {
        let max_d = (MAX_SAMPLES.saturating_sub(4)) as f32;
        self.delay_l_samples = delay_l_samples.clamp(1.0, max_d);
        self.delay_r_samples = delay_r_samples.clamp(1.0, max_d);
        self.feedback = feedback.clamp(0.0, 0.95);
        self.mix = mix.clamp(0.0, 1.0);
    }

    /// Limpia los buffers internos sin liberar memoria.
    pub fn reset(&mut self) {
        self.buf_l.fill(0.0);
        self.buf_r.fill(0.0);
        self.write_pos = 0;
    }

    #[inline(always)]
    fn read_fractional(buf: &[f32], write_pos: usize, delay: f32) -> f32 {
        let len = buf.len();
        let d_int = delay as usize;
        let frac = delay - d_int as f32;
        let idx0 = (write_pos + len - d_int) % len;
        let idx1 = (idx0 + len - 1) % len;
        lerp(buf[idx0], buf[idx1], frac)
    }

    /// Procesa un par estéreo en tiempo real ($0$ allocs).
    #[inline(always)]
    pub fn process_stereo(&mut self, in_l: f32, in_r: f32) -> (f32, f32) {
        let tap_l = flush_denormal(Self::read_fractional(&self.buf_l, self.write_pos, self.delay_l_samples));
        let tap_r = flush_denormal(Self::read_fractional(&self.buf_r, self.write_pos, self.delay_r_samples));

        self.buf_l[self.write_pos] = flush_denormal(in_l + tap_r * self.feedback);
        self.buf_r[self.write_pos] = flush_denormal(in_r + tap_l * self.feedback);
        self.write_pos = (self.write_pos + 1) % MAX_SAMPLES;

        (
            in_l * (1.0 - self.mix) + tap_l * self.mix,
            in_r * (1.0 - self.mix) + tap_r * self.mix,
        )
    }
}
