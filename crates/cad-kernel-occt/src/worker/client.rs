use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, BufReader, BufWriter};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use cad_kernel::{BoolOp, KResult, Kernel, KernelError, MassProps, Mesh, OpResult, ShapeId};
use cad_math::{Tolerance, Vec3};

use super::wire::{Reply, Request, decode_response, read_frame, write_frame};

/// How to start and supervise a worker process.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// The worker executable, normally the `occt-worker` binary of this crate.
    pub program: PathBuf,
    /// Arguments passed to it.
    pub args: Vec<OsString>,
    /// How long one call may take before the worker is killed.
    pub timeout: Duration,
}

impl WorkerConfig {
    /// A configuration with no arguments and a 30 second timeout per call.
    #[must_use]
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            timeout: Duration::from_secs(30),
        }
    }
}

/// A live connection to one worker process.
struct Link {
    stdin: BufWriter<ChildStdin>,
    /// Response frames, read by a thread so that waiting for one can time out.
    frames: Receiver<io::Result<Vec<u8>>>,
}

#[derive(Default)]
struct Session {
    link: Option<Link>,
    /// Counts workers. A handle is only valid for the worker that issued it.
    generation: u64,
    next_id: u64,
    /// Our handle to (generation, the worker's handle).
    shapes: BTreeMap<u64, (u64, ShapeId)>,
    lost: u64,
}

/// A kernel that runs in a separate worker process.
///
/// Calls are sent one at a time over a pipe. If the worker crashes, hangs past the timeout,
/// or sends something corrupt, the call returns [`KernelError::BackendLost`], the worker is
/// killed, and the next call starts a fresh one. Shapes that lived in the lost worker are
/// gone: their handles return [`KernelError::UnknownShape`], and the caller rebuilds them
/// (geometry is a cache, DECISIONS.md D4).
pub struct WorkerKernel {
    config: WorkerConfig,
    /// Held for the whole of each call, so calls are serialised.
    session: Mutex<Session>,
    /// Separate from `session` so the worker can be killed while a call is waiting on it.
    process: Mutex<Option<Child>>,
}

impl std::fmt::Debug for WorkerKernel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerKernel")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // Both mutexes guard state that stays valid if a holder panicked.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl WorkerKernel {
    /// Creates the kernel. The worker process is started by the first call.
    #[must_use]
    pub fn new(config: WorkerConfig) -> Self {
        Self {
            config,
            session: Mutex::new(Session::default()),
            process: Mutex::new(None),
        }
    }

    /// How many workers have been lost to a crash, a timeout, or a corrupt reply.
    #[must_use]
    pub fn workers_lost(&self) -> u64 {
        locked(&self.session).lost
    }

    /// Kills the worker process, if one is running. A call in progress on another thread
    /// returns [`KernelError::BackendLost`]. Safe to call at any time.
    pub fn kill_worker(&self) {
        if let Some(child) = locked(&self.process).as_mut() {
            // Failing to kill means it has already exited, which is what we wanted.
            let _ = child.kill();
        }
    }

    fn start(&self) -> Result<Link, String> {
        let mut child = Command::new(&self.config.program)
            .args(&self.config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("cannot start {}: {e}", self.config.program.display()))?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("worker started without pipes".into());
        };
        *locked(&self.process) = Some(child);

        let (sender, frames) = mpsc::channel();
        std::thread::spawn(move || {
            let mut stdout = BufReader::new(stdout);
            loop {
                let frame = match read_frame(&mut stdout) {
                    Ok(Some(body)) => Ok(body),
                    Ok(None) => Err(io::Error::other("the worker process exited")),
                    Err(e) => Err(e),
                };
                let last = frame.is_err();
                // A send fails only when the kernel has already given up on this worker.
                if sender.send(frame).is_err() || last {
                    break;
                }
            }
        });
        Ok(Link {
            stdin: BufWriter::new(stdin),
            frames,
        })
    }

    /// Sends one request and waits for its response. `Err` means the worker is unusable.
    fn exchange(&self, session: &mut Session, request: &Request) -> Result<KResult<Reply>, String> {
        let link = match &mut session.link {
            Some(link) => link,
            empty => empty.insert(self.start()?),
        };
        write_frame(&mut link.stdin, &request.encode())
            .map_err(|e| format!("cannot send to the worker: {e}"))?;
        let body = match link.frames.recv_timeout(self.config.timeout) {
            Ok(Ok(body)) => body,
            Ok(Err(e)) => return Err(e.to_string()),
            Err(RecvTimeoutError::Timeout) => {
                return Err(format!("no answer within {:?}", self.config.timeout));
            }
            Err(RecvTimeoutError::Disconnected) => return Err("the worker process exited".into()),
        };
        decode_response(request, &body).map_err(|e| e.to_string())
    }

    /// Makes one call. On any transport failure the worker is discarded.
    fn call(&self, session: &mut Session, request: Request) -> KResult<Reply> {
        match self.exchange(session, &request) {
            Ok(result) => result,
            Err(why) => {
                self.discard(session);
                Err(KernelError::BackendLost(why))
            }
        }
    }

    /// Drops the connection, kills the process, and invalidates every handle it issued.
    fn discard(&self, session: &mut Session) {
        if session.link.take().is_some() {
            session.lost += 1;
        }
        session.generation += 1;
        session.shapes.clear();
        self.reap();
    }

    fn reap(&self) {
        if let Some(mut child) = locked(&self.process).take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn remote(session: &Session, shape: ShapeId) -> KResult<ShapeId> {
        match session.shapes.get(&shape.0) {
            Some((generation, remote)) if *generation == session.generation => Ok(*remote),
            _ => Err(KernelError::UnknownShape(shape)),
        }
    }

    fn register(session: &mut Session, remote: ShapeId) -> ShapeId {
        let id = session.next_id;
        session.next_id += 1;
        session.shapes.insert(id, (session.generation, remote));
        ShapeId(id)
    }
}

impl WorkerKernel {
    /// A call on one of our shapes that returns bytes.
    fn shape_to_bytes(
        &self,
        shape: ShapeId,
        request: impl FnOnce(ShapeId) -> Request,
    ) -> KResult<Vec<u8>> {
        let mut session = locked(&self.session);
        let remote = Self::remote(&session, shape)?;
        match self.call(&mut session, request(remote)) {
            Ok(Reply::Bytes(bytes)) => Ok(bytes),
            Ok(_) => Err(unexpected()),
            Err(KernelError::UnknownShape(_)) => Err(KernelError::UnknownShape(shape)),
            Err(e) => Err(e),
        }
    }

    /// A call that creates a shape in the worker.
    fn new_shape(&self, request: Request) -> KResult<ShapeId> {
        let mut session = locked(&self.session);
        match self.call(&mut session, request)? {
            Reply::Shape(remote) => Ok(Self::register(&mut session, remote)),
            _ => Err(unexpected()),
        }
    }
}

fn unexpected() -> KernelError {
    KernelError::BackendLost("the worker sent the wrong kind of reply".into())
}

impl Kernel for WorkerKernel {
    fn name(&self) -> &'static str {
        "occt-worker"
    }

    fn make_box(&self, origin: Vec3, size: Vec3) -> KResult<ShapeId> {
        self.new_shape(Request::MakeBox { origin, size })
    }

    fn boolean(&self, op: BoolOp, a: ShapeId, b: ShapeId) -> KResult<OpResult> {
        let mut session = locked(&self.session);
        let (remote_a, remote_b) = (Self::remote(&session, a)?, Self::remote(&session, b)?);
        // Translate the worker's handles back to ours, in results and in errors.
        let ours = |remote: ShapeId| if remote == remote_a { a } else { b };
        let request = Request::Boolean {
            op,
            a: remote_a,
            b: remote_b,
        };
        match self.call(&mut session, request) {
            Ok(Reply::Boolean { shape, mut history }) => {
                for entry in &mut history.faces {
                    entry.source_shape = ours(entry.source_shape);
                }
                Ok(OpResult {
                    shape: Self::register(&mut session, shape),
                    history,
                })
            }
            Ok(_) => Err(unexpected()),
            Err(KernelError::UnknownShape(remote)) => Err(KernelError::UnknownShape(ours(remote))),
            Err(e) => Err(e),
        }
    }

    fn face_count(&self, shape: ShapeId) -> KResult<usize> {
        let mut session = locked(&self.session);
        let remote = Self::remote(&session, shape)?;
        match self.call(&mut session, Request::FaceCount(remote)) {
            Ok(Reply::Count(n)) => usize::try_from(n).map_err(|_| unexpected()),
            Ok(_) => Err(unexpected()),
            Err(KernelError::UnknownShape(_)) => Err(KernelError::UnknownShape(shape)),
            Err(e) => Err(e),
        }
    }

    fn tessellate(&self, shape: ShapeId, tol: Tolerance) -> KResult<Mesh> {
        let mut session = locked(&self.session);
        let remote = Self::remote(&session, shape)?;
        let request = Request::Tessellate {
            shape: remote,
            linear: tol.linear,
            angular: tol.angular,
        };
        match self.call(&mut session, request) {
            Ok(Reply::Mesh(mesh)) => Ok(mesh),
            Ok(_) => Err(unexpected()),
            Err(KernelError::UnknownShape(_)) => Err(KernelError::UnknownShape(shape)),
            Err(e) => Err(e),
        }
    }

    fn mass_props(&self, shape: ShapeId) -> KResult<MassProps> {
        let mut session = locked(&self.session);
        let remote = Self::remote(&session, shape)?;
        match self.call(&mut session, Request::MassProps(remote)) {
            Ok(Reply::MassProps(props)) => Ok(props),
            Ok(_) => Err(unexpected()),
            Err(KernelError::UnknownShape(_)) => Err(KernelError::UnknownShape(shape)),
            Err(e) => Err(e),
        }
    }

    fn release(&self, shape: ShapeId) {
        let mut session = locked(&self.session);
        let Ok(remote) = Self::remote(&session, shape) else {
            return;
        };
        session.shapes.remove(&shape.0);
        // If the worker is lost here the shape is gone with it, which is what was asked for.
        let _ = self.call(&mut session, Request::Release(remote));
    }

    fn to_brep_bytes(&self, shape: ShapeId) -> KResult<Vec<u8>> {
        self.shape_to_bytes(shape, Request::ToBrep)
    }

    fn from_brep_bytes(&self, bytes: &[u8]) -> KResult<ShapeId> {
        self.new_shape(Request::FromBrep(bytes.to_vec()))
    }

    fn export_step(&self, shape: ShapeId) -> KResult<Vec<u8>> {
        self.shape_to_bytes(shape, Request::ExportStep)
    }

    fn import_step(&self, bytes: &[u8]) -> KResult<ShapeId> {
        self.new_shape(Request::ImportStep(bytes.to_vec()))
    }
}

impl Drop for WorkerKernel {
    fn drop(&mut self) {
        // Closing the pipe lets the worker finish by itself; `reap` makes sure of it.
        locked(&self.session).link = None;
        self.reap();
    }
}
