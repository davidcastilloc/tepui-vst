//! Primitivas de tiempo real libres de bloqueos y de asignaciones dinámicas (`rt_primitives`).
//!
//! Re-exporta directamente las primitivas universales de [`dsp_core::primitives`]:
//! 1. [`OnePoleSmoother`]: Suavizador exponencial de 1 polo para eliminar *zipper noise*.
//! 2. [`SeqLockArray`]: Arreglo compartido con protocolo *Sequence Lock* (`Acquire`/`Release`) libre de *torn reads*.
//! 3. [`SpscRingBuffer`]: Cola circular SPSC de capacidad potencia de 2 en pila.

pub use dsp_core::primitives::{OnePoleSmoother, SeqLockArray, SpscRingBuffer};
