// SPDX-License-Identifier: CDDL-1.0
// Copyright 2019-2025 LibRaw LLC (info@libraw.org).
// dcraw.c portions copyright 1997-2018 Dave Coffin.
// Modified by Antonio Dicorato / TrueRenderer, 2026-10-11: nine X-T5
// XYZ-to-camera D65 coefficients extracted as data from LibRaw 0.22.2.
// Upstream attributes the matrix to Adobe. Not measured by TrueRenderer.
// See LICENSE.CDDL and manifest.json in this directory.
pub const MATRIX: [i32; 9] = [11809, -5358, -1141, -4248, 12164, 2343, -514, 1097, 5848];
