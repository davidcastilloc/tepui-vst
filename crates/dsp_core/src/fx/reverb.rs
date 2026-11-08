//! Reverberador algorítmico estéreo basado en red de filtros Comb y difusores Allpass de Schroeder/Freeverb.

use crate::filters::{AllpassFilter, CombFilter};

/// Reverberador algorítmico estéreo libre de asignaciones en tiempo real.
#[derive(Debug, Clone)]
pub struct AlgorithmicReverb {
    combs_l: [CombFilter<2048>; 4],
    combs_r: [CombFilter<2048>; 4],
    allpass_l: [AllpassFilter<1024>; 2],
    allpass_r: [AllpassFilter<1024>; 2],
    mix: f32,
}

impl Default for AlgorithmicReverb {
    fn default() -> Self {
        Self::new()
    }
}

impl AlgorithmicReverb {
    /// Crea un nuevo reverberador algorítmico con longitudes primas desacopladas entre L y R.
    pub fn new() -> Self {
        Self {
            combs_l: [
                CombFilter::new(1116, 0.78, 0.25),
                CombFilter::new(1188, 0.78, 0.25),
                CombFilter::new(1277, 0.78, 0.25),
                CombFilter::new(1356, 0.78, 0.25),
            ],
            combs_r: [
                CombFilter::new(1139, 0.78, 0.25),
                CombFilter::new(1211, 0.78, 0.25),
                CombFilter::new(1300, 0.78, 0.25),
                CombFilter::new(1379, 0.78, 0.25),
            ],
            allpass_l: [
                AllpassFilter::new(556, 0.5),
                AllpassFilter::new(441, 0.5),
            ],
            allpass_r: [
                AllpassFilter::new(579, 0.5),
                AllpassFilter::new(464, 0.5),
            ],
            mix: 0.2,
        }
    }

    /// Configura el nivel de mezcla húmedo/seco (`[0.0, 1.0]`).
    pub fn set_mix(&mut self, mix: f32) {
        self.mix = mix.clamp(0.0, 1.0);
    }

    /// Reinicia todas las colas de reverberación.
    pub fn reset(&mut self) {
        for c in &mut self.combs_l {
            c.reset();
        }
        for c in &mut self.combs_r {
            c.reset();
        }
        for a in &mut self.allpass_l {
            a.reset();
        }
        for a in &mut self.allpass_r {
            a.reset();
        }
    }

    /// Procesa un par estéreo en tiempo real ($0$ allocs).
    #[inline(always)]
    pub fn process_stereo(&mut self, in_l: f32, in_r: f32) -> (f32, f32) {
        let mono_in = (in_l + in_r) * 0.125;
        let mut out_l = 0.0;
        let mut out_r = 0.0;

        for c in &mut self.combs_l {
            out_l += c.process(mono_in);
        }
        for c in &mut self.combs_r {
            out_r += c.process(mono_in);
        }
        for a in &mut self.allpass_l {
            out_l = a.process(out_l);
        }
        for a in &mut self.allpass_r {
            out_r = a.process(out_r);
        }

        (
            in_l * (1.0 - self.mix) + out_l * self.mix,
            in_r * (1.0 - self.mix) + out_r * self.mix,
        )
    }
}
