// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::mir::backend::config::BackendConfig;

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
