//! One application CPU pool shared by pyramid construction and presentation.
use std::sync::{Arc, OnceLock, RwLock};
static POOL: OnceLock<RwLock<Arc<rayon::ThreadPool>>> = OnceLock::new();
fn pool() -> &'static RwLock<Arc<rayon::ThreadPool>> {
    POOL.get_or_init(|| {
        RwLock::new(Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(
                    std::thread::available_parallelism().map_or(1, |n| (n.get() / 2).max(1)),
                )
                .thread_name(|i| format!("tr-compute-{i}"))
                .build()
                .expect("CPU pool"),
        ))
    })
}
pub fn configure(threads: usize) -> anyhow::Result<()> {
    let threads = threads.clamp(1, 256);
    if pool().read().unwrap().current_num_threads() == threads {
        return Ok(());
    }
    let next = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .thread_name(|i| format!("tr-compute-{i}"))
        .build()?;
    *pool().write().unwrap() = Arc::new(next);
    Ok(())
}
pub fn install<T: Send>(work: impl FnOnce() -> T + Send) -> T {
    let pool = pool().read().unwrap().clone();
    pool.install(work)
}

/// Channel SIMD, with the same f64 multiply/add order as the scalar reference.
/// No FMA or global AVX2 requirement. Geometry and coefficients remain f64.
#[inline]
pub(crate) fn accumulate(sum: &mut [f64; 4], pixel: [f32; 4], weight: f64) {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        use std::arch::aarch64::*;
        let p = pixel.map(f64::from);
        let w = vdupq_n_f64(weight);
        for i in [0, 2] {
            let value = vaddq_f64(
                vld1q_f64(sum.as_ptr().add(i)),
                vmulq_f64(vld1q_f64(p.as_ptr().add(i)), w),
            );
            vst1q_f64(sum.as_mut_ptr().add(i), value);
        }
    }
    #[cfg(not(target_arch = "aarch64"))]
    for c in 0..4 {
        sum[c] += pixel[c] as f64 * weight;
    }
}
