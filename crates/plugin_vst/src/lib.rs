//! Plugin Principal VST3 y CLAP de Sampler CromÃ¡tico con GeneraciÃ³n de IA.
//!
//! Desarrollado en Rust con nih-plug, garantizando seguridad en tiempo real
//! y cero asignaciones dinÃ¡micas en el ciclo de audio.
#![allow(
    unknown_lints,
    semicolon_in_expressions_from_non_local_macros,
    clippy::chunks_exact_to_as_chunks
)]

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use ai_bridge::AiWorker;
use crossbeam_channel::{bounded, Receiver, Sender};
use nih_plug::prelude::*;
use synth_core::{StereoFxRack, VoiceManager};

pub mod audio_devices;
mod params;
pub mod ui_dsp_contract;
pub mod web_editor;

use params::{LiveParamStore, SamplerParams};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GuiMidiEvent {
    NoteOn(u8, f32),
    NoteOff(u8),
    PitchBend(f32),
    ModWheel(f32),
    SustainPedal(bool),
    Aftertouch(f32),
    PolyAftertouch(u8, f32),
    NotePitchBend(u8, f32),
    MidiCC(u8, f32),
    BrightnessCC74(f32),
    SetVelocity(f32),
    AllNotesOff,
}

#[derive(Clone, Copy)]
struct LastDawParams {
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
    cutoff: f32,
    resonance: f32,
    gain: f32,
    space_mix: f32,
    filter_mode: i32,
    root_note: i32,
    lfo_rate: f32,
    lfo_depth: f32,
    lfo_target: i32,
    lfo_shape: i32,
    arp_mode: i32,
    wt_pos: f32,
    wt_warp: f32,
    unison_voices: i32,
    unison_detune: f32,
    sub_level: f32,
    fm_from_b: f32,
    drive_amount: f32,
    ott_amount: f32,
    filter_keytrack: f32,
    macro1: f32,
    macro2: f32,
    macro3: f32,
    macro4: f32,
}

impl LastDawParams {
    fn from_params(params: &SamplerParams) -> Self {
        Self {
            attack: params.attack.value(),
            decay: params.decay.value(),
            sustain: params.sustain.value(),
            release: params.release.value(),
            cutoff: params.cutoff.value(),
            resonance: params.resonance.value(),
            gain: params.gain.value(),
            space_mix: params.space_mix.value(),
            filter_mode: params.filter_mode.value(),
            root_note: params.root_note.value(),
            lfo_rate: params.lfo_rate.value(),
            lfo_depth: params.lfo_depth.value(),
            lfo_target: params.lfo_target.value(),
            lfo_shape: params.lfo_shape.value(),
            arp_mode: params.arp_mode.value(),
            wt_pos: params.wt_pos.value(),
            wt_warp: params.wt_warp.value(),
            unison_voices: params.unison_voices.value(),
            unison_detune: params.unison_detune.value(),
            sub_level: params.sub_level.value(),
            fm_from_b: params.fm_from_b.value(),
            drive_amount: params.drive_amount.value(),
            ott_amount: params.ott_amount.value(),
            filter_keytrack: params.filter_keytrack.value(),
            macro1: params.macro1.value(),
            macro2: params.macro2.value(),
            macro3: params.macro3.value(),
            macro4: params.macro4.value(),
        }
    }
}

pub struct AiSamplerPlugin {
    params: Arc<SamplerParams>,
    live_params: Arc<LiveParamStore>,
    voice_manager: VoiceManager,
    fx_rack: StereoFxRack,
    ai_worker: Arc<AiWorker>,
    active_sample: Arc<synth_core::SampleBuffer>,
    sample_update_rx: Receiver<synth_core::SharedSampleBuffer>,
    active_zones: [Option<synth_core::SharedSampleBuffer>; 3],
    zone_update_rx: Receiver<(usize, Option<synth_core::SharedSampleBuffer>)>,
    sample_rate: f32,
    sample_rate_atomic: Arc<AtomicU32>,
    gui_midi_rx: Receiver<GuiMidiEvent>,
    gui_midi_tx: Sender<GuiMidiEvent>,
    web_gui_tx: Sender<web_editor::WebGuiEvent>,
    web_gui_rx: Receiver<web_editor::WebGuiEvent>,
    last_daw: LastDawParams,
    last_custom_wt_rev: u32,
    smoothed_gain: f32,
    mono_scratch_buffer: Vec<f32>,
}

impl Default for AiSamplerPlugin {
    fn default() -> Self {
        let sample_rate: f32 = 44100.0;
        let sample_rate_atomic = Arc::new(AtomicU32::new(sample_rate.to_bits()));
        let ai_worker = Arc::new(AiWorker::new(sample_rate));
        let active_sample = ai_worker.get_sample();
        let sample_update_rx = ai_worker.take_sample_receiver();
        let zone_update_rx = ai_worker.take_zone_receiver();
        let voice_manager = VoiceManager::new(sample_rate, 16);
        let fx_rack = StereoFxRack::new(sample_rate);
        let (gui_midi_tx, gui_midi_rx) = bounded(1024);
        let (web_gui_tx, web_gui_rx) = bounded(512);
        if audio_devices::is_standalone() {
            audio_devices::init_hardware_midi_router(gui_midi_tx.clone());
        }
        let live_params = Arc::new(LiveParamStore::default());
        let params = Arc::new(SamplerParams::new_linked(
            live_params.clone(),
            Some(ai_worker.clone()),
            Some(sample_rate_atomic.clone()),
        ));

        let last_daw = LastDawParams::from_params(&params);

        Self {
            params,
            live_params,
            voice_manager,
            fx_rack,
            ai_worker,
            active_sample,
            sample_update_rx,
            active_zones: [None, None, None],
            zone_update_rx,
            sample_rate,
            sample_rate_atomic,
            gui_midi_rx,
            gui_midi_tx,
            web_gui_tx,
            web_gui_rx,
            last_daw,
            last_custom_wt_rev: 0,
            smoothed_gain: 1.0,
            mono_scratch_buffer: vec![0.0; 4096],
        }
    }
}

impl AiSamplerPlugin {
    pub fn apply_web_gui_event(&mut self, evt: web_editor::WebGuiEvent) {
        use web_editor::{GuiParamId, WebGuiEvent};
        match evt {
            WebGuiEvent::SetParam { id, normalized } => {
                let clamped = normalized.clamp(0.0, 1.0);
                match id {
                    GuiParamId::Attack => {
                        self.live_params
                            .set_attack(self.params.attack.preview_plain(clamped));
                        self.last_daw.attack = self.params.attack.value();
                    }
                    GuiParamId::Decay => {
                        self.live_params
                            .set_decay(self.params.decay.preview_plain(clamped));
                        self.last_daw.decay = self.params.decay.value();
                    }
                    GuiParamId::Sustain => {
                        self.live_params
                            .set_sustain(self.params.sustain.preview_plain(clamped));
                        self.last_daw.sustain = self.params.sustain.value();
                    }
                    GuiParamId::Release => {
                        self.live_params
                            .set_release(self.params.release.preview_plain(clamped));
                        self.last_daw.release = self.params.release.value();
                    }
                    GuiParamId::Cutoff => {
                        self.live_params
                            .set_cutoff(self.params.cutoff.preview_plain(clamped));
                        self.last_daw.cutoff = self.params.cutoff.value();
                    }
                    GuiParamId::Resonance => {
                        self.live_params
                            .set_resonance(self.params.resonance.preview_plain(clamped));
                        self.last_daw.resonance = self.params.resonance.value();
                    }
                    GuiParamId::FilterMode => {
                        let mode = (normalized.round() as i32).clamp(0, 8);
                        self.live_params.set_filter_mode(mode);
                        self.last_daw.filter_mode = self.params.filter_mode.value();
                    }
                    GuiParamId::Gain => {
                        self.live_params
                            .set_gain(self.params.gain.preview_plain(clamped));
                        self.last_daw.gain = self.params.gain.value();
                    }
                    GuiParamId::SpaceMix => {
                        self.live_params
                            .set_space_mix(self.params.space_mix.preview_plain(clamped));
                        self.last_daw.space_mix = self.params.space_mix.value();
                    }
                    GuiParamId::RootNote => {
                        let note = ((clamped * 127.0).round() as i32).clamp(0, 127);
                        self.live_params.set_root_note(note);
                        self.last_daw.root_note = self.params.root_note.value();
                    }
                    GuiParamId::LfoRate => {
                        self.live_params
                            .set_lfo_rate(self.params.lfo_rate.preview_plain(clamped));
                        self.last_daw.lfo_rate = self.params.lfo_rate.value();
                    }
                    GuiParamId::LfoDepth => {
                        self.live_params
                            .set_lfo_depth(self.params.lfo_depth.preview_plain(clamped));
                        self.last_daw.lfo_depth = self.params.lfo_depth.value();
                    }
                    GuiParamId::LfoTarget => {
                        let target = (normalized.round() as i32).clamp(0, 3);
                        self.live_params.set_lfo_target(target);
                        self.last_daw.lfo_target = self.params.lfo_target.value();
                    }
                    GuiParamId::AllRefresh => {
                        self.last_daw = LastDawParams::from_params(&self.params);
                    }
                }
                self.live_params.bump_gui_param_sync();
                self.live_params.bump_revision();
            }
        }
    }
}

impl Plugin for AiSamplerPlugin {
    const NAME: &'static str = "TEPUI Neural Chromatic Sampler";
    const VENDOR: &'static str = "Gran Sabana Sonic Lab";
    const URL: &'static str = "https://github.com/davidcastilloc/tepui-vst";
    const EMAIL: &'static str = "contact@gransabana.audio";

    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    // ConfiguraciÃ³n de buses de audio: instrumento virtual con salida estÃ©reo y entrada MIDI,
    // y compatibilidad con hardware multicanal (7.1 THX Spatial / 8 canales, 5.1 / 6 canales, y mono).
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: None,
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: None,
            main_output_channels: NonZeroU32::new(8),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: None,
            main_output_channels: NonZeroU32::new(6),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: None,
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];

    const MIDI_INPUT: MidiConfig = MidiConfig::MidiCCs;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        Some(Box::new(web_editor::WebViewEditor::new(
            1180,
            780,
            self.gui_midi_tx.clone(),
            self.params.clone(),
            self.live_params.clone(),
            self.ai_worker.clone(),
            self.sample_rate_atomic.clone(),
            Some(self.web_gui_tx.clone()),
        )))
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate;
        self.sample_rate_atomic
            .store(buffer_config.sample_rate.to_bits(), Ordering::Relaxed);
        self.voice_manager.set_sample_rate(self.sample_rate);
        self.fx_rack.set_sample_rate(self.sample_rate);
        self.mono_scratch_buffer
            .resize(buffer_config.max_buffer_size.max(4096) as usize, 0.0);
        true
    }

    fn reset(&mut self) {
        self.voice_manager.kill_all();
        self.fx_rack.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let num_samples = buffer.samples();
        if num_samples == 0 {
            return ProcessStatus::Normal;
        }

        // 1. Actualizar el sample activo y las Keyzones solo si el worker produjo cambios en segundo plano (lock-free y zero-allocation/zero-deallocation)
        while let Ok(new_sample) = self.sample_update_rx.try_recv() {
            self.live_params.set_root_note(new_sample.root_note as i32);
            self.voice_manager
                .set_root_note_override(Some(new_sample.root_note), &new_sample);
            self.live_params.bump_revision();
            let old_sample = std::mem::replace(&mut self.active_sample, new_sample);
            self.ai_worker.defer_drop(old_sample);
        }
        while let Ok((z_idx, z_buf)) = self.zone_update_rx.try_recv() {
            if z_idx < 3 {
                if let Some(old_zone) = std::mem::replace(&mut self.active_zones[z_idx], z_buf) {
                    self.ai_worker.defer_drop(old_zone);
                }
                let mut mask = 0u8;
                for i in 0..3 {
                    if self.active_zones[i].is_some() {
                        mask |= 1 << i;
                    }
                }
                self.live_params.set_active_keyzones_mask(mask);
                self.live_params.bump_revision();
            }
        }

        // 2. Sincronizar parÃ¡metros host/DAW -> LiveParamStore en caso de automatizaciÃ³n en el DAW
        let mut daw_changed = false;

        let p_attack = self.params.attack.value();
        if (p_attack - self.last_daw.attack).abs() > 1e-5 {
            self.live_params.set_attack(p_attack);
            self.last_daw.attack = p_attack;
            daw_changed = true;
        }

        let p_decay = self.params.decay.value();
        if (p_decay - self.last_daw.decay).abs() > 1e-5 {
            self.live_params.set_decay(p_decay);
            self.last_daw.decay = p_decay;
            daw_changed = true;
        }

        let p_sustain = self.params.sustain.value();
        if (p_sustain - self.last_daw.sustain).abs() > 1e-5 {
            self.live_params.set_sustain(p_sustain);
            self.last_daw.sustain = p_sustain;
            daw_changed = true;
        }

        let p_release = self.params.release.value();
        if (p_release - self.last_daw.release).abs() > 1e-5 {
            self.live_params.set_release(p_release);
            self.last_daw.release = p_release;
            daw_changed = true;
        }

        let p_cutoff = self.params.cutoff.value();
        if (p_cutoff - self.last_daw.cutoff).abs() > 0.05 {
            self.live_params.set_cutoff(p_cutoff);
            self.last_daw.cutoff = p_cutoff;
            daw_changed = true;
        }

        let p_resonance = self.params.resonance.value();
        if (p_resonance - self.last_daw.resonance).abs() > 1e-4 {
            self.live_params.set_resonance(p_resonance);
            self.last_daw.resonance = p_resonance;
            daw_changed = true;
        }

        let p_gain = self.params.gain.value();
        if (p_gain - self.last_daw.gain).abs() > 1e-5 {
            self.live_params.set_gain(p_gain);
            self.last_daw.gain = p_gain;
            daw_changed = true;
        }

        let p_space_mix = self.params.space_mix.value();
        if (p_space_mix - self.last_daw.space_mix).abs() > 1e-4 {
            self.live_params.set_space_mix(p_space_mix);
            self.last_daw.space_mix = p_space_mix;
            daw_changed = true;
        }

        let p_mode = self.params.filter_mode.value();
        if p_mode != self.last_daw.filter_mode {
            self.live_params.set_filter_mode(p_mode);
            self.last_daw.filter_mode = p_mode;
            daw_changed = true;
        }

        let p_root = self.params.root_note.value();
        if p_root != self.last_daw.root_note {
            self.live_params.set_root_note(p_root);
            self.last_daw.root_note = p_root;
            daw_changed = true;
        }

        let p_lfo_rate = self.params.lfo_rate.value();
        if (p_lfo_rate - self.last_daw.lfo_rate).abs() > 1e-4 {
            self.live_params.set_lfo_rate(p_lfo_rate);
            self.last_daw.lfo_rate = p_lfo_rate;
            daw_changed = true;
        }

        let p_lfo_depth = self.params.lfo_depth.value();
        if (p_lfo_depth - self.last_daw.lfo_depth).abs() > 1e-4 {
            self.live_params.set_lfo_depth(p_lfo_depth);
            self.last_daw.lfo_depth = p_lfo_depth;
            daw_changed = true;
        }

        let p_lfo_target = self.params.lfo_target.value();
        if p_lfo_target != self.last_daw.lfo_target {
            self.live_params.set_lfo_target(p_lfo_target);
            self.last_daw.lfo_target = p_lfo_target;
            daw_changed = true;
        }

        let p_lfo_shape = self.params.lfo_shape.value();
        if p_lfo_shape != self.last_daw.lfo_shape {
            self.live_params.set_lfo_shape(p_lfo_shape);
            self.last_daw.lfo_shape = p_lfo_shape;
            daw_changed = true;
        }

        let p_arp_mode = self.params.arp_mode.value();
        if p_arp_mode != self.last_daw.arp_mode {
            self.live_params.set_arp_mode(p_arp_mode);
            self.last_daw.arp_mode = p_arp_mode;
            daw_changed = true;
        }

        let p_wt_pos = self.params.wt_pos.value();
        if (p_wt_pos - self.last_daw.wt_pos).abs() > 1e-4 {
            self.live_params.set_wt_pos(p_wt_pos);
            self.last_daw.wt_pos = p_wt_pos;
            daw_changed = true;
        }

        let p_wt_warp = self.params.wt_warp.value();
        if (p_wt_warp - self.last_daw.wt_warp).abs() > 1e-4 {
            self.live_params.set_wt_warp(p_wt_warp);
            self.last_daw.wt_warp = p_wt_warp;
            daw_changed = true;
        }

        let p_unison_voices = self.params.unison_voices.value();
        if p_unison_voices != self.last_daw.unison_voices {
            self.live_params
                .set_unison_voices(p_unison_voices.clamp(1, 8) as u8);
            self.last_daw.unison_voices = p_unison_voices;
            daw_changed = true;
        }

        let p_unison_detune = self.params.unison_detune.value();
        if (p_unison_detune - self.last_daw.unison_detune).abs() > 1e-4 {
            self.live_params.set_unison_detune(p_unison_detune);
            self.last_daw.unison_detune = p_unison_detune;
            daw_changed = true;
        }

        let p_sub_level = self.params.sub_level.value();
        if (p_sub_level - self.last_daw.sub_level).abs() > 1e-4 {
            self.live_params.set_sub_level(p_sub_level);
            self.last_daw.sub_level = p_sub_level;
            daw_changed = true;
        }

        let p_fm_from_b = self.params.fm_from_b.value();
        if (p_fm_from_b - self.last_daw.fm_from_b).abs() > 1e-4 {
            self.live_params.set_fm_from_b(p_fm_from_b);
            self.live_params.set_osc_b_level(if p_fm_from_b > 0.001 {
                (p_fm_from_b * 0.75).clamp(0.15, 0.85)
            } else {
                0.0
            });
            self.last_daw.fm_from_b = p_fm_from_b;
            daw_changed = true;
        }

        let p_drive = self.params.drive_amount.value();
        if (p_drive - self.last_daw.drive_amount).abs() > 1e-4 {
            self.live_params.set_drive_amount(p_drive);
            self.last_daw.drive_amount = p_drive;
            daw_changed = true;
        }

        let p_ott = self.params.ott_amount.value();
        if (p_ott - self.last_daw.ott_amount).abs() > 1e-4 {
            self.live_params.set_ott_amount(p_ott);
            self.last_daw.ott_amount = p_ott;
            daw_changed = true;
        }

        let p_keytrack = self.params.filter_keytrack.value();
        if (p_keytrack - self.last_daw.filter_keytrack).abs() > 1e-4 {
            self.live_params.set_filter_keytrack(p_keytrack);
            self.last_daw.filter_keytrack = p_keytrack;
            daw_changed = true;
        }

        let p_m1 = self.params.macro1.value();
        if (p_m1 - self.last_daw.macro1).abs() > 1e-4 {
            self.live_params.set_macro(0, p_m1);
            self.last_daw.macro1 = p_m1;
            daw_changed = true;
        }

        let p_m2 = self.params.macro2.value();
        if (p_m2 - self.last_daw.macro2).abs() > 1e-4 {
            self.live_params.set_macro(1, p_m2);
            self.last_daw.macro2 = p_m2;
            daw_changed = true;
        }

        let p_m3 = self.params.macro3.value();
        if (p_m3 - self.last_daw.macro3).abs() > 1e-4 {
            self.live_params.set_macro(2, p_m3);
            self.last_daw.macro3 = p_m3;
            daw_changed = true;
        }

        let p_m4 = self.params.macro4.value();
        if (p_m4 - self.last_daw.macro4).abs() > 1e-4 {
            self.live_params.set_macro(3, p_m4);
            self.last_daw.macro4 = p_m4;
            daw_changed = true;
        }

        if daw_changed {
            self.live_params.bump_revision();
        }

        // 2b. Drenar y aplicar eventos de parÃ¡metros desde la GUI WebView2 (100% Copy, zero-alloc)
        while let Ok(evt) = self.web_gui_rx.try_recv() {
            self.apply_web_gui_event(evt);
        }

        let active_sample = &self.active_sample;
        let zone_refs = [
            self.active_zones[0].as_deref(),
            self.active_zones[1].as_deref(),
            self.active_zones[2].as_deref(),
        ];

        // 3. Proyector Canónico Único (Single-Point DSP Projector): sincronizar LiveParamStore -> VoiceManager + StereoFxRack
        let host_bpm_opt = context
            .transport()
            .tempo
            .map(|t| t as f32)
            .filter(|&b| b > 20.0);
        self.live_params.project_to_dsp(
            &mut self.voice_manager,
            &mut self.fx_rack,
            host_bpm_opt,
            active_sample,
            &mut self.last_custom_wt_rev,
        );

        // 4. Procesar notas tocadas interactivamente desde la GUI / Teclado QWERTY nativo
        //    (MÃ¡xima prioridad interactiva sin jitter: entran en el sample 0 del bloque)
        while let Ok(gui_event) = self.gui_midi_rx.try_recv() {
            match gui_event {
                GuiMidiEvent::NoteOn(note, velocity) => {
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "[TEPUI AUDIO THREAD] NoteOn note={}, vel={:.2}",
                        note, velocity
                    );
                    self.voice_manager.note_on_with_zones(
                        note,
                        velocity,
                        active_sample,
                        &zone_refs,
                    );
                }
                GuiMidiEvent::NoteOff(note) => {
                    self.voice_manager.note_off(note);
                }
                GuiMidiEvent::PitchBend(semitones) => {
                    self.voice_manager.set_pitch_bend(semitones);
                    self.live_params.set_pitch_bend_semitones(semitones);
                    self.live_params.bump_midi_cc_events();
                }
                GuiMidiEvent::ModWheel(val) => {
                    self.voice_manager.set_mod_wheel(val);
                    self.live_params.set_mod_wheel(val);
                    self.live_params.bump_midi_cc_events();
                    if self.live_params.apply_or_learn_midi_cc(1, val) {
                        self.live_params.project_to_dsp(
                            &mut self.voice_manager,
                            &mut self.fx_rack,
                            host_bpm_opt,
                            active_sample,
                            &mut self.last_custom_wt_rev,
                        );
                    }
                }
                GuiMidiEvent::SustainPedal(down) => {
                    self.voice_manager.set_sustain_pedal(down);
                    self.live_params.set_sustain_pedal(down);
                    self.live_params.bump_midi_cc_events();
                }
                GuiMidiEvent::Aftertouch(pressure) => {
                    self.voice_manager.set_aftertouch(pressure);
                    self.live_params.bump_midi_cc_events();
                }
                GuiMidiEvent::PolyAftertouch(note, pressure) => {
                    self.voice_manager.set_poly_aftertouch(note, pressure);
                    self.live_params.bump_midi_cc_events();
                }
                GuiMidiEvent::NotePitchBend(note, semitones) => {
                    self.voice_manager.set_note_pitch_bend(note, semitones);
                    self.live_params.bump_midi_cc_events();
                }
                GuiMidiEvent::MidiCC(cc, value) => {
                    self.live_params.bump_midi_cc_events();
                    if self.live_params.apply_or_learn_midi_cc(cc, value) {
                        self.live_params.project_to_dsp(
                            &mut self.voice_manager,
                            &mut self.fx_rack,
                            host_bpm_opt,
                            active_sample,
                            &mut self.last_custom_wt_rev,
                        );
                    }
                }
                GuiMidiEvent::BrightnessCC74(val) => {
                    self.voice_manager.set_brightness_cc74(val);
                    self.live_params.bump_midi_cc_events();
                    if self.live_params.apply_or_learn_midi_cc(74, val) {
                        self.live_params.project_to_dsp(
                            &mut self.voice_manager,
                            &mut self.fx_rack,
                            host_bpm_opt,
                            active_sample,
                            &mut self.last_custom_wt_rev,
                        );
                    }
                }
                GuiMidiEvent::SetVelocity(vel) => {
                    self.voice_manager.set_arp_live_velocity(vel);
                }
                GuiMidiEvent::AllNotesOff => {
                    #[cfg(debug_assertions)]
                    eprintln!("[TEPUI AUDIO THREAD] AllNotesOff / Panic");
                    self.voice_manager.kill_all();
                }
            }
        }

        // 5. Limpiar todos los canales de salida antes de renderizar
        let out_slices = buffer.as_slice();
        if out_slices.is_empty() {
            return ProcessStatus::Normal;
        }

        for ch in out_slices.iter_mut() {
            ch.fill(0.0);
        }

        let (out_left, out_right) = out_slices.split_at_mut(1);
        let left_channel = &mut out_left[0];
        let (right_channel, is_stereo): (&mut [f32], bool) = if !out_right.is_empty() {
            (&mut out_right[0], true)
        } else {
            let scratch = &mut self.mono_scratch_buffer[..num_samples];
            scratch.fill(0.0);
            (scratch, false)
        };

        // 6. Renderizar con precisión de muestra exacta (Sample-Accurate MIDI)
        let mut next_event = context.next_event();
        let mut block_start = 0;

        while block_start < num_samples {
            // Despachar todos los eventos que correspondan al inicio del sub-bloque actual
            while let Some(event) = next_event {
                if (event.timing() as usize) <= block_start {
                    match event {
                        NoteEvent::NoteOn { note, velocity, .. } => {
                            self.voice_manager.note_on_with_zones(
                                note,
                                velocity,
                                active_sample,
                                &zone_refs,
                            );
                        }
                        NoteEvent::NoteOff { note, .. } => {
                            self.voice_manager.note_off(note);
                        }
                        NoteEvent::MidiCC { cc, value, .. } => {
                            self.live_params.bump_midi_cc_events();
                            if self.live_params.apply_or_learn_midi_cc(cc, value) {
                                self.live_params.project_to_dsp(
                                    &mut self.voice_manager,
                                    &mut self.fx_rack,
                                    host_bpm_opt,
                                    active_sample,
                                    &mut self.last_custom_wt_rev,
                                );
                            }
                            match cc {
                                1 => self.voice_manager.set_mod_wheel(value),
                                11 | 74 => self.voice_manager.set_brightness_cc74(value),
                                64 => self.voice_manager.set_sustain_pedal(value >= 0.5),
                                120 | 123 => self.voice_manager.kill_all(),
                                _ => {}
                            }
                        }
                        NoteEvent::PolyPressure { note, pressure, .. } => {
                            self.live_params.bump_midi_cc_events();
                            self.voice_manager.set_poly_aftertouch(note, pressure);
                        }
                        NoteEvent::PolyTuning { note, tuning, .. } => {
                            self.live_params.bump_midi_cc_events();
                            self.voice_manager
                                .set_note_pitch_bend(note, tuning.clamp(-48.0, 48.0));
                        }
                        NoteEvent::MidiChannelPressure { pressure, .. } => {
                            self.live_params.bump_midi_cc_events();
                            self.voice_manager.set_aftertouch(pressure);
                        }
                        NoteEvent::MidiPitchBend { value, .. } => {
                            self.voice_manager.set_pitch_bend(value * 2.0);
                        }
                        _ => {}
                    }
                    next_event = context.next_event();
                } else {
                    break;
                }
            }

            let block_end = match next_event {
                Some(ref event) => (event.timing() as usize).clamp(block_start, num_samples),
                None => num_samples,
            };

            if block_end > block_start {
                self.voice_manager.process_block_with_zones(
                    active_sample,
                    &zone_refs,
                    &mut left_channel[block_start..block_end],
                    &mut right_channel[block_start..block_end],
                );
                block_start = block_end;
            } else if next_event.is_some() {
                // Safeguard contra timestamps no monÃ³tonos o repetidos: forzar avance de 1 muestra
                let next_step = (block_start + 1).min(num_samples);
                self.voice_manager.process_block_with_zones(
                    active_sample,
                    &zone_refs,
                    &mut left_channel[block_start..next_step],
                    &mut right_channel[block_start..next_step],
                );
                block_start = next_step;
            } else {
                break;
            }
        }

        // Despachar eventos residuales al borde final del buffer (si timing == num_samples)
        while let Some(event) = next_event {
            match event {
                NoteEvent::NoteOn { note, velocity, .. } => {
                    self.voice_manager.note_on_with_zones(
                        note,
                        velocity,
                        active_sample,
                        &zone_refs,
                    );
                }
                NoteEvent::NoteOff { note, .. } => {
                    self.voice_manager.note_off(note);
                }
                NoteEvent::MidiCC { cc, value, .. } => {
                    let _ = self.live_params.apply_or_learn_midi_cc(cc, value);
                    match cc {
                        64 => self.voice_manager.set_sustain_pedal(value >= 0.5),
                        120 | 123 => self.voice_manager.kill_all(),
                        _ => {}
                    }
                }
                NoteEvent::MidiPitchBend { value, .. } => {
                    self.voice_manager.set_pitch_bend(value * 2.0);
                }
                _ => {}
            }
            next_event = context.next_event();
        }

        // 7a. Aplicar Master Stereo FX Rack (configurado por project_to_dsp)
        self.fx_rack.process_block(left_channel, right_channel);

        // 7b. Aplicar Master Gain suavizado por muestra (anti-zipper), saturaciÃ³n analÃ³gica suave anti-clipping, sanitizaciÃ³n NaN y medir pico L/R + mÃ©tricas ODD
        let target_gain = self.live_params.get_gain();
        let start_gain = self.smoothed_gain;
        let mut g_l = start_gain;
        let mut block_peak_l = 0.0_f32;
        let mut limiter_hits: u32 = 0;
        let mut nan_count: u32 = 0;

        for s in left_channel.iter_mut() {
            g_l += (target_gain - g_l) * 0.015;
            let raw = *s * g_l;
            let val = if raw.is_finite() {
                raw
            } else {
                nan_count += 1;
                0.0
            };
            let out = if val > 0.95 {
                limiter_hits += 1;
                0.95 + ((val - 0.95) / (1.0 + (val - 0.95).powi(2))).min(0.049)
            } else if val < -0.95 {
                limiter_hits += 1;
                -0.95 + ((val + 0.95) / (1.0 + (val + 0.95).powi(2))).max(-0.049)
            } else {
                val
            };
            *s = out;
            let abs = out.abs();
            if abs > block_peak_l {
                block_peak_l = abs;
            }
        }
        let mut block_peak_r = block_peak_l;
        if is_stereo {
            let mut g_r = start_gain;
            block_peak_r = 0.0_f32;
            for s in right_channel.iter_mut() {
                g_r += (target_gain - g_r) * 0.015;
                let raw = *s * g_r;
                let val = if raw.is_finite() {
                    raw
                } else {
                    nan_count += 1;
                    0.0
                };
                let out = if val > 0.95 {
                    limiter_hits += 1;
                    0.95 + ((val - 0.95) / (1.0 + (val - 0.95).powi(2))).min(0.049)
                } else if val < -0.95 {
                    limiter_hits += 1;
                    -0.95 + ((val + 0.95) / (1.0 + (val + 0.95).powi(2))).max(-0.049)
                } else {
                    val
                };
                *s = out;
                let abs = out.abs();
                if abs > block_peak_r {
                    block_peak_r = abs;
                }
            }
        }
        self.smoothed_gain = if (target_gain - g_l).abs() < 1e-5 {
            target_gain
        } else {
            g_l
        };

        // 7c. Capturar salida Master post-FX en la Grabadora Master en Vivo si estÃ¡ activa (0 allocs, try_lock)
        self.live_params
            .recorder
            .push_stereo_block_rt(left_channel, right_channel, is_stereo);

        // Publicar telemetrÃ­a atÃ³mica lock-free y contadores ODD
        let smooth_l = block_peak_l.max(self.live_params.get_peak_l() * 0.85);
        let smooth_r = block_peak_r.max(self.live_params.get_peak_r() * 0.85);
        self.live_params.set_peaks(smooth_l, smooth_r);
        self.live_params
            .set_active_voices(self.voice_manager.active_voice_count() as u32);
        self.live_params
            .set_lfo_mod_value(self.voice_manager.lfo_last_value());
        self.live_params.set_playhead_norm(
            self.voice_manager
                .active_playhead_norm(active_sample, &zone_refs),
        );
        self.live_params
            .set_arp_steps_triggered(self.voice_manager.arp_steps_triggered());
        self.live_params
            .set_mod_matrix_dsp_frames(self.voice_manager.mod_matrix_dsp_frames());
        self.live_params.set_vital_serum_dsp_frames(
            self.voice_manager
                .vital_serum_dsp_frames()
                .wrapping_add(self.fx_rack.vital_fx_frames()),
        );
        self.live_params
            .set_glide_transitions(self.voice_manager.glide_transitions_count());
        self.live_params
            .set_mpe_poly_events(self.voice_manager.mpe_poly_events_count());
        self.live_params
            .set_env2_dsp_frames(self.voice_manager.env2_dsp_frames());
        self.live_params
            .set_arp_ratchet_hits(self.voice_manager.arp_ratchet_hits_count());
        self.live_params
            .set_granular_dsp_frames(self.voice_manager.granular_dsp_frames());
        self.live_params
            .set_advanced_filter_frames(self.voice_manager.advanced_filter_frames());
        self.live_params
            .set_last_velocity(self.voice_manager.last_velocity());
        self.live_params
            .set_velocity_events(self.voice_manager.velocity_events_count());
        self.live_params
            .set_pitch_bend_semitones(self.voice_manager.pitch_bend_semitones());
        self.live_params
            .set_mod_wheel(self.voice_manager.mod_wheel());
        self.live_params
            .set_sustain_pedal(self.voice_manager.sustain_pedal());
        self.live_params.add_limiter_activations(limiter_hits);
        self.live_params.add_nan_sanitized(nan_count);
        self.live_params.bump_dsp_blocks();

        // 8. Si el hardware opera en modo multicanal (7.1 THX / 5.1), mapear acÃºsticamente el par estÃ©reo a todos los altavoces
        let _total_channels = 1 + out_right.len();
        if out_right.len() >= 3 && is_stereo {
            let fl = &left_channel[..];
            let (right_front, surround) = out_right.split_at_mut(1);
            let fr = &right_front[0][..];
            for (idx, ch) in surround.iter_mut().enumerate() {
                match idx {
                    0 => {
                        // Canal 2: Altavoz Central (Downmix mono 3dB)
                        for i in 0..ch.len() {
                            ch[i] = (fl[i] + fr[i]) * 0.707;
                        }
                    }
                    1 => {
                        // Canal 3: LFE / Subwoofer (mono)
                        for i in 0..ch.len() {
                            ch[i] = (fl[i] + fr[i]) * 0.5;
                        }
                    }
                    even if even % 2 == 0 => {
                        // Canales 4, 6: Surround / Rear Left
                        for i in 0..ch.len() {
                            ch[i] = fl[i] * 0.85;
                        }
                    }
                    _ => {
                        // Canales 5, 7: Surround / Rear Right
                        for i in 0..ch.len() {
                            ch[i] = fr[i] * 0.85;
                        }
                    }
                }
            }
        }

        // DiagnÃ³stico en vivo de picos de audio renderizados
        #[cfg(debug_assertions)]
        {
            let max_val = left_channel.iter().fold(0.0f32, |acc, &s| acc.max(s.abs()));
            if max_val > 0.005 {
                static PEAK_LOG_TICK: std::sync::atomic::AtomicU32 =
                    std::sync::atomic::AtomicU32::new(0);
                let tick = PEAK_LOG_TICK.fetch_add(1, Ordering::Relaxed);
                if tick.is_multiple_of(20) {
                    eprintln!(
                        "[TEPUI DSP PEAK] Audio renderizado hacia hardware: peak={:.3}, canales={}, voces={}",
                        max_val,
                        _total_channels,
                        self.voice_manager.active_voice_count()
                    );
                }
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for AiSamplerPlugin {
    const CLAP_ID: &'static str = "ve.gransabana.tepui-chromatic-sampler";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("TEPUI // Gran Sabana Sonic Lab â€” AI Chromatic Polyphonic Sampler & Synthesizer");
    const CLAP_MANUAL_URL: Option<&'static str> =
        Some("https://github.com/davidcastilloc/tepui-vst");
    const CLAP_SUPPORT_URL: Option<&'static str> =
        Some("https://github.com/davidcastilloc/tepui-vst");
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Synthesizer,
        ClapFeature::Sampler,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for AiSamplerPlugin {
    const VST3_CLASS_ID: [u8; 16] = *b"AiChromaticSmplr";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Instrument,
        Vst3SubCategory::Synth,
        Vst3SubCategory::Sampler,
    ];
}

nih_export_clap!(AiSamplerPlugin);
nih_export_vst3!(AiSamplerPlugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_instantiation_and_defaults() {
        let plugin = AiSamplerPlugin::default();
        let _params = plugin.params();
        assert!(!plugin.ai_worker.get_sample().is_empty());
        assert_eq!(plugin.params.root_note.value(), 60);
        assert!((plugin.params.attack.value() - 0.01).abs() < 1e-4);
        assert_eq!(plugin.voice_manager.active_voice_count(), 0);
    }

    #[test]
    fn test_gui_midi_event_flow() {
        let mut plugin = AiSamplerPlugin::default();

        // 1. Enviar NoteOn desde GUI / IPC nativo
        plugin
            .gui_midi_tx
            .send(GuiMidiEvent::NoteOn(60, 0.9))
            .expect("send failed");

        // Simular consumo de eventos en el hilo de audio
        let active_sample = plugin.ai_worker.get_sample();
        while let Ok(gui_event) = plugin.gui_midi_rx.try_recv() {
            match gui_event {
                GuiMidiEvent::NoteOn(note, velocity) => {
                    plugin.voice_manager.note_on(note, velocity, &active_sample);
                }
                GuiMidiEvent::NoteOff(note) => {
                    plugin.voice_manager.note_off(note);
                }
                GuiMidiEvent::PitchBend(semitones) => {
                    plugin.voice_manager.set_pitch_bend(semitones);
                }
                GuiMidiEvent::ModWheel(norm) => {
                    plugin.voice_manager.set_mod_wheel(norm);
                }
                GuiMidiEvent::SustainPedal(active) => {
                    plugin.voice_manager.set_sustain_pedal(active);
                }
                GuiMidiEvent::Aftertouch(pressure) => {
                    plugin.voice_manager.set_aftertouch(pressure);
                }
                GuiMidiEvent::BrightnessCC74(slide) => {
                    plugin.voice_manager.set_brightness_cc74(slide);
                }
                GuiMidiEvent::SetVelocity(vel) => {
                    plugin.voice_manager.set_arp_live_velocity(vel);
                }
                GuiMidiEvent::PolyAftertouch(note, pressure) => {
                    plugin.voice_manager.set_poly_aftertouch(note, pressure);
                }
                GuiMidiEvent::NotePitchBend(note, semi) => {
                    plugin.voice_manager.set_note_pitch_bend(note, semi);
                }
                GuiMidiEvent::MidiCC(cc, norm) => {
                    plugin.live_params.apply_or_learn_midi_cc(cc, norm);
                }
                GuiMidiEvent::AllNotesOff => {
                    plugin.voice_manager.kill_all();
                }
            }
        }

        assert_eq!(plugin.voice_manager.active_voice_count(), 1);

        // 2. Enviar NoteOff
        plugin
            .gui_midi_tx
            .send(GuiMidiEvent::NoteOff(60))
            .expect("send failed");
        while let Ok(gui_event) = plugin.gui_midi_rx.try_recv() {
            match gui_event {
                GuiMidiEvent::NoteOn(note, velocity) => {
                    plugin.voice_manager.note_on(note, velocity, &active_sample);
                }
                GuiMidiEvent::NoteOff(note) => {
                    plugin.voice_manager.note_off(note);
                }
                GuiMidiEvent::PitchBend(semitones) => {
                    plugin.voice_manager.set_pitch_bend(semitones);
                }
                GuiMidiEvent::ModWheel(norm) => {
                    plugin.voice_manager.set_mod_wheel(norm);
                }
                GuiMidiEvent::SustainPedal(active) => {
                    plugin.voice_manager.set_sustain_pedal(active);
                }
                GuiMidiEvent::Aftertouch(pressure) => {
                    plugin.voice_manager.set_aftertouch(pressure);
                }
                GuiMidiEvent::BrightnessCC74(slide) => {
                    plugin.voice_manager.set_brightness_cc74(slide);
                }
                GuiMidiEvent::SetVelocity(vel) => {
                    plugin.voice_manager.set_arp_live_velocity(vel);
                }
                GuiMidiEvent::PolyAftertouch(note, pressure) => {
                    plugin.voice_manager.set_poly_aftertouch(note, pressure);
                }
                GuiMidiEvent::NotePitchBend(note, semi) => {
                    plugin.voice_manager.set_note_pitch_bend(note, semi);
                }
                GuiMidiEvent::MidiCC(cc, norm) => {
                    plugin.live_params.apply_or_learn_midi_cc(cc, norm);
                }
                GuiMidiEvent::AllNotesOff => {
                    plugin.voice_manager.kill_all();
                }
            }
        }

        // Al entrar en release, la voz sigue activa temporalmente hasta agotar la envolvente
        assert_eq!(plugin.voice_manager.active_voice_count(), 1);

        // Matar todas las voces (fade out de 5ms para prevenir clicks)
        plugin.voice_manager.kill_all();
        let mut left = vec![0.0; 512];
        let mut right = vec![0.0; 512];
        plugin
            .voice_manager
            .process_block(&active_sample, &mut left, &mut right);
        assert_eq!(plugin.voice_manager.active_voice_count(), 0);
    }

    #[test]
    fn test_gui_midi_panic_event() {
        let mut plugin = AiSamplerPlugin::default();
        let active_sample = plugin.ai_worker.get_sample();

        // Tocar varias notas simultáneas
        plugin
            .gui_midi_tx
            .send(GuiMidiEvent::NoteOn(60, 0.8))
            .unwrap();
        plugin
            .gui_midi_tx
            .send(GuiMidiEvent::NoteOn(64, 0.8))
            .unwrap();
        plugin
            .gui_midi_tx
            .send(GuiMidiEvent::NoteOn(67, 0.8))
            .unwrap();

        while let Ok(gui_event) = plugin.gui_midi_rx.try_recv() {
            match gui_event {
                GuiMidiEvent::NoteOn(note, velocity) => {
                    plugin.voice_manager.note_on(note, velocity, &active_sample)
                }
                GuiMidiEvent::NoteOff(note) => plugin.voice_manager.note_off(note),
                GuiMidiEvent::PitchBend(s) => plugin.voice_manager.set_pitch_bend(s),
                GuiMidiEvent::ModWheel(m) => plugin.voice_manager.set_mod_wheel(m),
                GuiMidiEvent::SustainPedal(sus) => plugin.voice_manager.set_sustain_pedal(sus),
                GuiMidiEvent::Aftertouch(p) => plugin.voice_manager.set_aftertouch(p),
                GuiMidiEvent::BrightnessCC74(b) => plugin.voice_manager.set_brightness_cc74(b),
                GuiMidiEvent::SetVelocity(v) => plugin.voice_manager.set_arp_live_velocity(v),
                GuiMidiEvent::PolyAftertouch(note, p) => {
                    plugin.voice_manager.set_poly_aftertouch(note, p)
                }
                GuiMidiEvent::NotePitchBend(note, s) => {
                    plugin.voice_manager.set_note_pitch_bend(note, s)
                }
                GuiMidiEvent::MidiCC(cc, n) => {
                    plugin.live_params.apply_or_learn_midi_cc(cc, n);
                }
                GuiMidiEvent::AllNotesOff => plugin.voice_manager.kill_all(),
            }
        }
        assert_eq!(plugin.voice_manager.active_voice_count(), 3);

        // Enviar evento de Pánico
        plugin.gui_midi_tx.send(GuiMidiEvent::AllNotesOff).unwrap();
        while let Ok(gui_event) = plugin.gui_midi_rx.try_recv() {
            match gui_event {
                GuiMidiEvent::NoteOn(note, velocity) => {
                    plugin.voice_manager.note_on(note, velocity, &active_sample)
                }
                GuiMidiEvent::NoteOff(note) => plugin.voice_manager.note_off(note),
                GuiMidiEvent::PitchBend(s) => plugin.voice_manager.set_pitch_bend(s),
                GuiMidiEvent::ModWheel(m) => plugin.voice_manager.set_mod_wheel(m),
                GuiMidiEvent::SustainPedal(sus) => plugin.voice_manager.set_sustain_pedal(sus),
                GuiMidiEvent::Aftertouch(p) => plugin.voice_manager.set_aftertouch(p),
                GuiMidiEvent::BrightnessCC74(b) => plugin.voice_manager.set_brightness_cc74(b),
                GuiMidiEvent::SetVelocity(v) => plugin.voice_manager.set_arp_live_velocity(v),
                GuiMidiEvent::PolyAftertouch(note, p) => {
                    plugin.voice_manager.set_poly_aftertouch(note, p)
                }
                GuiMidiEvent::NotePitchBend(note, s) => {
                    plugin.voice_manager.set_note_pitch_bend(note, s)
                }
                GuiMidiEvent::MidiCC(cc, n) => {
                    plugin.live_params.apply_or_learn_midi_cc(cc, n);
                }
                GuiMidiEvent::AllNotesOff => plugin.voice_manager.kill_all(),
            }
        }
        // Todas las voces silenciadas
        let mut left = vec![0.0; 256];
        let mut right = vec![0.0; 256];
        plugin
            .voice_manager
            .process_block(&active_sample, &mut left, &mut right);
        assert_eq!(plugin.voice_manager.active_voice_count(), 0);
    }

    #[test]
    fn test_audio_output_signal_level() {
        let mut plugin = AiSamplerPlugin::default();
        let sample_rate = 44100.0;
        plugin.voice_manager.set_sample_rate(sample_rate);
        let active_sample = plugin.ai_worker.get_sample();
        assert!(!active_sample.is_empty(), "Active sample buffer is empty!");

        // Trigger note 60 (C4)
        plugin.voice_manager.note_on(60, 1.0, &active_sample);
        assert_eq!(plugin.voice_manager.active_voice_count(), 1);

        let mut left = vec![0.0; 1024];
        let mut right = vec![0.0; 1024];
        plugin
            .voice_manager
            .process_block(&active_sample, &mut left, &mut right);

        let max_left = left.iter().fold(0.0_f32, |acc, &x| acc.max(x.abs()));
        let max_right = right.iter().fold(0.0_f32, |acc, &x| acc.max(x.abs()));

        println!(
            "DIAGNOSTIC: max_left = {}, max_right = {}",
            max_left, max_right
        );
        assert!(
            max_left > 0.01,
            "Audio signal too weak or silent! max_left: {}",
            max_left
        );
        assert!(
            max_right > 0.01,
            "Audio signal too weak or silent! max_right: {}",
            max_right
        );
    }

    #[test]
    fn test_piano_continuous_playback_and_looping() {
        let sample_rate = 44100.0;
        let mut plugin = AiSamplerPlugin::default();
        plugin.voice_manager.set_sample_rate(sample_rate);
        let buffer = plugin.ai_worker.get_sample();

        plugin.voice_manager.note_on(60, 0.9, &buffer);

        let block_size = 512;
        let mut out_l = vec![0.0; block_size];
        let mut out_r = vec![0.0; block_size];

        let mut max_jump: f32 = 0.0;
        let mut prev_l: f32 = 0.0;
        let mut max_peak: f32 = 0.0;

        // Reproducir durante 4 segundos (supera el umbral de loop de 2.8s)
        let total_blocks = (4.0 * sample_rate / block_size as f32) as usize;
        for _ in 0..total_blocks {
            out_l.fill(0.0);
            out_r.fill(0.0);
            plugin
                .voice_manager
                .process_block(&buffer, &mut out_l, &mut out_r);

            for &s in &out_l {
                let diff = (s - prev_l).abs();
                if diff > max_jump {
                    max_jump = diff;
                }
                prev_l = s;
                let abs = s.abs();
                if abs > max_peak {
                    max_peak = abs;
                }
            }
        }

        println!(
            "DIAGNOSTIC PIANO: max_peak = {}, max_jump = {}",
            max_peak, max_jump
        );
    }

    #[test]
    fn test_cpal_devices_and_supported_configs() {
        if std::env::var("CI").is_ok() || std::env::var("GITHUB_ACTIONS").is_ok() {
            return;
        }
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal::default_host();
        println!("Default host: {:?}", host.id());
        if let Ok(devices) = host.output_devices() {
            for dev in devices {
                if let Ok(name) = dev.name() {
                    println!("\nDevice: {}", name);
                    if let Ok(configs) = dev.supported_output_configs() {
                        for c in configs {
                            println!(
                                "  channels: {}, sample_rate: {}..={}, buffer_size: {:?}, format: {:?}",
                                c.channels(),
                                c.min_sample_rate().0,
                                c.max_sample_rate().0,
                                c.buffer_size(),
                                c.sample_format()
                            );
                        }
                    }
                    if let Ok(default_config) = dev.default_output_config() {
                        println!(
                            "  Default config: channels: {}, sample_rate: {}, buffer_size: {:?}, format: {:?}",
                            default_config.channels(),
                            default_config.sample_rate().0,
                            default_config.buffer_size(),
                            default_config.sample_format()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn test_cpal_stream_creation() {
        if std::env::var("CI").is_ok() || std::env::var("GITHUB_ACTIONS").is_ok() {
            return;
        }
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let host = cpal::default_host();
        if let Ok(devices) = host.output_devices() {
            for dev in devices {
                if let Ok(name) = dev.name() {
                    println!("\nTesting stream creation for: {}", name);
                    if let Ok(config) = dev.default_output_config() {
                        println!(
                            "  Config: {} channels, {} Hz",
                            config.channels(),
                            config.sample_rate().0
                        );
                        let err_fn = |err| eprintln!("an error occurred on stream: {}", err);
                        let channels = config.channels() as usize;
                        let sample_rate = config.sample_rate().0 as f32;
                        let mut sample_clock = 0f32;

                        let stream_result = match config.sample_format() {
                            cpal::SampleFormat::F32 => dev.build_output_stream(
                                &config.into(),
                                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                                    for frame in data.chunks_mut(channels) {
                                        sample_clock = (sample_clock + 1.0) % sample_rate;
                                        let value =
                                            (sample_clock * 440.0 * 2.0 * std::f32::consts::PI
                                                / sample_rate)
                                                .sin()
                                                * 0.25;
                                        for sample in frame.iter_mut() {
                                            *sample = value;
                                        }
                                    }
                                },
                                err_fn,
                                None,
                            ),
                            _ => Err(cpal::BuildStreamError::DeviceNotAvailable),
                        };
                        match stream_result {
                            Ok(stream) => {
                                println!("  SUCCESS building stream for {}", name);
                                let play_res = stream.play();
                                println!("  Play result: {:?}", play_res);
                                std::thread::sleep(std::time::Duration::from_millis(15));
                            }
                            Err(e) => {
                                println!("  FAILED building stream for {}: {:?}", name, e);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_plugin_large_buffer_support() {
        let mut plugin = AiSamplerPlugin::default();
        assert_eq!(plugin.mono_scratch_buffer.len(), 4096);

        // Simular configuraciÃ³n de DAW con tamaÃ±o de bloque grande (8192 muestras)
        let large_block_size: usize = 8192;
        plugin
            .mono_scratch_buffer
            .resize(large_block_size.max(4096), 0.0);
        assert_eq!(plugin.mono_scratch_buffer.len(), 8192);

        let active_sample = plugin.ai_worker.get_sample();
        plugin.voice_manager.note_on(60, 0.9, &active_sample);

        let mut left_channel = vec![0.0_f32; large_block_size];
        let right_scratch = &mut plugin.mono_scratch_buffer[..large_block_size];
        right_scratch.fill(0.0);

        plugin
            .voice_manager
            .process_block(&active_sample, &mut left_channel, right_scratch);

        // Verificar que se renderizÃ³ audio vÃ¡lido hasta el final del bloque de 8192 muestras sin truncamiento ni NaNs
        let tail_energy: f32 = left_channel[4096..8192].iter().map(|s| s.abs()).sum();
        assert!(
            tail_energy > 0.0,
            "Large buffer tail (samples 4096..8192) should contain rendered audio"
        );
        for &s in left_channel.iter().chain(right_scratch.iter()) {
            assert!(!s.is_nan() && !s.is_infinite());
        }
    }

    #[test]
    fn test_root_note_sync_and_live_telemetry() {
        let mut plugin = AiSamplerPlugin::default();
        plugin.voice_manager.set_sample_rate(44100.0);
        let sample = plugin.ai_worker.get_sample();

        // Override root_note a 48 (C3) en live_params y sincronizar con VoiceManager
        plugin.live_params.set_root_note(48);
        plugin
            .voice_manager
            .set_root_note_override(Some(plugin.live_params.get_root_note() as u8), &sample);
        assert_eq!(plugin.voice_manager.effective_root_note(&sample), 48);

        // Tocar nota 60 (C4 -> 1 octava arriba del nuevo root C3)
        plugin.voice_manager.note_on(60, 0.9, &sample);
        assert_eq!(plugin.voice_manager.active_voice_count(), 1);

        let mut left = vec![0.0_f32; 512];
        let mut right = vec![0.0_f32; 512];
        plugin
            .voice_manager
            .process_block(&sample, &mut left, &mut right);

        let peak_l = left.iter().fold(0.0_f32, |acc, &s| acc.max(s.abs()));
        let peak_r = right.iter().fold(0.0_f32, |acc, &s| acc.max(s.abs()));
        plugin.live_params.set_peaks(peak_l, peak_r);
        plugin
            .live_params
            .set_active_voices(plugin.voice_manager.active_voice_count() as u32);

        assert!(plugin.live_params.get_peak_l() > 0.01);
        assert!(plugin.live_params.get_peak_r() > 0.01);
        assert_eq!(plugin.live_params.get_active_voices(), 1);
    }

    #[test]
    fn test_daw_session_persistence_roundtrip_and_odd_counter() {
        use nih_plug::params::persist::PersistentField;

        let plugin = AiSamplerPlugin::default();
        assert_eq!(plugin.live_params.get_persistence_restores(), 0);

        // 1. Configurar regiÃ³n personalizada y cargar WAV custom
        plugin.live_params.set_trim_start_norm(0.12);
        plugin.live_params.set_trim_end_norm(0.88);
        plugin.live_params.set_loop_enabled_override(Some(true));
        plugin.live_params.set_loop_start_norm(0.25);
        plugin.live_params.set_loop_end_norm(0.75);
        plugin.live_params.set_root_note(52);

        let custom_sample = plugin.ai_worker.get_sample();
        let wav_bytes = synth_core::encode_wav_f32(&custom_sample);
        plugin
            .ai_worker
            .load_wav_bytes(&wav_bytes, Some("DAW Persisted Synth".to_string()))
            .expect("WAV load failed");
        plugin.ai_worker.assign_current_to_zone(0);
        plugin.ai_worker.assign_current_to_zone(1);

        // 2. Serializar sesiÃ³n como lo hace el DAW al guardar el proyecto
        let serialized_session = plugin
            .params
            .session_state
            .map(|session| serde_json::to_string(session).expect("serialize failed"));
        assert!(serialized_session.contains("DAW Persisted Synth"));
        assert!(serialized_session.contains("custom_wav_b64"));
        assert!(serialized_session.contains("zone_wavs_b64"));

        // 3. Crear una instancia fresca del plugin y restaurar el estado del proyecto DAW
        let restored_plugin = AiSamplerPlugin::default();
        let parsed: params::PersistedSamplerSession =
            serde_json::from_str(&serialized_session).expect("deserialize failed");
        restored_plugin.params.session_state.set(parsed);

        assert_eq!(restored_plugin.live_params.get_persistence_restores(), 1);
        assert_eq!(restored_plugin.live_params.get_root_note(), 52);
        assert!((restored_plugin.live_params.get_trim_start_norm() - 0.12).abs() < 1e-4);
        assert!((restored_plugin.live_params.get_trim_end_norm() - 0.88).abs() < 1e-4);
        assert_eq!(
            restored_plugin.live_params.get_loop_enabled_override(),
            Some(true)
        );
        assert!((restored_plugin.live_params.get_loop_start_norm() - 0.25).abs() < 1e-4);
        assert!((restored_plugin.live_params.get_loop_end_norm() - 0.75).abs() < 1e-4);
        assert_eq!(
            restored_plugin.ai_worker.get_sample().name,
            "DAW Persisted Synth"
        );
        assert_eq!(restored_plugin.ai_worker.active_zones_mask(), 0b011);
        assert_eq!(
            restored_plugin.live_params.get_active_keyzones_mask(),
            0b011
        );
        assert_eq!(
            restored_plugin.ai_worker.zone_names()[0].as_deref(),
            Some("DAW Persisted Synth")
        );
        assert_eq!(
            restored_plugin.ai_worker.zone_names()[1].as_deref(),
            Some("DAW Persisted Synth")
        );
    }

    #[test]
    fn test_stereo_fx_rack_and_region_override_in_plugin() {
        let mut plugin = AiSamplerPlugin::default();
        plugin.voice_manager.set_sample_rate(44100.0);
        plugin.fx_rack.set_sample_rate(44100.0);

        plugin.live_params.set_space_mix(0.65);
        plugin.live_params.set_trim_start_norm(0.05);
        plugin.live_params.set_trim_end_norm(0.90);
        plugin.live_params.set_loop_enabled_override(Some(true));
        plugin.live_params.set_loop_start_norm(0.20);
        plugin.live_params.set_loop_end_norm(0.70);

        plugin
            .voice_manager
            .set_region_override(plugin.live_params.get_region_override());

        let sample = plugin.ai_worker.get_sample();
        plugin.voice_manager.note_on(60, 0.9, &sample);

        let mut left = vec![0.0_f32; 512];
        let mut right = vec![0.0_f32; 512];
        plugin
            .voice_manager
            .process_block(&sample, &mut left, &mut right);

        plugin.fx_rack.set_mix(plugin.live_params.get_space_mix());
        plugin.fx_rack.process_block(&mut left, &mut right);

        let stereo_diff: f32 = left
            .iter()
            .zip(right.iter())
            .map(|(l, r)| (l - r).abs())
            .sum();
        assert!(
            stereo_diff > 1e-3,
            "StereoFxRack should create quadrature stereo width"
        );
    }

    #[test]
    fn test_tepui_lfo_and_multizone_plugin_integration() {
        let mut plugin = AiSamplerPlugin::default();
        plugin.voice_manager.set_sample_rate(44100.0);

        // Asignar el sample actual a la Keyzone Low (0) y verificar recepciÃ³n en el hilo de audio
        plugin.ai_worker.assign_current_to_zone(0);
        while let Ok((z_idx, z_buf)) = plugin.zone_update_rx.try_recv() {
            plugin.active_zones[z_idx] = z_buf;
        }
        assert!(plugin.active_zones[0].is_some());

        // Configurar LFO activo sobre Cutoff
        plugin.live_params.set_lfo_rate(6.0);
        plugin.live_params.set_lfo_depth(0.8);
        plugin.live_params.set_lfo_target(0);
        plugin.voice_manager.update_lfo_params(
            plugin.live_params.get_lfo_rate(),
            plugin.live_params.get_lfo_depth(),
            plugin.live_params.get_lfo_target(),
        );

        let zone_refs = [
            plugin.active_zones[0].as_deref(),
            plugin.active_zones[1].as_deref(),
            plugin.active_zones[2].as_deref(),
        ];

        plugin
            .voice_manager
            .note_on_with_zones(48, 0.9, &plugin.active_sample, &zone_refs);

        let mut left = vec![0.0_f32; 256];
        let mut right = vec![0.0_f32; 256];
        plugin.voice_manager.process_block_with_zones(
            &plugin.active_sample,
            &zone_refs,
            &mut left,
            &mut right,
        );
        plugin
            .live_params
            .set_lfo_mod_value(plugin.voice_manager.lfo_last_value());
        plugin.live_params.set_playhead_norm(
            plugin
                .voice_manager
                .active_playhead_norm(&plugin.active_sample, &zone_refs),
        );

        assert!(plugin.live_params.get_lfo_mod_value().abs() > 0.0);
        assert!(plugin.live_params.get_playhead_norm() > 0.0);
    }

    #[test]
    fn test_web_gui_event_zero_alloc_sync_and_odd_counter() {
        use web_editor::{GuiParamId, WebGuiEvent};

        let mut plugin = AiSamplerPlugin::default();
        assert_eq!(plugin.live_params.get_gui_param_sync(), 0);

        // Enviar eventos Copy por el canal web_gui_tx
        plugin
            .web_gui_tx
            .send(WebGuiEvent::SetParam {
                id: GuiParamId::Cutoff,
                normalized: 0.25,
            })
            .expect("send Cutoff failed");
        plugin
            .web_gui_tx
            .send(WebGuiEvent::SetParam {
                id: GuiParamId::LfoDepth,
                normalized: 0.65,
            })
            .expect("send LfoDepth failed");

        while let Ok(evt) = plugin.web_gui_rx.try_recv() {
            plugin.apply_web_gui_event(evt);
        }

        assert_eq!(plugin.live_params.get_gui_param_sync(), 2);
        assert!((plugin.live_params.get_lfo_depth() - 0.65).abs() < 1e-4);
        assert!(plugin.live_params.get_cutoff() < 18000.0);
    }

    #[test]
    fn test_midi_cc_expression_modwheel_cc74_aftertouch_and_odd() {
        let mut plugin = AiSamplerPlugin::default();
        plugin.voice_manager.set_sample_rate(44100.0);
        plugin
            .voice_manager
            .update_filter_params(1200.0, 1.2, synth_core::FilterMode::Lowpass);
        plugin
            .voice_manager
            .update_lfo_params(5.0, 0.0, synth_core::LfoTarget::Pitch);

        // 1. ModWheel CC#1 activa el vibrato LFO incluso con el knob LFO Depth en 0%
        plugin.voice_manager.set_mod_wheel(0.85);
        plugin.live_params.bump_midi_cc_events();

        // 2. Brightness CC#74 y Aftertouch abren el filtro SVF dinÃ¡micamente (> 1200 Hz)
        plugin.voice_manager.set_brightness_cc74(0.6);
        plugin.live_params.bump_midi_cc_events();
        plugin.voice_manager.set_aftertouch(0.4);
        plugin.live_params.bump_midi_cc_events();

        assert!(plugin.voice_manager.effective_cutoff_hz() > 2400.0);
        assert_eq!(plugin.live_params.get_midi_cc_events(), 3);

        let sample = plugin.ai_worker.get_sample();
        plugin.voice_manager.note_on(60, 0.9, &sample);
        let mut left = vec![0.0_f32; 256];
        let mut right = vec![0.0_f32; 256];
        plugin
            .voice_manager
            .process_block(&sample, &mut left, &mut right);

        assert!(plugin.voice_manager.lfo_last_value().abs() > 0.0);
    }

    #[test]
    fn test_live_master_recorder_capture_and_wav_export() {
        let mut plugin = AiSamplerPlugin::default();
        plugin.voice_manager.set_sample_rate(44100.0);

        // 1. Iniciar grabaciÃ³n Master en vivo
        plugin.live_params.recorder.start_recording(44100.0);
        assert!(plugin.live_params.recorder.is_recording());
        assert_eq!(plugin.live_params.recorder.recorded_frames(), 0);

        // 2. Renderizar audio y empujarlo a la grabadora Master RT
        let sample = plugin.ai_worker.get_sample();
        plugin.voice_manager.note_on(60, 0.95, &sample);
        let mut left = vec![0.0_f32; 512];
        let mut right = vec![0.0_f32; 512];
        plugin
            .voice_manager
            .process_block(&sample, &mut left, &mut right);
        plugin.fx_rack.process_block(&mut left, &mut right);
        plugin
            .live_params
            .recorder
            .push_stereo_block_rt(&left, &right, true);

        assert_eq!(plugin.live_params.recorder.recorded_frames(), 512);

        // 3. Detener y exportar WAV IEEE Float 32-bit
        plugin.live_params.recorder.stop_recording();
        assert!(!plugin.live_params.recorder.is_recording());
        let wav_bytes = plugin.live_params.recorder.export_wav_bytes();
        let decoded = synth_core::decode_wav(&wav_bytes).expect("decode_wav of live rec failed");
        assert_eq!(decoded.len(), 512);
        assert!((decoded.sample_rate - 44100.0).abs() < 1e-4);
    }

    #[test]
    fn test_vital_serum_live_params_ipc_and_dsp_telemetry() {
        let mut plugin = AiSamplerPlugin::default();
        plugin.voice_manager.set_sample_rate(44100.0);

        web_editor::set_param_from_client(
            &plugin.params,
            &plugin.live_params,
            &None,
            "wt_pos",
            0.65,
        );
        web_editor::set_param_from_client(
            &plugin.params,
            &plugin.live_params,
            &None,
            "wt_warp",
            0.45,
        );
        web_editor::set_param_from_client(
            &plugin.params,
            &plugin.live_params,
            &None,
            "unison_voices",
            7.0,
        );
        web_editor::set_param_from_client(
            &plugin.params,
            &plugin.live_params,
            &None,
            "unison_detune",
            0.55,
        );
        web_editor::set_param_from_client(
            &plugin.params,
            &plugin.live_params,
            &None,
            "sub_level",
            0.50,
        );
        web_editor::set_param_from_client(
            &plugin.params,
            &plugin.live_params,
            &None,
            "drive",
            0.40,
        );
        web_editor::set_param_from_client(&plugin.params, &plugin.live_params, &None, "ott", 0.60);
        web_editor::set_param_from_client(
            &plugin.params,
            &plugin.live_params,
            &None,
            "lfo_shape",
            5.0,
        );
        web_editor::set_param_from_client(
            &plugin.params,
            &plugin.live_params,
            &None,
            "macro1",
            0.30,
        );
        web_editor::set_param_from_client(
            &plugin.params,
            &plugin.live_params,
            &None,
            "macro4",
            0.50,
        );

        assert!((plugin.live_params.get_wt_pos() - 0.65).abs() < 1e-4);
        assert_eq!(plugin.live_params.get_unison_voices(), 7);
        assert_eq!(
            plugin.live_params.get_lfo_shape(),
            synth_core::LfoShape::SampleAndHold
        );

        plugin.voice_manager.update_vital_serum_params(
            plugin.live_params.get_wt_pos(),
            plugin.live_params.get_wt_warp(),
            plugin.live_params.get_unison_voices(),
            plugin.live_params.get_unison_detune(),
            plugin.live_params.get_sub_level(),
            plugin.live_params.get_lfo_shape(),
            plugin.live_params.get_macros(),
        );
        plugin
            .fx_rack
            .set_drive(plugin.live_params.get_drive_amount());
        plugin.fx_rack.set_ott(plugin.live_params.get_ott_amount());

        let sample = plugin.ai_worker.get_sample();
        plugin.voice_manager.note_on(60, 0.95, &sample);
        let mut left = vec![0.0_f32; 256];
        let mut right = vec![0.0_f32; 256];
        plugin
            .voice_manager
            .process_block(&sample, &mut left, &mut right);
        plugin.fx_rack.process_block(&mut left, &mut right);

        plugin.live_params.set_vital_serum_dsp_frames(
            plugin
                .voice_manager
                .vital_serum_dsp_frames()
                .wrapping_add(plugin.fx_rack.vital_fx_frames()),
        );
        assert!(plugin.live_params.get_vital_serum_dsp_frames() >= 512);

        // Verificar también el nuevo Custom Freehand Wavetable Draw (v1.12.0)
        plugin
            .live_params
            .set_custom_wavetable(&[0.0, 0.5, 1.0, -0.5, -1.0]);
        assert!(plugin.live_params.is_custom_wavetable_active());
        assert_eq!(plugin.live_params.get_custom_wavetable_draws(), 1);
        let cycle = plugin.live_params.get_custom_wavetable_cycle();
        plugin.voice_manager.set_custom_wavetable_cycle(&cycle);
        assert!(plugin.voice_manager.is_custom_wavetable_active());
        plugin.live_params.clear_custom_wavetable();
        assert!(!plugin.live_params.is_custom_wavetable_active());
    }

    #[test]
    fn test_rt_sample_swap_deferred_drop_prevents_audio_thread_dealloc() {
        let mut plugin = AiSamplerPlugin::default();

        // Simular que el worker IA generó un nuevo sample ("Deep 808 Sub Bass") y ya reemplazó su ArcSwap
        let next_buf = Arc::new(
            synth_core::SampleBuffer::new_mono(vec![0.25; 1024], 44100.0, 36)
                .with_name("Deep 808 Sub Bass"),
        );
        plugin.ai_worker.current_sample.store(next_buf.clone());

        // En este punto, `plugin.active_sample` tiene strong_count == 1 (única referencia viva en el hilo de audio)
        assert_eq!(Arc::strong_count(&plugin.active_sample), 1);

        // El reemplazo con `defer_drop` traslada la propiedad al hilo recolector (`rt-sample-reaper`)
        // sin ejecutar `dealloc` de `String`/`Vec<f32>` en el hilo de audio bajo `assert_no_alloc`.
        let old_sample = std::mem::replace(&mut plugin.active_sample, next_buf);
        plugin.ai_worker.defer_drop(old_sample);

        assert_eq!(plugin.active_sample.name, "Deep 808 Sub Bass");
        assert_eq!(plugin.active_sample.root_note, 36);
    }
}
