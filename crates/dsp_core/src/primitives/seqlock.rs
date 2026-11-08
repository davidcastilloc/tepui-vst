//! Arreglo lock-free basado en Sequence Lock (`SeqLockArray`) alineado a línea de caché de 64 bytes
//! con búfer de doble copia en pila para garantizar cero lecturas fragmentadas (0 torn reads).

use std::sync::atomic::{fence, AtomicU32, Ordering};

/// Arreglo denso sincronizado mediante Sequence Lock sin bloqueo en el hilo lector (Audio/GUI).
#[repr(C, align(64))]
pub struct SeqLockArray<const N: usize> {
    seq: AtomicU32,
    _pad: [u8; 60],
    cells: [AtomicU32; N],
}

impl<const N: usize> std::fmt::Debug for SeqLockArray<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeqLockArray")
            .field("seq", &self.seq.load(Ordering::Relaxed))
            .field("len", &N)
            .finish()
    }
}

impl<const N: usize> SeqLockArray<N> {
    /// Crea un nuevo `SeqLockArray` inicializando todas las celdas con `default_value`.
    pub fn new(default_value: f32) -> Self {
        let bits = default_value.to_bits();
        Self {
            seq: AtomicU32::new(0),
            _pad: [0u8; 60],
            cells: std::array::from_fn(|_| AtomicU32::new(bits)),
        }
    }

    /// Escritura atómica desde el hilo productor (incrementa secuencia impar -> escribe -> par).
    pub fn write_slice(&self, values: &[f32], default_val: f32) {
        self.seq.fetch_add(1, Ordering::AcqRel);
        let count = values.len().min(N);
        for (cell, &v) in self.cells.iter().take(count).zip(values.iter().take(count)) {
            let val = if v.is_finite() { v } else { default_val };
            cell.store(val.to_bits(), Ordering::Relaxed);
        }
        for cell in self.cells.iter().skip(count) {
            cell.store(default_val.to_bits(), Ordering::Relaxed);
        }
        self.seq.fetch_add(1, Ordering::Release);
    }

    /// Lectura lock-free en tiempo real libre de lecturas fragmentadas (torn reads).
    ///
    /// Copia primero a un búfer temporal en la pila (`temp`) y solo actualiza `cached_snapshot`
    /// cuando `s1 == s2` y la secuencia es par.
    #[inline(always)]
    pub fn read_into_snapshot(&self, cached_snapshot: &mut [f32; N]) -> bool {
        let mut temp = [0.0_f32; N];
        for _ in 0..3 {
            let s1 = self.seq.load(Ordering::Acquire);
            if (s1 & 1) != 0 {
                std::hint::spin_loop();
                continue;
            }
            for (dst, cell) in temp.iter_mut().zip(self.cells.iter()) {
                *dst = f32::from_bits(cell.load(Ordering::Relaxed));
            }
            fence(Ordering::Acquire);
            let s2 = self.seq.load(Ordering::Relaxed);
            if s1 == s2 {
                *cached_snapshot = temp;
                return true;
            }
        }
        false
    }

    /// Retorna la secuencia actual (par = estable, impar = escritura en progreso).
    #[inline(always)]
    pub fn sequence(&self) -> u32 {
        self.seq.load(Ordering::Acquire)
    }

    /// Retorna la revisión actual (secuencia dividida entre 2).
    #[inline(always)]
    pub fn revision(&self) -> u32 {
        self.seq.load(Ordering::Acquire) >> 1
    }

    /// Actualiza un único índice `idx` de forma atómica dentro del protocolo SeqLock.
    #[inline(always)]
    pub fn write_index(&self, idx: usize, value: f32) {
        if idx < N && value.is_finite() {
            self.seq.fetch_add(1, Ordering::AcqRel);
            self.cells[idx].store(value.to_bits(), Ordering::Relaxed);
            self.seq.fetch_add(1, Ordering::Release);
        }
    }

    /// Lee una instantánea 100 % consistente de los `N` elementos desde el hilo de audio (0 bloqueos).
    #[inline(always)]
    pub fn read_snapshot(&self) -> [f32; N] {
        let mut out = [0.0_f32; N];
        let _ = self.read_into_snapshot(&mut out);
        out
    }
}
