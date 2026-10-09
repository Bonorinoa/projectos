// Gate C2 probe — unbounded allocation bomb for the hard memory ceiling test (C5).
// Expected: wasmtime traps (memory limit exceeded) and the process dies cleanly.
"use strict";
const keep = [];
for (;;) { keep.push(new Array(65536).fill(7)); }