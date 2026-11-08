//! Utilidades matemáticas de audio en tiempo real (dB, pitch, interpolación y protección anti-denormal).

pub mod db;
pub mod denormal;
pub mod interp;
pub mod pitch;

pub use db::{db_to_gain, gain_to_db};
pub use denormal::{flush_denormal, sanitize_sample};
pub use interp::{hermite4, lerp};
pub use pitch::{detune_cents_ratio, hz_to_midi, midi_to_hz};
