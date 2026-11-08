//! Generadores de modulación en tiempo real (Envolventes ADSR anti-clic y LFOs polimórficos).

pub mod adsr;
pub mod lfo;

pub use adsr::{AdsrEnvelope, AdsrStage};
pub use lfo::{LfoOscillator, LfoWaveform};
