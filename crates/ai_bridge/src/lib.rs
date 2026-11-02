//! Puente asíncrono para generación de samples por IA.
#![allow(unknown_lints, clippy::chunks_exact_to_as_chunks)]

pub mod fallback_synth;
pub mod npu_synth;
pub mod worker;

pub use fallback_synth::{
    generate_grand_piano_c4, generate_sample_from_prompt, generate_sample_from_prompt_with_backend,
};
pub use npu_synth::{
    extract_midi_note_from_prompt, generate_neural_spectral_sample, SemanticTimbreDescriptor,
};
pub use worker::{AiWorker, GenerateRequest, GenerationStatus};

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;
    use synth_core::{encode_wav_f32, SampleBuffer};

    #[test]
    fn test_grand_piano_c4_generation() {
        let sample = generate_grand_piano_c4(44100.0);
        assert!(!sample.is_empty());
        assert_eq!(sample.root_note, 60); // C4
        assert_eq!(sample.name, "Concert Grand Piano");
        assert!(sample.duration_seconds() > 3.0);
        assert!(!sample.loop_enabled, "Acoustic piano decays naturally without loop");
        assert!(!sample.is_empty());

        // Comprobar que no hay clipping (pico <= 1.0)
        let mut max_val: f32 = 0.0;
        for &s in sample.left.iter().chain(sample.right.iter()) {
            assert!(!s.is_nan());
            assert!(!s.is_infinite());
            let abs = s.abs();
            if abs > max_val {
                max_val = abs;
            }
        }
        assert!(max_val <= 1.0, "Sample exceeds 1.0 peak: {}", max_val);
        assert!(max_val > 0.5, "Sample is too quiet: {}", max_val);
    }

    #[test]
    fn test_all_seven_presets_generation() {
        let cases = [
            ("Concert Grand Piano C4", "Concert Grand Piano", 60u8),
            ("Cuatro Venezolano Camburpinton C4", "Cuatro Venezolano", 60u8),
            ("Arpa Llanera Venezolana C4", "Arpa Llanera Venezolana", 60u8),
            ("Bandola Llanera Venezolana A3", "Bandola Llanera Venezolana", 57u8),
            ("Maracas Llaneras Capachos C4", "Maracas Llaneras Capachos", 60u8),
            ("Tambor Cumaco Venezolano C3", "Tambor Cumaco Venezolano", 48u8),
            ("Quitiplas de Barlovento C4", "Quitiplas de Barlovento", 60u8),
            ("Furruco Zuliano Sub C2", "Furruco Zuliano Sub", 36u8),
            ("Warm Analog Synth Pad", "Warm Analog Pad", 60u8),
            ("Metallic FM Bell Chime", "Metallic FM Bell", 72u8),
            ("Deep 808 Sub Bass", "Deep 808 Sub Bass", 36u8),
            ("Acoustic Solo Cello Expression", "Solo Acoustic Cello", 48u8),
            ("Vintage Brass Ensemble Horns", "Vintage Brass Horns", 60u8),
            ("Cyberpunk Pluck Lead Synth", "Cyberpunk Pluck Lead", 60u8),
        ];

        for (prompt, expected_name, expected_root) in cases {
            let buf = generate_sample_from_prompt(prompt, 44100.0);
            assert!(!buf.is_empty(), "Preset '{}' produced empty buffer", prompt);
            assert_eq!(buf.name, expected_name, "Mismatch for prompt '{}'", prompt);
            assert_eq!(buf.root_note, expected_root, "Root note mismatch for '{}'", prompt);
            let peak = buf.left.iter().fold(0.0f32, |acc, &x| acc.max(x.abs()));
            assert!(peak > 0.1 && peak <= 1.0, "Invalid peak {} for '{}'", peak, prompt);
        }
    }

    #[test]
    fn test_ai_worker_load_wav_bytes() {
        let worker = AiWorker::new(44100.0);
        let rx = worker.take_sample_receiver();

        let custom = SampleBuffer::new_mono(vec![0.1, 0.4, -0.4, 0.2], 48000.0, 60);
        let wav_bytes = encode_wav_f32(&custom);

        let loaded = worker
            .load_wav_bytes(&wav_bytes, Some("Custom Synth Shot.wav".to_string()))
            .expect("load_wav_bytes failed");

        assert_eq!(loaded.name, "Custom Synth Shot.wav");
        assert_eq!(loaded.len(), 4);
        assert_eq!(loaded.sample_rate, 48000.0);

        let from_rx = rx.try_recv().expect("Expected sample in audio thread channel");
        assert_eq!(from_rx.name, "Custom Synth Shot.wav");
    }

    #[test]
    fn test_ai_worker_async_generation() {
        let worker = AiWorker::new(44100.0);
        let initial_sample = worker.get_sample();
        assert!(!initial_sample.is_empty());
        assert_eq!(initial_sample.root_note, 60);

        // Solicitar un nuevo sample (FM Bell)
        worker.request_generation("Metallic FM Bell Chime".to_string(), 44100.0, None);

        // Esperar brevemente a que el hilo en background procese la solicitud
        thread::sleep(Duration::from_millis(150));

        let updated_sample = worker.get_sample();
        assert!(!updated_sample.is_empty());
        assert!(worker.generation_count() >= 2);
        assert!(worker.last_generation_duration_us() > 0);
    }

    #[test]
    fn test_npu_neural_spectral_open_prompts_and_note_extraction() {
        assert_eq!(extract_midi_note_from_prompt("Dark Dystopian Sub Drone C2"), Some(36));
        assert_eq!(extract_midi_note_from_prompt("Crystal Glass Harp E4"), Some(64));
        assert_eq!(extract_midi_note_from_prompt("Shimmering Choir F#3"), Some(54));

        let (dark_drone, backend1) = generate_sample_from_prompt_with_backend("Dark Dystopian Drone C2", 44100.0);
        assert_eq!(dark_drone.root_note, 36);
        assert!(dark_drone.loop_enabled, "Drone should automatically enable sustain loop");
        assert!(backend1 == "Neural-Spectral-DSP" || backend1 == "OpenVINO-NPU");

        let (crystal_harp, _) = generate_sample_from_prompt_with_backend("Bright Crystal Harp Pluck E4", 44100.0);
        assert_eq!(crystal_harp.root_note, 64);
        assert!(!crystal_harp.loop_enabled, "Pluck harp should have percussive non-looped envelope");
    }
}
