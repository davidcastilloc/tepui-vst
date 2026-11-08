//! Algoritmos de interpolación sin asignaciones para líneas de retardo y osciladores.

/// Interpolación lineal rápida entre `a` y `b` con fracción `t` en `[0.0, 1.0]`.
#[inline(always)]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + t * (b - a)
}

/// Interpolación polinómica Hermite de 4 puntos (3er orden) para lectura de tablas y líneas de retardo fraccionarias.
#[inline(always)]
pub fn hermite4(frac: f32, xm1: f32, x0: f32, x1: f32, x2: f32) -> f32 {
    let c = (x1 - xm1) * 0.5;
    let v = x0 - x1;
    let w = c + v;
    let a = w + v + (x2 - x0) * 0.5;
    let b_neg = w + a;
    (((a * frac) - b_neg) * frac + c) * frac + x0
}
