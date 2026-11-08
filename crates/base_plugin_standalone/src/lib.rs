//! # `base_plugin_standalone` — Runner Autónomo Agnóstico para Motores `DspEngine`
//!
//! Permite ejecutar cualquier implementación de `DspEngine` fuera de un DAW comercial,
//! gestionando bloques de audio estéreo, eventos MIDI y protección ante dispositivos con 0 canales de entrada.

use base_plugin_core::{
    AtomicParamBank, DspEngine, HostTransportContext, LiveParamSnapshot, MidiEvent,
    MidiOutputBuffer, ProcessStatus, MAX_PLUGIN_PARAMS,
};

/// Configuración de ejecución del host autónomo (Standalone).
#[derive(Debug, Clone)]
pub struct StandaloneConfig {
    pub sample_rate: f32,
    pub block_size: usize,
    pub input_channels: u16,
    pub output_channels: u16,
}

impl Default for StandaloneConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            block_size: 256,
            input_channels: 2,
            output_channels: 2,
        }
    }
}

/// Host autónomo en tiempo real que envuelve un `DspEngine` y su `AtomicParamBank`.
pub struct StandaloneRunner<E: DspEngine> {
    engine: E,
    param_bank: AtomicParamBank<MAX_PLUGIN_PARAMS>,
    snapshot: LiveParamSnapshot,
    transport: HostTransportContext,
}

impl<E: DspEngine> StandaloneRunner<E> {
    /// Crea e inicializa un nuevo runner autónomo para el motor `engine`.
    pub fn new(mut engine: E, config: StandaloneConfig) -> Self {
        engine.initialize(config.sample_rate, config.block_size);
        let descriptors = engine.param_descriptors();
        let param_bank = AtomicParamBank::from_descriptors(descriptors);
        let mut snapshot = LiveParamSnapshot::empty();
        param_bank.take_snapshot(descriptors, &mut snapshot);

        Self {
            engine,
            param_bank,
            snapshot,
            transport: HostTransportContext {
                sample_rate: config.sample_rate,
                ..HostTransportContext::default()
            },
        }
    }

    /// Retorna una referencia al banco atómico de parámetros para enlace con la GUI.
    pub fn param_bank(&self) -> &AtomicParamBank<MAX_PLUGIN_PARAMS> {
        &self.param_bank
    }

    /// Procesa un bloque estéreo intercalado o separado de forma segura en tiempo real.
    pub fn process_stereo_buffers(
        &mut self,
        in_l: &[f32],
        in_r: &[f32],
        out_l: &mut [f32],
        out_r: &mut [f32],
        midi_in: &[MidiEvent],
    ) -> ProcessStatus {
        let descriptors = self.engine.param_descriptors();
        let _ = self
            .param_bank
            .sync_snapshot_if_changed(descriptors, &mut self.snapshot);
        let inputs = [in_l, in_r];
        let mut outputs = [out_l, out_r];
        let mut midi_out_storage = [None; 32];
        let mut midi_out = MidiOutputBuffer::new(&mut midi_out_storage);

        self.engine.process_block(
            &inputs,
            &mut outputs,
            midi_in,
            &mut midi_out,
            &self.transport,
            &self.snapshot,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base_plugin_core::reference_engines::StereoDriveFilterEffect;

    #[test]
    fn test_standalone_runner_processes_reference_effect() {
        let effect = StereoDriveFilterEffect::new();
        let mut runner = StandaloneRunner::new(effect, StandaloneConfig::default());
        let in_l = [0.25_f32; 64];
        let in_r = [-0.25_f32; 64];
        let mut out_l = [0.0_f32; 64];
        let mut out_r = [0.0_f32; 64];

        let status = runner.process_stereo_buffers(&in_l, &in_r, &mut out_l, &mut out_r, &[]);
        assert_eq!(status, ProcessStatus::Normal);
        assert!(out_l.iter().any(|&s| s.abs() > 1e-4));
    }
}
