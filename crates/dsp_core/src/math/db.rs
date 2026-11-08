//! Conversiones entre decibelios (dB) y ganancia de amplitud lineal.

/// Umbral mínimo de silencio en decibelios (-120 dB).
pub const MIN_FLOOR_DB: f32 = -120.0;

/// Convierte decibelios (dB) a ganancia lineal de amplitud (`10^(dB / 20)`).
#[inline(always)]
pub fn db_to_gain(db: f32) -> f32 {
    if !db.is_finite() || db <= MIN_FLOOR_DB {
        0.0
    } else {
        10.0_f32.powf(db * 0.05)
    }
}

/// Convierte ganancia lineal de amplitud a decibelios (`20 * log10(gain)`).
#[inline(always)]
pub fn gain_to_db(gain: f32) -> f32 {
    let abs_g = gain.abs();
    if !abs_g.is_finite() || abs_g <= 1e-6 {
        MIN_FLOOR_DB
    } else {
        (20.0 * abs_g.log10()).max(MIN_FLOOR_DB)
    }
}
