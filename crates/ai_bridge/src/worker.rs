//! Actor asíncrono para generación de samples por IA fuera del hilo de audio,
//! instrumentado con métricas de Observability-Driven Development (ODD), soporte de sesión persistente
//! y banco Multi-Sample de Keyzones (`Low / Mid / High`).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Instant;

use arc_swap::ArcSwap;
use crossbeam_channel::{bounded, Receiver as CbReceiver, Sender as CbSender};
use synth_core::{
    base64_decode, base64_encode, decode_wav, encode_wav_f32, SampleBuffer, SharedSampleBuffer,
};

use crate::cloudflare::{
    design_patch_spec_from_prompt, try_fetch_cloudflare_ai_gateway_wav, AiSynthesizerPatchSpec,
};
use crate::fallback_synth::generate_grand_piano_c4;
use crate::npu_synth::{generate_sample_with_cache, npu_cache_hits, npu_cache_misses};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerationStatus {
    Idle,
    Generating,
    Success,
    Error(String),
}

pub struct GenerateRequest {
    pub prompt: String,
    pub sample_rate: f32,
    pub endpoint_url: Option<String>,
}

pub struct AiWorker {
    request_tx: Sender<GenerateRequest>,
    pub current_sample: Arc<ArcSwap<SampleBuffer>>,
    is_busy: Arc<AtomicBool>,
    _worker_thread: Option<JoinHandle<()>>,
    sample_update_tx: CbSender<SharedSampleBuffer>,
    sample_update_rx: CbReceiver<SharedSampleBuffer>,

    // Banco de 3 Keyzones lock-free (0: Low, 1: Mid, 2: High)
    zone_samples: [Arc<ArcSwap<Option<SharedSampleBuffer>>>; 3],
    zone_update_tx: CbSender<(usize, Option<SharedSampleBuffer>)>,
    zone_update_rx: CbReceiver<(usize, Option<SharedSampleBuffer>)>,

    // Estado persistible y métricas ODD lock-free
    active_prompt: Arc<ArcSwap<String>>,
    custom_wav_b64: Arc<ArcSwap<Option<String>>>,
    last_backend: Arc<ArcSwap<String>>,
    last_generation_duration_us: Arc<AtomicU32>,
    generation_count: Arc<AtomicU32>,
    last_patch_spec: Arc<ArcSwap<Option<AiSynthesizerPatchSpec>>>,
    ai_presets_synthesized: Arc<AtomicU32>,
    piano_roll_auto_trims: Arc<AtomicU32>,
    last_detected_pitch_hz: Arc<AtomicU32>,
    last_trimmed_samples: Arc<AtomicU32>,
}

impl AiWorker {
    /// Inicializa el worker en segundo plano y carga el sample de piano acústico C4 de fábrica.
    pub fn new(sample_rate: f32) -> Self {
        let (tx, rx): (Sender<GenerateRequest>, Receiver<GenerateRequest>) = channel();
        let (sample_update_tx, sample_update_rx) = bounded(8);
        let (zone_update_tx, zone_update_rx) = bounded(16);

        let initial_sample = generate_grand_piano_c4(sample_rate);
        let current_sample = Arc::new(ArcSwap::from_pointee(initial_sample));
        let is_busy = Arc::new(AtomicBool::new(false));

        let zone_samples = [
            Arc::new(ArcSwap::from_pointee(None)),
            Arc::new(ArcSwap::from_pointee(None)),
            Arc::new(ArcSwap::from_pointee(None)),
        ];

        let active_prompt = Arc::new(ArcSwap::from_pointee("Concert Grand Piano C4".to_string()));
        let custom_wav_b64 = Arc::new(ArcSwap::from_pointee(None));
        let last_backend = Arc::new(ArcSwap::from_pointee("Physical-Modal-Piano".to_string()));
        let last_generation_duration_us = Arc::new(AtomicU32::new(0));
        let generation_count = Arc::new(AtomicU32::new(1));
        let (initial_spec, _) = design_patch_spec_from_prompt("Concert Grand Piano C4");
        let last_patch_spec = Arc::new(ArcSwap::from_pointee(Some(initial_spec)));
        let ai_presets_synthesized = Arc::new(AtomicU32::new(1));
        let piano_roll_auto_trims = Arc::new(AtomicU32::new(0));
        let last_detected_pitch_hz = Arc::new(AtomicU32::new(261.63_f32.to_bits()));
        let last_trimmed_samples = Arc::new(AtomicU32::new(0));

        let current_sample_clone = current_sample.clone();
        let is_busy_clone = is_busy.clone();
        let worker_update_tx = sample_update_tx.clone();
        let prompt_clone = active_prompt.clone();
        let wav_b64_clone = custom_wav_b64.clone();
        let backend_clone = last_backend.clone();
        let dur_clone = last_generation_duration_us.clone();
        let count_clone = generation_count.clone();
        let spec_clone = last_patch_spec.clone();
        let presets_synth_clone = ai_presets_synthesized.clone();
        let auto_trims_clone = piano_roll_auto_trims.clone();
        let pitch_hz_clone = last_detected_pitch_hz.clone();
        let trimmed_smp_clone = last_trimmed_samples.clone();

        let handle = thread::Builder::new()
            .name("ai-sample-worker".to_string())
            .spawn(move || {
                Self::worker_loop(
                    rx,
                    current_sample_clone,
                    is_busy_clone,
                    worker_update_tx,
                    prompt_clone,
                    wav_b64_clone,
                    backend_clone,
                    dur_clone,
                    count_clone,
                    spec_clone,
                    presets_synth_clone,
                    auto_trims_clone,
                    pitch_hz_clone,
                    trimmed_smp_clone,
                );
            })
            .expect("Failed to spawn ai-sample-worker thread");

        Self {
            request_tx: tx,
            current_sample,
            is_busy,
            _worker_thread: Some(handle),
            sample_update_tx,
            sample_update_rx,
            zone_samples,
            zone_update_tx,
            zone_update_rx,
            active_prompt,
            custom_wav_b64,
            last_backend,
            last_generation_duration_us,
            generation_count,
            last_patch_spec,
            ai_presets_synthesized,
            piano_roll_auto_trims,
            last_detected_pitch_hz,
            last_trimmed_samples,
        }
    }

    /// Asigna el sample activo actual a una de las 3 Keyzones (`0 = Low`, `1 = Mid`, `2 = High`).
    pub fn assign_current_to_zone(&self, zone_idx: usize) -> Option<SharedSampleBuffer> {
        if zone_idx >= 3 {
            return None;
        }
        let sample = self.get_sample();
        self.zone_samples[zone_idx].store(Arc::new(Some(sample.clone())));
        while self.zone_update_tx.is_full() {
            let _ = self.zone_update_rx.try_recv();
        }
        let _ = self
            .zone_update_tx
            .try_send((zone_idx, Some(sample.clone())));
        Some(sample)
    }

    /// Limpia todas las asignaciones de Keyzones, regresando al modo cromático global único.
    pub fn clear_zones(&self) {
        for idx in 0..3 {
            self.zone_samples[idx].store(Arc::new(None));
            while self.zone_update_tx.is_full() {
                let _ = self.zone_update_rx.try_recv();
            }
            let _ = self.zone_update_tx.try_send((idx, None));
        }
    }

    /// Retorna la máscara de bits de las Keyzones activas (`bit 0 = Low`, `bit 1 = Mid`, `bit 2 = High`).
    pub fn active_zones_mask(&self) -> u8 {
        let mut mask = 0u8;
        for idx in 0..3 {
            if self.zone_samples[idx].load().is_some() {
                mask |= 1 << idx;
            }
        }
        mask
    }

    /// Retorna los nombres de los samples asignados a cada una de las 3 Keyzones.
    pub fn zone_names(&self) -> [Option<String>; 3] {
        [
            self.zone_samples[0]
                .load()
                .as_ref()
                .as_ref()
                .map(|s| s.name.clone()),
            self.zone_samples[1]
                .load()
                .as_ref()
                .as_ref()
                .map(|s| s.name.clone()),
            self.zone_samples[2]
                .load()
                .as_ref()
                .as_ref()
                .map(|s| s.name.clone()),
        ]
    }

    /// Exporta los buffers WAV IEEE Float 32-bit en Base64 de las 3 Keyzones activas para persistencia `.tepui` y DAW.
    pub fn export_zones_b64(&self) -> [Option<String>; 3] {
        let mut out = [None, None, None];
        for (idx, slot) in out.iter_mut().enumerate() {
            if let Some(sample) = self.zone_samples[idx].load().as_ref() {
                let wav_bytes = encode_wav_f32(sample);
                *slot = Some(base64_encode(&wav_bytes));
            }
        }
        out
    }

    /// Restaura las 3 Keyzones desde sus buffers WAV Base64 y nombres persistidos, notificando al hilo de audio.
    pub fn restore_zones_b64(
        &self,
        zone_wavs: &[Option<String>; 3],
        zone_names: &[Option<String>; 3],
    ) -> Result<(), String> {
        for idx in 0..3 {
            let restored = if let Some(b64) = &zone_wavs[idx] {
                if !b64.trim().is_empty() {
                    let bytes = base64_decode(b64)?;
                    let mut buf = decode_wav(&bytes)?;
                    if let Some(name) = &zone_names[idx] {
                        if !name.trim().is_empty() {
                            buf.name = name.trim().to_string();
                        }
                    }
                    Some(Arc::new(buf))
                } else {
                    None
                }
            } else {
                None
            };
            self.zone_samples[idx].store(Arc::new(restored.clone()));
            while self.zone_update_tx.is_full() {
                let _ = self.zone_update_rx.try_recv();
            }
            let _ = self.zone_update_tx.try_send((idx, restored));
        }
        Ok(())
    }

    /// Retorna el canal de suscripción para recibir actualizaciones de Keyzones en el hilo de audio.
    pub fn take_zone_receiver(&self) -> CbReceiver<(usize, Option<SharedSampleBuffer>)> {
        self.zone_update_rx.clone()
    }

    /// Envía una solicitud de generación de sample sin bloquear el hilo que llama.
    pub fn request_generation(
        &self,
        prompt: String,
        sample_rate: f32,
        endpoint_url: Option<String>,
    ) {
        self.active_prompt.store(Arc::new(prompt.clone()));
        self.custom_wav_b64.store(Arc::new(None));
        let _ = self.request_tx.send(GenerateRequest {
            prompt,
            sample_rate,
            endpoint_url,
        });
    }

    /// Decodifica un archivo WAV en memoria, lo calibra para Piano Roll (`Auto-Onset Trim` + `Auto-Pitch YIN`)
    /// y lo publica de forma atómica tanto para la UI como para el hilo de audio en tiempo real (0 bloqueos).
    pub fn load_wav_bytes(
        &self,
        bytes: &[u8],
        custom_name: Option<String>,
    ) -> Result<SharedSampleBuffer, String> {
        let t0 = Instant::now();
        let mut buffer = decode_wav(bytes)?;
        if let Some(name) = custom_name {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                buffer.name = trimmed.to_string();
            }
        }
        let prep = buffer.prepare_for_piano_roll();
        if prep.trimmed_leading_samples > 0 || prep.is_pitched {
            self.piano_roll_auto_trims.fetch_add(1, Ordering::Relaxed);
        }
        self.last_trimmed_samples
            .store(prep.trimmed_leading_samples as u32, Ordering::Relaxed);
        if prep.detected_f0_hz > 0.0 {
            self.last_detected_pitch_hz
                .store(prep.detected_f0_hz.to_bits(), Ordering::Relaxed);
        }

        let shared = Arc::new(buffer);
        self.current_sample.store(shared.clone());
        self.active_prompt.store(Arc::new(shared.name.clone()));
        self.custom_wav_b64
            .store(Arc::new(Some(base64_encode(bytes))));
        self.last_backend
            .store(Arc::new("Custom-WAV-Memory".to_string()));
        self.last_generation_duration_us.store(
            t0.elapsed().as_micros().min(u32::MAX as u128) as u32,
            Ordering::Relaxed,
        );
        self.generation_count.fetch_add(1, Ordering::Relaxed);

        while self.sample_update_tx.is_full() {
            let _ = self.sample_update_rx.try_recv();
        }
        let _ = self.sample_update_tx.try_send(shared.clone());
        Ok(shared)
    }

    /// Restaura una sesión persistida desde el DAW (WAV Base64 personalizado o prompt de IA).
    pub fn restore_session(
        &self,
        prompt: &str,
        wav_b64: Option<&str>,
        sample_rate: f32,
    ) -> Result<(), String> {
        if let Some(b64) = wav_b64 {
            if !b64.trim().is_empty() {
                let bytes = base64_decode(b64)?;
                let name = if prompt.trim().is_empty() {
                    None
                } else {
                    Some(prompt.trim().to_string())
                };
                self.load_wav_bytes(&bytes, name)?;
                return Ok(());
            }
        }
        let clean_prompt = if prompt.trim().is_empty() {
            "Concert Grand Piano C4".to_string()
        } else {
            prompt.trim().to_string()
        };
        self.request_generation(clean_prompt, sample_rate, None);
        Ok(())
    }

    /// Retorna una referencia compartida segura del sample activo.
    pub fn get_sample(&self) -> SharedSampleBuffer {
        self.current_sample.load_full()
    }

    /// Retorna el canal de suscripción para recibir samples nuevos en el hilo de audio con 0 asignaciones.
    pub fn take_sample_receiver(&self) -> CbReceiver<SharedSampleBuffer> {
        self.sample_update_rx.clone()
    }

    /// Indica si el worker está actualmente sintetizando un sample.
    pub fn is_generating(&self) -> bool {
        self.is_busy.load(Ordering::Relaxed)
    }

    /// Retorna el prompt o identificador de sample activo (lock-free).
    pub fn active_prompt(&self) -> String {
        (**self.active_prompt.load()).clone()
    }

    /// Retorna el WAV personalizado en Base64 si el sample actual provino de importación WAV.
    pub fn custom_wav_b64(&self) -> Option<String> {
        (**self.custom_wav_b64.load()).clone()
    }

    /// Retorna el nombre del backend de síntesis empleado en la última generación (ODD).
    pub fn last_backend(&self) -> String {
        (**self.last_backend.load()).clone()
    }

    /// Retorna la duración en microsegundos de la última síntesis/decodificación (ODD).
    pub fn last_generation_duration_us(&self) -> u32 {
        self.last_generation_duration_us.load(Ordering::Relaxed)
    }

    /// Retorna el total acumulado de generaciones o cargas en la sesión (ODD).
    pub fn generation_count(&self) -> u32 {
        self.generation_count.load(Ordering::Relaxed)
    }

    /// Retorna el total acumulado de aciertos (hits) en la caché de samples NPU (ODD).
    pub fn cache_hits(&self) -> u32 {
        npu_cache_hits()
    }

    /// Retorna el total acumulado de fallos (misses) en la caché de samples NPU (ODD).
    pub fn cache_misses(&self) -> u32 {
        npu_cache_misses()
    }

    /// Retorna la última especificación de preset + receta espectral generada por IA (lock-free).
    pub fn last_patch_spec(&self) -> Option<AiSynthesizerPatchSpec> {
        (**self.last_patch_spec.load()).clone()
    }

    /// Retorna el total acumulado de presets sintetizados por el arquitecto IA (ODD).
    pub fn ai_presets_synthesized(&self) -> u32 {
        self.ai_presets_synthesized.load(Ordering::Relaxed)
    }

    /// Retorna el total acumulado de calibraciones `Piano-Roll Ready` (Auto-Onset Trim + Auto-Pitch YIN) (ODD).
    pub fn piano_roll_auto_trims(&self) -> u32 {
        self.piano_roll_auto_trims.load(Ordering::Relaxed)
    }

    /// Retorna la última frecuencia fundamental `f0` detectada en Hz (ODD).
    pub fn last_detected_pitch_hz(&self) -> f32 {
        f32::from_bits(self.last_detected_pitch_hz.load(Ordering::Relaxed))
    }

    /// Retorna la cantidad de muestras de silencio inicial recortadas en el último sample (ODD).
    pub fn last_trimmed_samples(&self) -> u32 {
        self.last_trimmed_samples.load(Ordering::Relaxed)
    }

    fn try_load_from_endpoint(endpoint: &str, prompt: &str) -> Option<SampleBuffer> {
        let clean = endpoint
            .trim()
            .strip_prefix("file://")
            .unwrap_or(endpoint.trim());
        let path = std::path::Path::new(clean);
        if path.exists() && path.is_file() {
            if let Ok(bytes) = std::fs::read(path) {
                if let Ok(mut buf) = decode_wav(&bytes) {
                    buf.name = format!("AI Endpoint ({})", prompt.trim());
                    let _ = buf.prepare_for_piano_roll();
                    return Some(buf);
                }
            }
        }
        None
    }

    #[allow(clippy::too_many_arguments)]
    fn worker_loop(
        rx: Receiver<GenerateRequest>,
        current_sample: Arc<ArcSwap<SampleBuffer>>,
        is_busy: Arc<AtomicBool>,
        sample_update_tx: CbSender<SharedSampleBuffer>,
        active_prompt: Arc<ArcSwap<String>>,
        custom_wav_b64: Arc<ArcSwap<Option<String>>>,
        last_backend: Arc<ArcSwap<String>>,
        last_generation_duration_us: Arc<AtomicU32>,
        generation_count: Arc<AtomicU32>,
        last_patch_spec: Arc<ArcSwap<Option<AiSynthesizerPatchSpec>>>,
        ai_presets_synthesized: Arc<AtomicU32>,
        piano_roll_auto_trims: Arc<AtomicU32>,
        last_detected_pitch_hz: Arc<AtomicU32>,
        last_trimmed_samples: Arc<AtomicU32>,
    ) {
        while let Ok(req) = rx.recv() {
            is_busy.store(true, Ordering::SeqCst);
            let t0 = Instant::now();

            // 1. Diseñar el Preset del sintetizador + Receta de parciales modales (Cloudflare Workers AI / NPU)
            let (patch_spec, _spec_backend) = design_patch_spec_from_prompt(&req.prompt);
            last_patch_spec.store(Arc::new(Some(patch_spec.clone())));
            ai_presets_synthesized.fetch_add(1, Ordering::Relaxed);

            let effective_endpoint = req
                .endpoint_url
                .or_else(|| std::env::var("TEPUI_AI_ENDPOINT").ok());

            let (mut new_buffer, backend_str, is_external) = if let Some(buf) = effective_endpoint
                .as_deref()
                .and_then(|ep| Self::try_load_from_endpoint(ep, &req.prompt))
            {
                (buf, "AI-Endpoint-WAV", true)
            } else if let Some(gw_buf) =
                try_fetch_cloudflare_ai_gateway_wav(&req.prompt, patch_spec.root_note)
            {
                (gw_buf, "Cloudflare-AIGateway-WAV", true)
            } else {
                let (buf, b_str, _is_hit) =
                    generate_sample_with_cache(&req.prompt, req.sample_rate);
                (buf, b_str, false)
            };

            let orig_root = new_buffer.root_note;
            let prep = new_buffer.prepare_for_piano_roll();
            if !is_external {
                // Conservar la nota raíz exacta del modelo físico/espectral local
                new_buffer.root_note = orig_root;
                new_buffer.tune_cents = 0.0;
            }
            piano_roll_auto_trims.fetch_add(1, Ordering::Relaxed);
            last_trimmed_samples.store(prep.trimmed_leading_samples as u32, Ordering::Relaxed);
            let reported_f0 = if prep.detected_f0_hz > 0.0 {
                prep.detected_f0_hz
            } else {
                440.0 * 2.0_f32.powf((new_buffer.root_note as f32 - 69.0) / 12.0)
            };
            last_detected_pitch_hz.store(reported_f0.to_bits(), Ordering::Relaxed);

            let elapsed_us = t0.elapsed().as_micros().min(u32::MAX as u128) as u32;
            last_generation_duration_us.store(elapsed_us.max(1), Ordering::Relaxed);
            last_backend.store(Arc::new(backend_str.to_string()));
            active_prompt.store(Arc::new(req.prompt.clone()));
            custom_wav_b64.store(Arc::new(None));
            generation_count.fetch_add(1, Ordering::Relaxed);

            let new_sample = Arc::new(new_buffer);
            current_sample.store(new_sample.clone());
            let _ = sample_update_tx.try_send(new_sample);

            is_busy.store(false, Ordering::SeqCst);
        }
    }
}
