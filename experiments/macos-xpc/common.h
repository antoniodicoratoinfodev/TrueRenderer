#pragma once

// This is a synthetic capability probe, never a decoder or a production protocol.
#ifndef TR_PROBE_SERVICE
#define TR_PROBE_SERVICE "it.truerenderer.sandboxprobe.worker"
#endif
#ifndef TR_PROBE_HOST
#define TR_PROBE_HOST "it.truerenderer.sandboxprobe"
#endif
#ifndef TR_PROBE_LIMIT_CHILDREN
#define TR_PROBE_LIMIT_CHILDREN 0
#endif
#define TR_PROBE_SOURCE "TrueRenderer XPC probe: readonly source.\n"
#define TR_PROBE_VERSION 1
