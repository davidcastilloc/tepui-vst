//! Protección contra números subnormales (denormals) y sanitización IEEE 754 en tiempo real.

/// Purga números subnormales (denormals) hacia `0.0` usando manipulación a nivel de bits IEEE 754
/// para evitar penalizaciones de microcódigo en la FPU durante colas de silencio.
#[inline(always)]
pub fn flush_denormal(x: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    let bits = x.to_bits();
    let exp = (bits >> 23) & 0xFF;
    if exp == 0 || x.abs() < 1.0e-20 {
        0.0
    } else {
        x
    }
}

/// Sanitiza una muestra de audio garantizando ausencia de `NaN`, `±Inf`, subnormales y picos extremos.
#[inline(always)]
pub fn sanitize_sample(x: f32, max_abs_limit: f32) -> f32 {
    let clean = flush_denormal(x);
    clean.clamp(-max_abs_limit, max_abs_limit)
}
