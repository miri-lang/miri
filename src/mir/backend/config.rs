// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Backend configuration for GPU and other accelerators.
//!
//! This module defines compile-time configuration parameters for GPU backends
//! (block sizes, memory limits, etc.). The configuration is backend-agnostic but
//! currently specialized to WebGPU.

/// Backend-neutral configuration for GPU code generation.
///
/// Holds block sizes and other tuning parameters that affect how kernels are
/// compiled and dispatched. The configuration is thread-local or passed through
/// the lowering context to ensure a single source of truth for block sizes —
/// they must match between the kernel's `@workgroup_size` directive in WGSL
/// and the runtime dispatch descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendConfig {
    /// Block sizes for 1D, 2D, and 3D kernels.
    /// Indexed by `rank - 1` (rank 1 → index 0, rank 2 → index 1, rank 3 → index 2).
    /// Each entry is `[x, y, z]` workgroup dimensions where unused dimensions are 1.
    block_sizes: [[u32; 3]; 3],
}

impl BackendConfig {
    /// WebGPU backend configuration (the only backend today).
    ///
    /// Block sizes chosen to fit typical GPU memory constraints and
    /// warp/wavefront architectures:
    /// - 1D: 256 threads (fits common compute shader limits)
    /// - 2D: 16×16 = 256 threads
    /// - 3D: 8×8×4 = 256 threads
    ///
    /// This is a backend parameter — distinct backends can override with different values.
    pub const WEB_GPU: BackendConfig = BackendConfig {
        block_sizes: [
            [256, 1, 1], // rank 1 (1D)
            [16, 16, 1], // rank 2 (2D)
            [8, 8, 4],   // rank 3 (3D)
        ],
    };

    /// Block size for a given rank (1, 2, or 3 dimensions).
    ///
    /// Returns `[x, y, z]` workgroup dimensions where unused dimensions are 1.
    /// All configurations use 256 threads total to fit typical GPU limits.
    ///
    /// The `rank` parameter must be in the range 1..=3. This is guaranteed by the
    /// dispatch seam in the MIR lowering code (which validates loop variable count
    /// at the AST stage). If `rank` is out of range, this function will clamp defensively
    /// so an out-of-range rank can never index out of bounds.
    pub fn block_size(&self, rank: usize) -> [u32; 3] {
        // rank is validated to 1..=3 at the forall dispatch seam; clamp defensively
        // so an out-of-range rank can never index out of bounds.
        let idx = rank.clamp(1, 3) - 1;
        self.block_sizes[idx]
    }
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self::WEB_GPU
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_size_1d() {
        let cfg = BackendConfig::WEB_GPU;
        assert_eq!(cfg.block_size(1), [256, 1, 1]);
    }

    #[test]
    fn block_size_2d() {
        let cfg = BackendConfig::WEB_GPU;
        assert_eq!(cfg.block_size(2), [16, 16, 1]);
    }

    #[test]
    fn block_size_3d() {
        let cfg = BackendConfig::WEB_GPU;
        assert_eq!(cfg.block_size(3), [8, 8, 4]);
    }

    #[test]
    fn block_size_all_256_threads() {
        let cfg = BackendConfig::WEB_GPU;
        assert_eq!(cfg.block_size(1)[0], 256);
        assert_eq!(cfg.block_size(2)[0] * cfg.block_size(2)[1], 256);
        assert_eq!(
            cfg.block_size(3)[0] * cfg.block_size(3)[1] * cfg.block_size(3)[2],
            256
        );
    }
}
