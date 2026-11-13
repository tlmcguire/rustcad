# T004 report: kernel worker process spike

Date: 2026-11-13. Machine: Apple M2 (8 cores), macOS 27.0.1. OCCT 7.9.3, release build.
Code: `cad_kernel_occt::worker` and the `occt-worker` binary.

## Result

A worker process serves every `Kernel` call over a pipe, survives being killed mid-call,
and adds between 10 and 40 microseconds to a call that returns little data. Returning a mesh
adds about 2.4 microseconds per kilobyte. Against kernel calls that take milliseconds, that
is small enough to run everything in workers, previews included.

| Acceptance point | Status | Evidence |
|---|---|---|
| 1. A worker process serves `Kernel` calls over a pipe | Met | `worker_gives_the_same_answers_as_the_in_process_kernel`: history, face count, mass properties, and mesh are identical |
| 2. Killing the worker mid-call returns `BackendLost` and the next call succeeds | Met | `killing_the_worker_mid_call_...`; a call that overruns its timeout is handled the same way |
| 3. Latency for box, boolean, and tessellate at three mesh sizes, both ways | Met | Table below |
| 4. Recommendation for previews | Workers | See "Recommendation" |

macOS only. The code uses nothing platform-specific, but Linux and Windows are untested.

## Measurements

`cargo test --release -p cad-kernel-occt --test worker -- --ignored --nocapture`

Median round trip. The client and worker run on the same machine, one call at a time.

| Call | In-process | Worker | Added |
|---|---|---|---|
| `face_count` | under 0.1 us | 9.2 us | 9.2 us |
| `make_box` | 15.8 us | 27.6 us | 11.8 us |
| `mass_props` of a box | 96.6 us | 110.8 us | 14.2 us |
| `boolean`, box minus pocket | 1,697 us | 1,694 us | within noise |
| `tessellate`, 28 triangles (2.8 kB) | 176 us | 206 us | 30 us |
| `tessellate`, 412 triangles (37 kB) | 2,237 us | 2,277 us | 40 us |
| `tessellate`, 4,108 triangles (362 kB) | 26,655 us | 27,531 us | 876 us |

Starting a worker, measured as the first call to a new one: 77 ms.

Reading the numbers:

- The fixed cost of crossing the process boundary is about 10 us. It is the whole of
  `face_count` and invisible in a boolean.
- Mesh transfer costs about 2.4 us per kilobyte at the largest size, which is 3% of that
  call. The mesh format is the simplest possible: 64-bit positions and normals, copied twice.
- The three mesh sizes are slabs with 1, 25, and 256 square pockets. The `Kernel` trait can
  only make boxes, so nothing larger was practical: building the largest took 10.7 s of
  booleans. **A million-triangle mesh was not measured.** Extrapolating linearly it would be
  about 88 MB and 200 ms per transfer, which is too slow for interaction. See open item 1.

Two findings about the backend itself, not the worker:

- **Tessellation is slow and is not cached.** 27 ms for 4,108 triangles, on every call,
  even when the shape has not changed. The time is in OCCT and the shim, not in transport.
- **Repeated booleans scale badly.** 256 cuts into one slab took 10.7 s, about 42 ms each on
  average, against 1.7 ms for a cut into a plain box.

## How it works

- **Protocol.** Length-prefixed binary frames on the worker's standard input and output.
  A request is an opcode and its arguments; a response is a status byte and the result or
  the error. Hand-written, no serialization dependency. Decoding checks every count against
  the bytes that remain, so a corrupt message is rejected instead of trusted.
- **Worker.** `serve` reads requests and answers them in order with any `Kernel`. The
  `occt-worker` binary runs it over an `OcctKernel`.
- **Client.** `WorkerKernel` implements `Kernel`. It starts the worker on the first call
  and holds a lock for the length of each call, so calls are serialised. A reader thread
  feeds response frames to a channel so that waiting can time out.
- **Losing the worker.** If the worker exits, overruns the timeout (30 s by default), or
  sends something that does not decode, the call returns `BackendLost`, the process is
  killed and reaped, and the next call starts a new one.
- **Handles.** The client issues its own shape handles and maps them to the worker's. When a
  worker is lost, every handle it issued returns `UnknownShape`. Nothing is replayed: the
  document layer rebuilds, since geometry is a cache (D4).
- No `unsafe`, and nothing in the module depends on OCCT except the binary.

## Recommendation

**Run the kernel in workers for everything, previews included.** The boundary costs 10 to
40 us on calls that take 100 us to tens of milliseconds, and the 50 ms preview budget
(PLAN 9.5) is spent in OCCT, not in transport. An in-process preview path would save well
under 1 ms per call and would give up crash containment exactly where bad input is most
likely, while the user is dragging.

Conditions on that recommendation:

1. Large meshes must not be copied through a pipe. Before models reach hundreds of
   thousands of triangles, move mesh transfer to shared memory, or send 32-bit data, or both.
2. Keep a worker warm. Starting one costs 77 ms, which is over the preview budget by itself.
3. Fix tessellation cost in the backend first. It dominates any preview, in either process.

## Open items

1. **Large meshes are unmeasured**, as above. This is the one result that could change the
   recommendation.
2. **One worker, one call at a time.** PLAN 6.4 wants a pool for multi-core rebuild. A pool
   needs shapes to move between workers, which means serialising B-rep (`to_brep_bytes` in
   the full trait). Not attempted.
3. **A lost worker loses all its shapes.** Recovery is a full rebuild of whatever lived
   there. The rebuild engine does not exist yet, so the cost of that is unknown.
4. **OCCT can print to standard output**, which would corrupt the protocol. The client
   would detect it and report `BackendLost`, but the worker should move the protocol to a
   private descriptor. Not observed in these tests.
5. **The timeout is one value for every call.** A long boolean and a hung worker look the
   same for 30 s. It should scale with the operation, and cancellation is not implemented.
6. **No protocol version.** Client and worker are built together for now.
7. **Finding the worker binary** is left to the caller. The app needs a rule for where it
   ships (next to the main binary, PLAN 6.3).
8. **Memory.** The worker's memory use and the cost of two copies of every mesh were not
   measured.
9. **The module lives in `cad-kernel-occt`** but is generic over `Kernel`. If the pure Rust
   backend wants it, it should move down a layer.
