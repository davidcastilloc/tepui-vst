//! Algoritmo de remuestreo (Resampler) con Interpolación Cúbica de Hermite.
//! Garantiza transposición continua sin discontinuidades ni artefactos de alta frecuencia.

use crate::buffer::SampleBuffer;

/// Calcula el factor de velocidad de reproducción para una nota MIDI dada.
///
/// # Fórmula
/// `ratio = 2.0^((midi_note - root_note + cents / 100.0) / 12.0) * (sample_sr / target_sr)`
#[inline]
pub fn calculate_playback_ratio(
    midi_note: u8,
    root_note: u8,
    cents: f32,
    sample_rate_sample: f32,
    sample_rate_out: f32,
) -> f64 {
    let semitone_diff = (midi_note as f32 - root_note as f32) + (cents / 100.0);
    let pitch_multiplier = 2.0_f64.powf(semitone_diff as f64 / 12.0);
    let sr_multiplier = (sample_rate_sample as f64) / (sample_rate_out as f64);
    pitch_multiplier * sr_multiplier
}

/// Interpolador Cúbico de Hermite de 4 puntos.
///
/// Recibe 4 muestras adyacentes (y0, y1, y2, y3) y la fracción t en [0.0, 1.0).
/// Retorna el valor suavemente interpolado en t entre y1 e y2.
#[inline(always)]
pub fn hermite_interpolate_4p(y0: f32, y1: f32, y2: f32, y3: f32, t: f32) -> f32 {
    let c0 = y1;
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);

    ((c3 * t + c2) * t + c1) * t + c0
}

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
