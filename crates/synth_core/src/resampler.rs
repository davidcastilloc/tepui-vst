//! Algoritmo de remuestreo (Resampler) con Interpolación Cúbica de Hermite.
//! Garantiza transposición continua sin discontinuidades ni artefactos de alta frecuencia.

use crate::buffer::SampleBuffer;

pub use dsp_core::math::interp::hermite_interpolate_4p;
pub use dsp_core::math::pitch::calculate_playback_ratio;

/// Estrategia de remuestreo sobre un SampleBuffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResamplingQuality {
    /// Interpolación lineal (muy bajo coste de CPU)
    Linear,
    /// Interpolación cúbica Hermite (estándar de alta fidelidad)
    HermiteCubic,
}

/// Lee una muestra interpolada del buffer en una posición fraccionaria continua.
/// Aprovecha las 2 muestras de guarda (sentinel padding) al final de `SampleBuffer`
/// para leer `chan[index + 1]` y `chan[index + 2]` directamente sin ramas condicionales.
#[inline]
pub fn read_interpolated_sample(
    buffer: &SampleBuffer,
    channel: usize,
    pos: f64,
    quality: ResamplingQuality,
) -> f32 {
    let chan = if channel == 0 {
        &buffer.left
    } else {
        &buffer.right
    };
    let len = buffer.len();
    if len == 0 || pos < 0.0 || pos >= len as f64 || chan.len() < len + 2 {
        return 0.0;
    }

    let index = pos.floor() as usize;
    let frac = (pos - index as f64) as f32;

    match quality {
        ResamplingQuality::Linear => {
            let y1 = chan[index];
            let y2 = chan[index + 1];
            y1 + frac * (y2 - y1)
        }
        ResamplingQuality::HermiteCubic => {
            let y0 = chan[index.saturating_sub(1)];
            let y1 = chan[index];
            let y2 = chan[index + 1];
            let y3 = chan[index + 2];
            hermite_interpolate_4p(y0, y1, y2, y3, frac)
        }
    }
}
