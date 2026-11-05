//! Núcleo de DSP en tiempo real para el Sampler Cromático Polifónico.
//!
//! Arquitectura lock-free, zero-allocation y desacoplada del framework de plugins.
#![allow(
    unknown_lints,
    clippy::chunks_exact_to_as_chunks,
    clippy::approx_constant
)]

pub mod arp;
pub mod buffer;
pub mod envelope;
pub mod filter;
pub mod fx;
pub mod lfo;
pub mod resampler;
pub mod rt_primitives;
pub mod voice;
pub mod voice_manager;
pub mod wav;

pub use arp::{ArpMode, ArpStepEvent, TepuiArpeggiator};
pub use buffer::{PianoRollPrepReport, SampleBuffer, SharedSampleBuffer};
pub use envelope::{AdsrEnvelope, AdsrStage};
pub use filter::{FilterMode, StateVariableFilter};
pub use fx::StereoFxRack;
pub use lfo::{LfoShape, LfoState, LfoTarget, DEFAULT_MOD_MATRIX_MASK};
pub use resampler::{calculate_playback_ratio, hermite_interpolate_4p, ResamplingQuality};
pub use rt_primitives::{OnePoleSmoother, SeqLockArray};
pub use voice::{SampleRegionOverride, Voice};
pub use voice_manager::VoiceManager;
pub use wav::{base64_decode, base64_encode, decode_wav, encode_wav_f32, encode_wav_pcm16};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hermite_interpolation_flat() {
        // Cuatro puntos iguales deben retornar exactamente el mismo valor constante
        let val = hermite_interpolate_4p(1.0, 1.0, 1.0, 1.0, 0.5);
        assert!((val - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_hermite_resampler_sentinel_padding() {
        let buf = SampleBuffer::from_mono(vec![0.25, 0.5, 0.75, 1.0], 44100.0, 60);
        // Longitud audible = 4, pero los vectores internos incluyen 2 muestras sentinel (total = 6)
        assert_eq!(buf.len(), 4);
        assert_eq!(buf.left.len(), 6);
        assert_eq!(buf.right.len(), 6);
        assert_eq!(buf.left[4], 0.0);
        assert_eq!(buf.left[5], 0.0);

        // Lectura en bordes (inicio, penúltima y última muestra fraccionaria) sin pánico
        let s_start =
            resampler::read_interpolated_sample(&buf, 0, 0.0, ResamplingQuality::HermiteCubic);
        assert!((s_start - 0.25).abs() < 1e-6);

        let s_last =
            resampler::read_interpolated_sample(&buf, 0, 3.5, ResamplingQuality::HermiteCubic);
        assert!(!s_last.is_nan() && !s_last.is_infinite());

        let s_oob =
            resampler::read_interpolated_sample(&buf, 0, 4.0, ResamplingQuality::HermiteCubic);
        assert_eq!(s_oob, 0.0);
    }

    #[test]
    fn test_chromatic_pitch_ratio_octaves() {
        // Una octava arriba (12 semitonos de C4 a C5) debe duplicar exactamente la frecuencia (ratio = 2.0)
        let ratio_octave_up = calculate_playback_ratio(72, 60, 0.0, 44100.0, 44100.0);
        assert!((ratio_octave_up - 2.0).abs() < 1e-6);

        // Misma nota (C4 a C4) debe ser ratio 1.0
        let ratio_unison = calculate_playback_ratio(60, 60, 0.0, 44100.0, 44100.0);
        assert!((ratio_unison - 1.0).abs() < 1e-6);

        // Una octava abajo (C3 a C4) debe ser ratio 0.5
        let ratio_octave_down = calculate_playback_ratio(48, 60, 0.0, 44100.0, 44100.0);
        assert!((ratio_octave_down - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_adsr_lifecycle() {
        let mut env = AdsrEnvelope::new(1000.0); // 1 kHz sample rate for easy counting
        env.set_parameters(0.01, 0.02, 0.5, 0.03); // 10ms attack, 20ms decay, 0.5 sustain, 30ms release

        env.note_on();
        assert_eq!(env.stage, AdsrStage::Attack);

        // Avanzar el ataque (10 muestras a 1000 Hz = 10 ms)
        for _ in 0..15 {
            env.process_sample();
        }
        // Debe haber llegado al menos a Decay o Sustain
        assert!(env.stage == AdsrStage::Decay || env.stage == AdsrStage::Sustain);

        // Soltar la nota
        env.note_off();
        assert_eq!(env.stage, AdsrStage::Release);

        // Avanzar tiempo suficiente para que llegue a Idle
        for _ in 0..100 {
            env.process_sample();
        }
        assert_eq!(env.stage, AdsrStage::Idle);
        assert_eq!(env.process_sample(), 0.0);
    }

    #[test]
    fn test_svf_filter_stability() {
        let mut filter = StateVariableFilter::new(44100.0);
        filter.set_cutoff(1000.0);
        filter.set_resonance(5.0);
        filter.mode = FilterMode::Lowpass;

        // Inyectar un impulso unitario
        let out1 = filter.process_sample(1.0);
        assert!(!out1.is_nan() && !out1.is_infinite());

        // Inyectar 1000 muestras de ceros para verificar decaimiento estable sin explosión
        for _ in 0..1000 {
            let out = filter.process_sample(0.0);
            assert!(!out.is_nan() && !out.is_infinite());
        }
    }

    #[test]
    fn test_svf_denormal_flush() {
        let mut filter = StateVariableFilter::new(44100.0);
        filter.set_cutoff(2000.0);
        filter.set_resonance(0.707);
        filter.mode = FilterMode::Lowpass;

        // Excitar el filtro con un impulso inicial
        let _ = filter.process_sample(1.0);

        // Alimentar con silencio hasta que la energía decaiga por debajo del umbral de flush (1e-15)
        for _ in 0..4000 {
            let out = filter.process_sample(0.0);
            assert!(!out.is_subnormal(), "Filter output must never be subnormal");
            let (ic1, ic2) = filter.integrator_states();
            assert!(!ic1.is_subnormal(), "ic1eq must never become subnormal");
            assert!(!ic2.is_subnormal(), "ic2eq must never become subnormal");
        }

        // Tras decaer por completo, ambos integradores deben ser exactamente 0.0
        let (ic1, ic2) = filter.integrator_states();
        assert_eq!(ic1, 0.0);
        assert_eq!(ic2, 0.0);
    }

    #[test]
    fn test_voice_manager_polyphony_and_stealing() {
        let mut vm = VoiceManager::new(44100.0, 16);
        // Crear un buffer de prueba de 1 segundo a 440 Hz
        let mut samples = Vec::with_capacity(44100);
        for i in 0..44100 {
            let t = i as f32 / 44100.0;
            samples.push((t * 440.0 * 2.0 * std::f32::consts::PI).sin());
        }
        let buffer = SampleBuffer::new_mono(samples, 44100.0, 60);

        // Activar 16 notas diferentes
        for note in 60..76 {
            vm.note_on(note, 0.8, &buffer);
        }
        assert_eq!(vm.active_voice_count(), 16);

        // Activar 10 notas adicionales para forzar Voice Stealing
        for note in 76..86 {
            vm.note_on(note, 0.9, &buffer);
        }
        // La cantidad máxima de voces nunca debe superar las 16 prealocadas
        assert_eq!(vm.active_voice_count(), 16);

        // Procesar un bloque de audio estéreo de 512 muestras
        let mut out_left = vec![0.0; 512];
        let mut out_right = vec![0.0; 512];
        vm.process_block(&buffer, &mut out_left, &mut out_right);

        // Comprobar que no hay NaNs ni infinitos
        for s in out_left.iter().chain(out_right.iter()) {
            assert!(!s.is_nan());
            assert!(!s.is_infinite());
        }
    }

    #[test]
    fn test_voice_manager_sustain_pedal() {
        let mut vm = VoiceManager::new(44100.0, 16);
        let samples = vec![0.5; 44100];
        let buffer = SampleBuffer::new_mono(samples, 44100.0, 60);

        // Activar pedal de sustain (CC 64)
        vm.set_sustain_pedal(true);

        // Tocar nota 60
        vm.note_on(60, 0.8, &buffer);
        assert_eq!(vm.active_voice_count(), 1);

        // Soltar nota 60 mientras el pedal está presionado
        vm.note_off(60);
        // La nota debe permanecer sonando en estado activo/sustain
        assert_eq!(vm.active_voice_count(), 1);

        // Soltar pedal de sustain
        vm.set_sustain_pedal(false);
        // Ahora entra en fase de release
        assert_eq!(vm.active_voice_count(), 1);

        // Kill all apaga todo
        vm.kill_all();
        let mut out_left = vec![0.0; 512];
        let mut out_right = vec![0.0; 512];
        vm.process_block(&buffer, &mut out_left, &mut out_right);
        assert_eq!(vm.active_voice_count(), 0);
    }

    #[test]
    fn test_voice_manager_pitch_bend() {
        let mut vm = VoiceManager::new(44100.0, 16);
        let samples = vec![0.5; 44100];
        let buffer = SampleBuffer::new_mono(samples, 44100.0, 60);

        vm.note_on(60, 0.8, &buffer);
        // Modificar pitch bend en +2 semitonos
        vm.set_pitch_bend(2.0);

        let mut out_left = vec![0.0; 64];
        let mut out_right = vec![0.0; 64];
        vm.process_block(&buffer, &mut out_left, &mut out_right);
        for s in out_left.iter().chain(out_right.iter()) {
            assert!(!s.is_nan() && !s.is_infinite());
        }
    }

    #[test]
    fn test_wav_f32_and_pcm16_roundtrip() {
        let orig_l = vec![0.0, 0.25, -0.5, 0.75, -0.9];
        let orig_r = vec![0.1, -0.25, 0.5, -0.75, 0.8];
        let original = SampleBuffer::new_stereo(orig_l.clone(), orig_r.clone(), 48000.0, 60)
            .with_name("Roundtrip Test");

        // 1. IEEE Float 32-bit Roundtrip (exacto)
        let wav_f32_bytes = encode_wav_f32(&original);
        let decoded_f32 = decode_wav(&wav_f32_bytes).expect("decode_wav f32 failed");
        assert_eq!(decoded_f32.len(), 5);
        assert_eq!(decoded_f32.left.len(), 7); // 5 audibles + 2 sentinel zeros
        assert_eq!(decoded_f32.sample_rate, 48000.0);
        for i in 0..5 {
            assert!((decoded_f32.get_sample(0, i) - orig_l[i]).abs() < 1e-6);
            assert!((decoded_f32.get_sample(1, i) - orig_r[i]).abs() < 1e-6);
        }
        assert_eq!(decoded_f32.left[5], 0.0);
        assert_eq!(decoded_f32.left[6], 0.0);

        // 2. PCM 16-bit Roundtrip (precisión 16 bits)
        let wav_pcm16_bytes = encode_wav_pcm16(&original);
        let decoded_pcm16 = decode_wav(&wav_pcm16_bytes).expect("decode_wav pcm16 failed");
        assert_eq!(decoded_pcm16.len(), 5);
        assert_eq!(decoded_pcm16.left.len(), 7);
        for i in 0..5 {
            assert!((decoded_pcm16.get_sample(0, i) - orig_l[i]).abs() < 1e-4);
            assert!((decoded_pcm16.get_sample(1, i) - orig_r[i]).abs() < 1e-4);
        }
    }

    #[test]
    fn test_voice_manager_root_note_override() {
        let mut vm = VoiceManager::new(44100.0, 16);
        let samples: Vec<f32> = (0..4096)
            .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 44100.0).sin())
            .collect();
        let buffer = SampleBuffer::new_mono(samples, 44100.0, 60);

        assert_eq!(vm.effective_root_note(&buffer), 60);
        vm.set_root_note_override(Some(48), &buffer);
        assert_eq!(vm.effective_root_note(&buffer), 48);

        vm.note_on(60, 0.9, &buffer);
        let mut out_l = vec![0.0; 128];
        let mut out_r = vec![0.0; 128];
        vm.process_block(&buffer, &mut out_l, &mut out_r);
        let energy: f32 = out_l.iter().map(|s| s.abs()).sum();
        assert!(energy > 0.1);
    }

    #[test]
    fn test_stereo_fx_rack_and_denormal_safety() {
        let mut fx = StereoFxRack::new(44100.0);
        fx.set_mix(0.45);
        assert!((fx.mix() - 0.45).abs() < 1e-6);

        let mut left: Vec<f32> = (0..1024)
            .map(|i| (i as f32 * 220.0 * 2.0 * std::f32::consts::PI / 44100.0).sin() * 0.5)
            .collect();
        let mut right = left.clone();
        let dry_left = left.clone();

        fx.process_block(&mut left, &mut right);

        let mut diff_energy = 0.0_f32;
        for i in 0..1024 {
            assert!(left[i].is_finite() && !left[i].is_subnormal());
            assert!(right[i].is_finite() && !right[i].is_subnormal());
            diff_energy += (left[i] - dry_left[i]).abs();
        }
        assert!(
            diff_energy > 0.1,
            "StereoFxRack should modulate signal when mix > 0"
        );

        // Verificar cola de reverberación "Cañón del Tepui" (4-Comb + 2-AllPass) tras cesar la señal de entrada
        let mut silence_l = vec![0.0_f32; 4096];
        let mut silence_r = vec![0.0_f32; 4096];
        fx.process_block(&mut silence_l, &mut silence_r);
        let reverb_tail: f32 = silence_l[1200..4096]
            .iter()
            .chain(silence_r[1200..4096].iter())
            .map(|s| s.abs())
            .sum();
        assert!(
            reverb_tail > 0.05,
            "Tepui Canyon Reverb tank should produce a sustained diffused acoustic tail: {}",
            reverb_tail
        );
    }

    #[test]
    fn test_sample_region_override_trim_and_loop() {
        let mut vm = VoiceManager::new(1000.0, 4);
        vm.update_envelope_params(0.001, 0.01, 1.0, 0.05);
        // 100 muestras (100 ms a 1 kHz)
        let samples = vec![0.6; 100];
        let buffer = SampleBuffer::new_mono(samples, 1000.0, 60);

        // 1. Con loop desactivado y recorte al 50% (50 muestras), la voz debe detenerse tras ~50 muestras
        vm.set_region_override(SampleRegionOverride {
            start_norm: 0.0,
            end_norm: 0.50,
            loop_enabled: Some(false),
            loop_start_norm: 0.10,
            loop_end_norm: 0.40,
        });
        vm.note_on(60, 1.0, &buffer);
        let mut out_l = vec![0.0; 80];
        let mut out_r = vec![0.0; 80];
        vm.process_block(&buffer, &mut out_l, &mut out_r);
        assert_eq!(
            vm.active_voice_count(),
            0,
            "Voice should stop at trimmed end_norm=0.50"
        );

        // 2. Con loop activado entre 0.10 y 0.40, la voz debe seguir sonando indefinidamente más allá de 200 muestras
        vm.set_region_override(SampleRegionOverride {
            start_norm: 0.0,
            end_norm: 0.50,
            loop_enabled: Some(true),
            loop_start_norm: 0.10,
            loop_end_norm: 0.40,
        });
        vm.note_on(60, 1.0, &buffer);
        let mut loop_l = vec![0.0; 250];
        let mut loop_r = vec![0.0; 250];
        vm.process_block(&buffer, &mut loop_l, &mut loop_r);
        assert_eq!(
            vm.active_voice_count(),
            1,
            "Voice should remain active when loop_enabled=Some(true)"
        );
        assert!(loop_l[240].abs() > 0.1);
    }

    #[test]
    fn test_base64_wav_roundtrip() {
        let orig = SampleBuffer::from_mono(vec![0.1, -0.3, 0.5, -0.7], 44100.0, 64);
        let wav_bytes = encode_wav_f32(&orig);
        let b64 = base64_encode(&wav_bytes);
        assert!(!b64.is_empty());
        let decoded_bytes = base64_decode(&b64).expect("base64_decode failed");
        assert_eq!(wav_bytes, decoded_bytes);
        let decoded_buf = decode_wav(&decoded_bytes).expect("decode_wav failed");
        assert_eq!(decoded_buf.len(), 4);
    }

    #[test]
    fn test_lfo_modulation_targets_and_keyzone_routing() {
        let mut vm = VoiceManager::new(44100.0, 8);
        let primary = SampleBuffer::new_mono(vec![0.5; 4410], 44100.0, 60);
        let low_zone = SampleBuffer::new_mono(vec![0.25; 4410], 44100.0, 36);
        let high_zone = SampleBuffer::new_mono(vec![0.85; 4410], 44100.0, 84);

        let zones = [Some(&low_zone), None, Some(&high_zone)];

        // 1. Verificar ruteo de Keyzones:
        // Nota 36 (C2) -> Low Zone (0.25)
        // Nota 60 (C4) -> Mid Zone vacía -> fallback a Primary (0.5)
        // Nota 84 (C6) -> High Zone (0.85)
        vm.update_envelope_params(0.001, 0.01, 1.0, 0.2);
        vm.update_filter_params(8000.0, 0.707, FilterMode::Lowpass);
        vm.update_lfo_params(6.0, 0.75, LfoTarget::Cutoff);

        vm.note_on_with_zones(36, 1.0, &primary, &zones);
        vm.note_on_with_zones(60, 1.0, &primary, &zones);
        vm.note_on_with_zones(84, 1.0, &primary, &zones);
        assert_eq!(vm.active_voice_count(), 3);

        let mut out_l = vec![0.0; 512];
        let mut out_r = vec![0.0; 512];
        vm.process_block_with_zones(&primary, &zones, &mut out_l, &mut out_r);
        assert!(
            vm.lfo_last_value().abs() > 0.0,
            "LFO should advance and produce modulation"
        );

        // Cambiar a LFO Pitch (Vibrato) y luego a Tremolo Estéreo
        vm.update_lfo_params(5.0, 0.8, LfoTarget::Pitch);
        vm.process_block_with_zones(&primary, &zones, &mut out_l, &mut out_r);

        vm.update_lfo_params(4.0, 0.9, LfoTarget::Tremolo);
        let mut trem_l = vec![0.0; 512];
        let mut trem_r = vec![0.0; 512];
        vm.process_block_with_zones(&primary, &zones, &mut trem_l, &mut trem_r);
        let diff_lr: f32 = trem_l
            .iter()
            .zip(trem_r.iter())
            .map(|(l, r)| (l - r).abs())
            .sum();
        assert!(
            diff_lr > 0.01,
            "Quadrature stereo tremolo should differentiate L and R"
        );
    }

    #[test]
    fn test_tepui_arpeggiator_joropo_and_merengue_modes() {
        let mut vm = VoiceManager::new(1000.0, 8);
        let primary = SampleBuffer::new_mono(vec![0.5; 2000], 1000.0, 60);
        let zones = [None, None, None];

        // 1. Activar modo Joropo 6/8 a 240 BPM (1 corchea = 125 ms = 125 muestras a 1 kHz)
        vm.update_arp_params(ArpMode::Joropo, 240.0);
        vm.note_on_with_zones(60, 0.9, &primary, &zones);
        vm.note_on_with_zones(64, 0.9, &primary, &zones);
        vm.note_on_with_zones(67, 0.9, &primary, &zones);

        // El primer paso se dispara al instante al pulsar la primera nota
        assert_eq!(vm.arp_steps_triggered(), 1);

        // Avanzar 6 bloques de 150 muestras (total 900 muestras -> múltiples pasos 6/8)
        let mut out_l = vec![0.0; 150];
        let mut out_r = vec![0.0; 150];
        for _ in 0..6 {
            vm.process_block_with_zones(&primary, &zones, &mut out_l, &mut out_r);
        }
        let joropo_steps = vm.arp_steps_triggered();
        assert!(
            joropo_steps >= 6,
            "Joropo 6/8 arpeggiator should trigger at least 6 steps over 900 samples, got {}",
            joropo_steps
        );

        // 2. Cambiar a Merengue Venezolano 5/8 y verificar avance continuo
        vm.update_arp_params(ArpMode::Merengue, 240.0);
        for _ in 0..5 {
            vm.process_block_with_zones(&primary, &zones, &mut out_l, &mut out_r);
        }
        assert!(
            vm.arp_steps_triggered() >= joropo_steps + 4,
            "Merengue 5/8 arpeggiator should continue triggering steps"
        );

        // 3. Soltar todas las notas detiene nuevos pasos del arpegiador
        vm.note_off(60);
        vm.note_off(64);
        vm.note_off(67);
        let steps_after_release = vm.arp_steps_triggered();
        vm.process_block_with_zones(&primary, &zones, &mut out_l, &mut out_r);
        assert_eq!(vm.arp_steps_triggered(), steps_after_release);
    }

    #[test]
    fn test_mod_matrix_5x5_multiroute_dsp_and_odd_telemetry() {
        let mut vm = VoiceManager::new(44100.0, 8);
        let primary = SampleBuffer::new_mono(vec![0.6; 4410], 44100.0, 60);
        let zones = [None, None, None];

        // Activar 5 rutas simultáneas en la matriz 5x5:
        // (0,0) LFO1->PITCH, (0,1) LFO1->CUTOFF, (1,3) LFO2->PAN, (2,2) ENV2->RESO, (4,4) MOD_W->MORPH
        let mask: u32 = (1 << 0) | (1 << 1) | (1 << 8) | (1 << 12) | (1 << 24);
        vm.set_mod_matrix_mask(mask);
        assert_eq!(vm.mod_matrix_mask(), mask);
        assert_eq!(vm.mod_matrix_active_routes(), 5);
        assert_eq!(vm.mod_matrix_dsp_frames(), 0);

        vm.update_envelope_params(0.002, 0.05, 0.85, 0.2);
        vm.update_filter_params(2400.0, 1.5, FilterMode::Lowpass);
        vm.update_lfo_params(5.0, 0.7, LfoTarget::Cutoff);
        vm.set_mod_wheel(0.8);

        vm.note_on_with_zones(60, 0.95, &primary, &zones);

        let mut out_l = vec![0.0; 256];
        let mut out_r = vec![0.0; 256];
        vm.process_block_with_zones(&primary, &zones, &mut out_l, &mut out_r);

        assert_eq!(vm.mod_matrix_dsp_frames(), 256);
        assert!(vm.lfo_last_value().abs() > 0.0);
        // Verificar diferenciación estéreo por ruta LFO2 -> PAN (bit 8) y ausencia de NaNs
        let mut stereo_diff = 0.0_f32;
        for i in 0..256 {
            assert!(out_l[i].is_finite() && out_r[i].is_finite());
            stereo_diff += (out_l[i] - out_r[i]).abs();
        }
        assert!(
            stereo_diff > 0.01,
            "LFO2->PAN route in 5x5 Mod Matrix should create stereo pan differentiation"
        );
    }

    #[test]
    fn test_velocity_expression_polyphonic_arp_and_odd_telemetry() {
        let mut vm = VoiceManager::new(44100.0, 8);
        let samples: Vec<f32> = (0..4410)
            .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 44100.0).sin())
            .collect();
        let primary = SampleBuffer::new_mono(samples, 44100.0, 60);
        let zones = [None, None, None];

        vm.update_envelope_params(0.001, 0.05, 1.0, 0.2);
        vm.update_filter_params(3200.0, 0.8, FilterMode::Lowpass);

        // 1. Tocar nota suave (velocity = 0.20) y medir energía
        vm.note_on_with_zones(60, 0.20, &primary, &zones);
        assert_eq!(vm.velocity_events_count(), 1);
        assert!((vm.last_velocity() - 0.20).abs() < 1e-4);

        let mut soft_l = vec![0.0; 256];
        let mut soft_r = vec![0.0; 256];
        vm.process_block_with_zones(&primary, &zones, &mut soft_l, &mut soft_r);
        let soft_rms: f32 = soft_l.iter().map(|s| s * s).sum::<f32>() / 256.0;

        vm.kill_all();

        // 2. Tocar nota fuerte (velocity = 1.00) y comprobar que produce mucha más energía y brillo
        vm.note_on_with_zones(60, 1.00, &primary, &zones);
        assert_eq!(vm.velocity_events_count(), 2);
        assert!((vm.last_velocity() - 1.00).abs() < 1e-4);

        let mut loud_l = vec![0.0; 256];
        let mut loud_r = vec![0.0; 256];
        vm.process_block_with_zones(&primary, &zones, &mut loud_l, &mut loud_r);
        let loud_rms: f32 = loud_l.iter().map(|s| s * s).sum::<f32>() / 256.0;

        assert!(
            loud_rms > soft_rms * 8.0,
            "Loud velocity (1.0) must produce significantly higher RMS energy than soft velocity (0.2): loud={}, soft={}",
            loud_rms,
            soft_rms
        );

        // 3. Verificar que el arpegiador en modo Joropo 6/8 escala los pasos según la velocidad de entrada
        let mut arp = TepuiArpeggiator::new(44100.0);
        arp.set_mode(ArpMode::Joropo);
        arp.press_note_with_velocity(60, 0.25);
        let step_soft = arp.advance_chunk(1).expect("Expected Joropo step");
        assert!(step_soft.velocity < 0.40);

        let _ = arp.clear();
        arp.press_note_with_velocity(60, 1.00);
        let step_loud = arp.advance_chunk(1).expect("Expected Joropo step");
        assert!(step_loud.velocity > 0.90);
    }

    #[test]
    fn test_equal_power_loop_crossfade_and_tempo_synced_fx_rack() {
        let mut fx = StereoFxRack::new(48000.0);
        fx.set_tempo_sync(120.0, ArpMode::Joropo);
        let (jl, jr) = fx.delay_times_ms();
        // A 120 BPM (beat = 500 ms), Joropo 6/8 usa 1/3 y 2/3 de pulso (~166.6 ms y ~333.3 ms)
        assert!((jl - 166.666).abs() < 1.0);
        assert!((jr - 333.333).abs() < 1.0);

        fx.set_tempo_sync(120.0, ArpMode::Merengue);
        let (ml, mr) = fx.delay_times_ms();
        // A 120 BPM (beat = 500 ms), Merengue 5/8 usa 2/5 y 3/5 de pulso (200 ms y 300 ms)
        assert!((ml - 200.0).abs() < 1.0);
        assert!((mr - 300.0).abs() < 1.0);
    }

    #[test]
    fn test_piano_roll_auto_onset_trim_and_yin_pitch_detection() {
        let sr = 48000.0;
        let silence_samples = 1200usize; // 25 ms de silencio inicial típico de modelos IA
        let tone_samples = 8000usize;
        let mut mono = vec![0.0f32; silence_samples + tone_samples];

        // Generar una nota A3 = 220.0 Hz (MIDI 57) con segundo y tercer armónico
        for i in 0..tone_samples {
            let t = i as f32 / sr;
            let f0 = 220.0f32;
            let wave = 0.70 * (2.0 * std::f32::consts::PI * f0 * t).sin()
                + 0.20 * (2.0 * std::f32::consts::PI * (f0 * 2.0) * t).sin()
                + 0.10 * (2.0 * std::f32::consts::PI * (f0 * 3.0) * t).sin();
            mono[silence_samples + i] = wave;
        }

        // Inicialmente etiquetado erróneamente como C4 (60) con silencio inicial
        let mut buf = SampleBuffer::from_mono(mono, sr, 60);
        let report = buf.prepare_for_piano_roll();

        assert!(
            report.trimmed_leading_samples >= 1180 && report.trimmed_leading_samples <= 1205,
            "Se esperaba recortar ~1200 muestras de silencio inicial, obtuvo {}",
            report.trimmed_leading_samples
        );
        assert!(report.is_pitched);
        assert_eq!(
            report.detected_root_note, 57,
            "220 Hz debe mapear a A3 (MIDI 57)"
        );
        assert_eq!(buf.root_note, 57);
        assert!(
            (report.detected_f0_hz - 220.0).abs() < 1.5,
            "f0 detectada = {} Hz",
            report.detected_f0_hz
        );
        assert!(report.confidence > 0.80);
    }

    #[test]
    fn test_vital_serum_wavetable_unison_sub_ott_drive_and_macros() {
        let sr = 48000.0;
        let samples: Vec<f32> = (0..4800)
            .map(|i| (i as f32 * 261.63 * 2.0 * std::f32::consts::PI / sr).sin() * 0.5)
            .collect();
        let mut buf = SampleBuffer::new_mono(samples, sr, 60);
        buf.loop_enabled = true;
        buf.loop_start = 480;
        buf.loop_end = 4200;

        let mut vm = VoiceManager::new(sr, 8);
        vm.update_vital_serum_params(
            0.65,
            0.40,
            6,
            0.35,
            0.50,
            LfoShape::Saw,
            [0.30, 0.25, 0.40, 0.50],
        );
        assert_eq!(vm.lfo_shape(), LfoShape::Saw);

        vm.note_on(60, 0.90, &buf);
        let mut l = vec![0.0; 256];
        let mut r = vec![0.0; 256];
        vm.process_block(&buf, &mut l, &mut r);

        assert!(vm.vital_serum_dsp_frames() >= 256);
        // Verificar que Super-Unison (6 voces) crea diferencia estéreo real entre L y R
        let stereo_diff: f32 = l
            .iter()
            .zip(r.iter())
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>();
        assert!(
            stereo_diff > 0.05,
            "Super-Unison debe abrir el campo estéreo: diff={}",
            stereo_diff
        );

        // Probar Drive + OTT Multiband Compressor en StereoFxRack
        let mut fx = StereoFxRack::new(sr);
        fx.set_mix(0.20);
        fx.set_drive(0.65);
        fx.set_ott(0.75);
        fx.process_block(&mut l, &mut r);
        assert_eq!(fx.vital_fx_frames(), 256);
        assert!(l.iter().all(|x| x.is_finite()));
        assert!(r.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn test_custom_wavetable_freehand_draw_dsp() {
        let sr = 48000.0;
        let buf = SampleBuffer::new_mono(vec![0.05; 4800], sr, 60);
        let mut vm = VoiceManager::new(sr, 4);
        assert!(!vm.is_custom_wavetable_active());

        // Dibujar una onda personalizada de 64 puntos (diente de sierra escalonado con armónicos)
        let mut custom_pts = [0.0_f32; 64];
        for (i, slot) in custom_pts.iter_mut().enumerate() {
            *slot = ((i as f32 / 32.0) - 1.0).sin();
        }
        vm.set_custom_wavetable_cycle(&custom_pts);
        assert!(vm.is_custom_wavetable_active());
        assert_eq!(vm.custom_wt_draws_count(), 1);

        vm.note_on(60, 0.95, &buf);
        let mut l = vec![0.0; 256];
        let mut r = vec![0.0; 256];
        vm.process_block(&buf, &mut l, &mut r);

        let peak = l.iter().fold(0.0_f32, |acc, &x| acc.max(x.abs()));
        assert!(
            peak > 0.20,
            "Custom wavetable must produce strong signal, got {}",
            peak
        );
        assert!(vm.vital_serum_dsp_frames() >= 256);

        vm.clear_custom_wavetable_cycle();
        assert!(!vm.is_custom_wavetable_active());
    }

    #[test]
    fn test_audio_to_wavetable_single_cycle_extraction() {
        let sr = 48000.0;
        let f0 = 220.0_f32;
        let samples: Vec<f32> = (0..4800)
            .map(|i| {
                let t = i as f32 / sr;
                let w = std::f32::consts::TAU * f0 * t;
                w.sin() * 0.65 + (w * 2.0).sin() * 0.25 + (w * 3.0).sin() * 0.10
            })
            .collect();
        let buf = SampleBuffer::new_mono(samples, sr, 57);
        let cycle = buf.extract_wavetable_cycle_64();
        let peak = cycle.iter().fold(0.0_f32, |acc, &x| acc.max(x.abs()));
        let dc: f32 = cycle.iter().sum::<f32>() / 64.0;
        assert!(
            peak > 0.85 && peak <= 1.0,
            "Peak should be ~0.95, got {}",
            peak
        );
        assert!(dc.abs() < 0.08, "DC offset should be near zero, got {}", dc);
    }

    #[test]
    fn test_dual_oscillator_osc_b_and_fm_cross_modulation() {
        let sr = 48000.0;
        let samples: Vec<f32> = (0..4800)
            .map(|i| (i as f32 * 261.63 * std::f32::consts::TAU / sr).sin() * 0.45)
            .collect();
        let buf = SampleBuffer::new_mono(samples, sr, 60);

        let mut vm_dry = VoiceManager::new(sr, 4);
        vm_dry.note_on(60, 0.90, &buf);
        let mut dry_l = vec![0.0; 256];
        let mut dry_r = vec![0.0; 256];
        vm_dry.process_block(&buf, &mut dry_l, &mut dry_r);

        let mut vm_fm = VoiceManager::new(sr, 4);
        vm_fm.set_osc_b_fm_params(0.75, 0.50);
        assert!((vm_fm.fm_from_b() - 0.75).abs() < 1e-6);
        assert!((vm_fm.osc_b_level() - 0.50).abs() < 1e-6);

        vm_fm.note_on(60, 0.90, &buf);
        let mut fm_l = vec![0.0; 256];
        let mut fm_r = vec![0.0; 256];
        vm_fm.process_block(&buf, &mut fm_l, &mut fm_r);

        assert!(vm_fm.vital_serum_dsp_frames() >= 256);
        let spectral_diff: f32 = dry_l
            .iter()
            .zip(fm_l.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(
            spectral_diff > 5.0,
            "OSC B + FM A<-B must introduce strong harmonic modulation, diff={}",
            spectral_diff
        );
        assert!(fm_l.iter().all(|x| x.is_finite()));
        assert!(fm_r.iter().all(|x| x.is_finite()));
    }
}
