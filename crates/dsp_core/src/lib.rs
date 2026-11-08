//! # `dsp_core` — Bloques de Procesamiento Digital de Señales (DSP) Agnósticos en Tiempo Real
//!
//! Este crate provee primitivas matemáticas, estructuras lock-free alineadas a caché,
//! filtros con preservación de topología (TPT ZDF), envolventes ADSR, procesadores dinámicos
//! y efectos espaciales reutilizables por cualquier plugin de audio (efectos, compresores,
//! ecualizadores o instrumentos).
//!
//! ## Garantías de Tiempo Real
//! - **Cero asignaciones dinámicas de memoria** (`0` heap allocations) durante el procesamiento de audio.
//! - **Cero primitivas bloqueantes** (`0` `Mutex` / `RwLock` en el camino de señal).
//! - **Protección anti-denormal** automática en lazos recursivos (`flush_denormal`).

pub mod dynamics;
pub mod filters;
pub mod fx;
pub mod math;
pub mod modulation;
pub mod primitives;

pub use dynamics::{BrickwallLimiter, EnvelopeDetector, FeedForwardCompressor};
pub use filters::{AllpassFilter, CombFilter, LadderFilter24dB, StateVariableFilter};
pub use fx::{AlgorithmicReverb, StereoChorus, StereoDelayLine};
pub use math::{
    db_to_gain, detune_cents_ratio, flush_denormal, gain_to_db, hermite4, hz_to_midi, lerp,
    midi_to_hz, sanitize_sample,
};
pub use modulation::{AdsrEnvelope, AdsrStage, LfoOscillator, LfoWaveform};
pub use primitives::{OnePoleSmoother, SeqLockArray, SpscRingBuffer};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_seqlock_alignment_and_zero_torn_reads_concurrent() {
        assert_eq!(std::mem::align_of::<SeqLockArray<64>>(), 64);

        let lock = Arc::new(SeqLockArray::<32>::new(1.0));
        let writer_lock = Arc::clone(&lock);

        let writer = std::thread::spawn(move || {
            for iter in 0..5_000 {
                let val = (iter as f32) * 0.25;
                let frame = [val; 32];
                writer_lock.write_slice(&frame, 0.0);
            }
        });

        let mut snapshot = [1.0_f32; 32];
        for _ in 0..20_000 {
            let _ = lock.read_into_snapshot(&mut snapshot);
            let first = snapshot[0];
            for &sample in &snapshot[1..] {
                assert_eq!(
                    sample.to_bits(),
                    first.to_bits(),
                    "Detectado torn read en SeqLockArray"
                );
            }
        }

        writer.join().expect("writer thread panicked");
    }

    #[test]
    fn test_one_pole_smoother_convergence_and_denormal_cutoff() {
        let mut smoother = OnePoleSmoother::new(0.0, 5.0, 48_000.0);
        smoother.set_target(1.0);
        assert!(smoother.is_smoothing());
        let final_val = smoother.advance_samples(48_000);
        assert_eq!(final_val, 1.0);
        assert!(!smoother.is_smoothing());
    }

    #[test]
    fn test_zdf_svf_lowpass_and_highpass_complementary_response() {
        let mut svf = StateVariableFilter::new();
        svf.set_sample_rate(48_000.0);
        svf.set_params(1_200.0, 0.707);

        let mut energy_lp = 0.0_f32;
        let mut energy_hp = 0.0_f32;
        for i in 0..512 {
            let t = i as f32 / 48_000.0;
            let low_tone = (2.0 * std::f32::consts::PI * 100.0 * t).sin();
            let high_tone = (2.0 * std::f32::consts::PI * 12_000.0 * t).sin();
            let (lp, _bp, hp, _notch) = svf.process_multimode(low_tone + high_tone);
            energy_lp += lp * lp;
            energy_hp += hp * hp;
        }
        assert!(energy_lp > 10.0);
        assert!(energy_hp > 10.0);
    }

    #[test]
    fn test_compressor_and_brickwall_limiter_dynamics() {
        let mut comp = FeedForwardCompressor::new(48_000.0);
        comp.set_params(-12.0, 4.0, 3.0, 0.0);
        let mut lim = BrickwallLimiter::new(48_000.0, -1.0);

        for _ in 0..512 {
            let (c_l, c_r) = comp.process_stereo(0.95, -0.95);
            let (l_l, l_r) = lim.process_stereo(c_l * 2.0, c_r * 2.0);
            assert!(l_l.abs() <= db_to_gain(-1.0) + 1e-4);
            assert!(l_r.abs() <= db_to_gain(-1.0) + 1e-4);
        }
        assert!(comp.gain_reduction_db() < -1.0);
    }
}
