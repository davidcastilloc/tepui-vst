//! Procesadores de dinámica en tiempo real (Detectores de envolvente, Compresor Soft-Knee y Limitador Brickwall).

pub mod compressor;
pub mod detector;
pub mod limiter;

pub use compressor::FeedForwardCompressor;
pub use detector::EnvelopeDetector;
pub use limiter::BrickwallLimiter;
