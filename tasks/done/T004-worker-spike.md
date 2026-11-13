# T004 Kernel worker process spike

**Goal:** Measure the cost of running the kernel in a separate process.
**Acceptance:**
1. A worker process serves `Kernel` calls over a pipe or shared memory.
2. Killing the worker mid-call returns `BackendLost` and the next call succeeds.
3. Report: round-trip latency for box, boolean, and tessellate at three mesh sizes, in-process versus worker.
4. Recommendation for previews: in-process or worker.
**Depends on:** T003.
