// SPDX-License-Identifier: CDDL-1.0
// Copyright 2019-2025 LibRaw LLC (info@libraw.org).
// dcraw.c portions copyright 1997-2018 Dave Coffin (dcoffin a cybercom o net).
// Modified by Antonio Dicorato / TrueRenderer, 2026-10-11:
// D750/D800 XYZ-to-camera matrices and 12/14-bit lossless Huffman tables extracted
// from LibRaw 0.22.2 tables into Rust constant data; no decoder code included.
// Upstream attributes the colour matrix to Adobe DNG Converter.
// This file is covered by LICENSE.CDDL in this directory, not the proprietary
// project license. Sources, exact portions and hashes: manifest.json.
pub const D750_MATRIX: [i32; 9] = [9020, -2890, -715, -4535, 12436, 2348, -934, 1919, 7086];
pub const D800_MATRIX: [i32; 9] = [7866, -2108, -555, -4869, 12483, 2681, -1176, 2069, 7501];
pub const LOSSLESS12_COUNTS: [u8; 8] = [0, 1, 4, 2, 3, 1, 2, 0];
pub const LOSSLESS12_CATEGORIES: [u8; 13] = [5, 4, 6, 3, 7, 2, 8, 1, 9, 0, 10, 11, 12];
pub const LOSSLESS14_COUNTS: [u8; 8] = [0, 1, 4, 2, 2, 3, 1, 2];
pub const LOSSLESS14_CATEGORIES: [u8; 15] = [7, 6, 8, 5, 9, 4, 10, 3, 11, 12, 2, 0, 1, 13, 14];
