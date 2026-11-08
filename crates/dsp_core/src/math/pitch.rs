//! Conversiones de nota MIDI, frecuencia en hercios (Hz) y desafinación en cents.

/// Convierte una nota MIDI fraccionaria (donde `69.0 = A4 = 440 Hz`) a frecuencia en Hz.
#[inline(always)]
pub fn midi_to_hz(midi_note: f32) -> f32 {
    440.0 * 2.0_f32.powf((midi_note - 69.0) / 12.0)
}

/// Convierte una frecuencia en Hz a número de nota MIDI fraccionario.
#[inline(always)]
pub fn hz_to_midi(freq_hz: f32) -> f32 {
    let safe_hz = freq_hz.max(1.0);
    69.0 + 12.0 * (safe_hz / 440.0).log2()
}

/// Convierte una desafinación en cents (`100 cents = 1 semitono`) a razón multiplicativa de frecuencia.
#[inline(always)]
pub fn detune_cents_ratio(cents: f32) -> f32 {
    2.0_f32.powf(cents / 1200.0)
}
