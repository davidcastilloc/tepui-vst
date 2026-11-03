//! Puente asíncrono para generación de samples por IA.
#![allow(
    unknown_lints,
    clippy::chunks_exact_to_as_chunks,
    clippy::approx_constant
)]

pub mod cloudflare;
pub mod fallback_synth;
pub mod npu_synth;
pub mod worker;

pub use cloudflare::{
    build_cloudflare_workers_ai_preset_request, compute_hmac_sha256_hex,
    design_patch_spec_from_prompt, design_patch_spec_with_cloudflare_live,
    format_piano_roll_oneshot_prompt, load_cloud_license_config, process_checkout_webhook_payload,
    redeem_cloud_license_code, save_cloud_license_config, synthesize_sample_from_patch_spec,
    try_fetch_cloudflare_edge_worker_patch, try_fetch_cloudflare_edge_worker_patch_with_report,
    verify_checkout_webhook_hmac_sha256, verify_cloud_license_quota, AiSynthesizerPatchSpec,
    CloudEdgeQuotaReport, CloudLicenseConfig, ModalPartialSpec, DEFAULT_CF_EDGE_WORKER_URL,
    DEFAULT_CF_PRESET_MODEL, DEFAULT_CF_SAMPLE_DIFFUSION_MODEL, MAX_DAILY_FREE_POOL_NEURONS,
};
pub use fallback_synth::{
    generate_grand_piano_c4, generate_sample_from_prompt, generate_sample_from_prompt_with_backend,
};
pub use npu_synth::{
    extract_midi_note_from_prompt, generate_neural_spectral_sample, generate_sample_with_cache,
    npu_cache_hits, npu_cache_misses, SemanticTimbreDescriptor,
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
        assert!(
            !sample.loop_enabled,
            "Acoustic piano decays naturally without loop"
        );
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
            (
                "Cuatro Venezolano Camburpinton C4",
                "Cuatro Venezolano",
                60u8,
            ),
            (
                "Arpa Llanera Venezolana C4",
                "Arpa Llanera Venezolana",
                60u8,
            ),
            (
                "Bandola Llanera Venezolana A3",
                "Bandola Llanera Venezolana",
                57u8,
            ),
            (
                "Maracas Llaneras Capachos C4",
                "Maracas Llaneras Capachos",
                60u8,
            ),
            (
                "Tambor Cumaco Venezolano C3",
                "Tambor Cumaco Venezolano",
                48u8,
            ),
            (
                "Quitiplas de Barlovento C4",
                "Quitiplas de Barlovento",
                60u8,
            ),
            ("Furruco Zuliano Sub C2", "Furruco Zuliano Sub", 36u8),
            ("Warm Analog Synth Pad", "Warm Analog Pad", 60u8),
            ("Metallic FM Bell Chime", "Metallic FM Bell", 72u8),
            ("Deep 808 Sub Bass", "Deep 808 Sub Bass", 36u8),
            (
                "Acoustic Solo Cello Expression",
                "Solo Acoustic Cello",
                48u8,
            ),
            ("Vintage Brass Ensemble Horns", "Vintage Brass Horns", 60u8),
            ("Cyberpunk Pluck Lead Synth", "Cyberpunk Pluck Lead", 60u8),
        ];

        for (prompt, expected_name, expected_root) in cases {
            let buf = generate_sample_from_prompt(prompt, 44100.0);
            assert!(!buf.is_empty(), "Preset '{}' produced empty buffer", prompt);
            assert_eq!(buf.name, expected_name, "Mismatch for prompt '{}'", prompt);
            assert_eq!(
                buf.root_note, expected_root,
                "Root note mismatch for '{}'",
                prompt
            );
            let peak = buf.left.iter().fold(0.0f32, |acc, &x| acc.max(x.abs()));
            assert!(
                peak > 0.1 && peak <= 1.0,
                "Invalid peak {} for '{}'",
                peak,
                prompt
            );
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

        let from_rx = rx
            .try_recv()
            .expect("Expected sample in audio thread channel");
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
        assert_eq!(
            extract_midi_note_from_prompt("Dark Dystopian Sub Drone C2"),
            Some(36)
        );
        assert_eq!(
            extract_midi_note_from_prompt("Crystal Glass Harp E4"),
            Some(64)
        );
        assert_eq!(
            extract_midi_note_from_prompt("Shimmering Choir F#3"),
            Some(54)
        );

        let (dark_drone, backend1) =
            generate_sample_from_prompt_with_backend("Dark Dystopian Drone C2", 44100.0);
        assert_eq!(dark_drone.root_note, 36);
        assert!(
            dark_drone.loop_enabled,
            "Drone should automatically enable sustain loop"
        );
        assert!(backend1 == "Neural-Spectral-DSP" || backend1 == "OpenVINO-NPU");

        let (crystal_harp, _) =
            generate_sample_from_prompt_with_backend("Bright Crystal Harp Pluck E4", 44100.0);
        assert_eq!(crystal_harp.root_note, 64);
        assert!(
            !crystal_harp.loop_enabled,
            "Pluck harp should have percussive non-looped envelope"
        );
    }

    #[test]
    fn test_npu_sample_cache_hits_and_misses_odd() {
        let unique_prompt = format!(
            "Gran Sabana Reese Sub Urbano C2 #{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let hits_before = npu_cache_hits();
        let misses_before = npu_cache_misses();

        let (buf1, _backend1, is_hit1) = generate_sample_with_cache(&unique_prompt, 44100.0);
        assert!(
            !is_hit1,
            "First request for unique prompt should be a cache miss"
        );
        assert!(!buf1.is_empty());
        assert!(npu_cache_misses() > misses_before);

        let (buf2, backend2, is_hit2) = generate_sample_with_cache(&unique_prompt, 44100.0);
        assert!(
            is_hit2,
            "Second request for identical prompt should be a cache hit"
        );
        assert!(backend2 == "NPU-RAM-Cache" || backend2 == "NPU-Disk-Cache");
        assert_eq!(buf1.len(), buf2.len());
        assert!(npu_cache_hits() > hits_before);
    }

    #[test]
    fn test_cloudflare_patch_spec_and_piano_roll_oneshot_synthesis() {
        let prompt = "Bright Joropo Arpa Llanera Pluck E4";
        let (spec, _backend) = design_patch_spec_from_prompt(prompt);
        assert_eq!(spec.root_note, 64); // E4 = 64
        assert_eq!(spec.arp_mode, 3); // Joropo 6/8
        assert!(spec.attack < 0.02, "Pluck should have fast attack");
        assert!(spec.cutoff > 5000.0, "Bright arpa should have open filter");
        assert_eq!(spec.partials.len(), 10);

        let cf_payload = build_cloudflare_workers_ai_preset_request(prompt);
        assert_eq!(
            cf_payload["response_format"]["type"],
            serde_json::json!("json_schema")
        );

        let oneshot_prompt = format_piano_roll_oneshot_prompt(prompt, spec.root_note);
        assert!(oneshot_prompt.contains("TrackType: Instrument"));
        assert!(oneshot_prompt.contains("Single isolated note E4"));

        let (synth_buf, prep_report) = synthesize_sample_from_patch_spec(&spec, 48000.0);
        assert!(!synth_buf.is_empty());
        assert!(prep_report.is_pitched);
        assert_eq!(prep_report.detected_root_note, 64);
    }

    #[test]
    fn test_pro_synth_reese_supersaw_acid_vocal_and_karplus_strong() {
        let pro_cases = [
            ("Neuro Reese Bass Sub C2", "Neuro Reese Bass", 36u8, true),
            (
                "Hyper Supersaw Anthem Lead C4",
                "Hyper Supersaw Lead",
                60u8,
                true,
            ),
            (
                "Resonant Acid 303 Squelch C2",
                "Resonant Acid 303",
                36u8,
                false,
            ),
            (
                "Crystal Glass FM Pluck C4",
                "Crystal Glass FM Pluck",
                60u8,
                false,
            ),
            (
                "Ethereal Formant Vocal Pad C4",
                "Ethereal Formant Vocal Pad",
                60u8,
                true,
            ),
            (
                "Karplus Nylon Guitar Pluck C4",
                "Karplus Nylon Guitar",
                60u8,
                false,
            ),
        ];

        for (prompt, expected_name, expected_root, expected_loop) in pro_cases {
            assert!(
                fallback_synth::is_dedicated_dsp_preset(prompt),
                "Prompt '{}' must be recognized as a dedicated DSP preset",
                prompt
            );
            let buf = generate_sample_from_prompt(prompt, 44100.0);
            assert!(!buf.is_empty());
            assert_eq!(buf.name, expected_name);
            assert_eq!(buf.root_note, expected_root);
            assert_eq!(buf.loop_enabled, expected_loop);
            let peak = buf.left.iter().fold(0.0f32, |acc, &x| acc.max(x.abs()));
            assert!(peak > 0.3 && peak <= 1.0, "Peak {} out of range", peak);
        }
    }

    #[test]
    fn test_cloud_license_redemption_and_hmac_sha256_webhook_idempotency() {
        std::env::set_var("TEPUI_OFFLINE_ONLY", "1");

        // 1. Verificar vector RFC 4231 de HMAC-SHA256
        let rfc_key = [0x0bu8; 20];
        let rfc_data = b"Hi There";
        let expected_rfc_hmac = "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7";
        assert_eq!(
            compute_hmac_sha256_hex(rfc_data, &rfc_key),
            expected_rfc_hmac
        );
        assert!(verify_checkout_webhook_hmac_sha256(
            rfc_data,
            expected_rfc_hmac,
            &rfc_key
        ));
        assert!(!verify_checkout_webhook_hmac_sha256(
            rfc_data,
            "0000000000000000000000000000000000000000000000000000000000000000",
            &rfc_key
        ));

        // 2. Canje de código Pro Cloud e idempotencia (no doble acreditación)
        let mut cfg = CloudLicenseConfig::default();
        let r1 = redeem_cloud_license_code("tepui-pro-2026-gold", &mut cfg)
            .expect("El primer canje válido debe acreditarse");
        assert_eq!(r1.tier, "PRO CLOUD");
        assert_eq!(cfg.redeemed_credits, 500);
        assert!(cfg.pro_tier_unlocked);
        assert_eq!(cfg.license_key, "TEPUI-PRO-2026-GOLD");

        let dup = redeem_cloud_license_code("TEPUI-PRO-2026-GOLD", &mut cfg);
        assert!(
            dup.is_err(),
            "Re-canjear el mismo código debe fallar por idempotencia"
        );
        assert_eq!(cfg.redeemed_credits, 500);

        // 3. Webhook de checkout con verificación HMAC-SHA256 e idempotencia
        let secret = "whsec_tepui_production_test";
        let payload =
            br#"{"event_id":"evt_checkout_9901","credits":1000,"license_code":"TEPUI-VIP-9901"}"#;
        let bad_res = process_checkout_webhook_payload(
            payload,
            "sha256=deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
            secret,
            &mut cfg,
        );
        assert!(bad_res.is_err());
        assert!(bad_res.unwrap_err().contains("HTTP 401"));

        let valid_sig = format!(
            "sha256={}",
            compute_hmac_sha256_hex(payload, secret.as_bytes())
        );
        let wh_report = process_checkout_webhook_payload(payload, &valid_sig, secret, &mut cfg)
            .expect("Webhook firmado válidamente debe acreditarse");
        assert_eq!(wh_report.tier, "PRO CLOUD");
        assert_eq!(cfg.redeemed_credits, 1500);

        // Reenvío del mismo webhook debe ser idempotente (sin duplicar créditos)
        let wh_dup = process_checkout_webhook_payload(payload, &valid_sig, secret, &mut cfg)
            .expect("Reenvío idempotente debe responder OK sin duplicar");
        assert!(wh_dup.cached);
        assert_eq!(cfg.redeemed_credits, 1500);
    }
}
