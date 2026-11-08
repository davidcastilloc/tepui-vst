//! Efectos espaciales y de modulación genéricos en tiempo real (Delay, Chorus, Reverb).

pub mod chorus;
pub mod delay;
pub mod reverb;

pub use chorus::StereoChorus;
pub use delay::StereoDelayLine;
pub use reverb::AlgorithmicReverb;
