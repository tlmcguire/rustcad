use std::io::{self, Read, Write};
use std::time::Duration;

use cad_kernel::{Kernel, KernelError};
use cad_math::Tolerance;

use super::wire::{Reply, Request, encode_response, read_frame, write_frame};

/// Options for [`serve`].
#[derive(Debug, Clone, Copy, Default)]
pub struct ServeOptions {
    /// Sleep this long before executing each request. For tests that need a slow call.
    pub delay: Option<Duration>,
}

/// Serves kernel calls read from `input`, writing responses to `output`, until `input` ends.
///
/// One request is handled at a time, in order. A request that cannot be decoded ends the
/// session with an error, since the stream can no longer be trusted.
///
/// # Errors
///
/// Returns an error when the streams fail or a request is corrupt.
pub fn serve(
    kernel: &dyn Kernel,
    mut input: impl Read,
    mut output: impl Write,
    options: ServeOptions,
) -> io::Result<()> {
    while let Some(body) = read_frame(&mut input)? {
        let request = Request::decode(&body)?;
        if let Some(delay) = options.delay {
            std::thread::sleep(delay);
        }
        write_frame(&mut output, &encode_response(&execute(kernel, request)))?;
    }
    Ok(())
}

fn execute(kernel: &dyn Kernel, request: Request) -> Result<Reply, KernelError> {
    Ok(match request {
        Request::MakeBox { origin, size } => Reply::Shape(kernel.make_box(origin, size)?),
        Request::Boolean { op, a, b } => {
            let result = kernel.boolean(op, a, b)?;
            Reply::Boolean {
                shape: result.shape,
                history: result.history,
            }
        }
        Request::FaceCount(shape) => Reply::Count(kernel.face_count(shape)? as u64),
        Request::Tessellate {
            shape,
            linear,
            angular,
        } => Reply::Mesh(kernel.tessellate(shape, Tolerance { linear, angular })?),
        Request::MassProps(shape) => Reply::MassProps(kernel.mass_props(shape)?),
        Request::Release(shape) => {
            kernel.release(shape);
            Reply::Done
        }
        Request::ToBrep(shape) => Reply::Bytes(kernel.to_brep_bytes(shape)?),
        Request::FromBrep(bytes) => Reply::Shape(kernel.from_brep_bytes(&bytes)?),
        Request::ExportStep(shape) => Reply::Bytes(kernel.export_step(shape)?),
        Request::ImportStep(bytes) => Reply::Shape(kernel.import_step(&bytes)?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::wire::decode_response;
    use cad_kernel::{BoolOp, KResult, MassProps, Mesh, OpResult, ShapeId};
    use cad_math::Vec3;

    /// A kernel that needs no geometry library: every box is shape 7 with six faces.
    struct Fake;

    impl Kernel for Fake {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn make_box(&self, _: Vec3, size: Vec3) -> KResult<ShapeId> {
            if size.x > 0.0 {
                Ok(ShapeId(7))
            } else {
                Err(KernelError::InvalidInput("size".into()))
            }
        }
        fn boolean(&self, _: BoolOp, _: ShapeId, _: ShapeId) -> KResult<OpResult> {
            Err(KernelError::Unsupported("boolean"))
        }
        fn face_count(&self, shape: ShapeId) -> KResult<usize> {
            if shape == ShapeId(7) {
                Ok(6)
            } else {
                Err(KernelError::UnknownShape(shape))
            }
        }
        fn tessellate(&self, _: ShapeId, _: Tolerance) -> KResult<Mesh> {
            Ok(Mesh::default())
        }
        fn mass_props(&self, _: ShapeId) -> KResult<MassProps> {
            Ok(MassProps {
                volume: 1.0,
                area: 6.0,
                center: Vec3::ZERO,
            })
        }
        fn release(&self, _: ShapeId) {}
    }

    #[test]
    fn requests_are_answered_in_order_until_the_stream_ends() -> io::Result<()> {
        let requests = [
            Request::MakeBox {
                origin: Vec3::ZERO,
                size: Vec3::X,
            },
            Request::MakeBox {
                origin: Vec3::ZERO,
                size: Vec3::ZERO,
            },
            Request::FaceCount(ShapeId(7)),
            Request::FaceCount(ShapeId(8)),
            Request::Release(ShapeId(7)),
        ];
        let mut input = Vec::new();
        for request in &requests {
            write_frame(&mut input, &request.encode())?;
        }
        let mut output = Vec::new();
        serve(
            &Fake,
            input.as_slice(),
            &mut output,
            ServeOptions::default(),
        )?;

        let mut replies = output.as_slice();
        let mut next = |request: &Request| -> io::Result<Result<Reply, KernelError>> {
            let body = read_frame(&mut replies)?.ok_or(io::ErrorKind::UnexpectedEof)?;
            decode_response(request, &body)
        };
        assert_eq!(next(&requests[0])?, Ok(Reply::Shape(ShapeId(7))));
        assert_eq!(
            next(&requests[1])?,
            Err(KernelError::InvalidInput("size".into()))
        );
        assert_eq!(next(&requests[2])?, Ok(Reply::Count(6)));
        assert_eq!(
            next(&requests[3])?,
            Err(KernelError::UnknownShape(ShapeId(8)))
        );
        assert_eq!(next(&requests[4])?, Ok(Reply::Done));
        assert_eq!(read_frame(&mut replies)?, None);
        Ok(())
    }

    #[test]
    fn a_corrupt_request_ends_the_session() -> io::Result<()> {
        let mut input = Vec::new();
        write_frame(&mut input, &[250])?;
        write_frame(&mut input, &Request::FaceCount(ShapeId(7)).encode())?;
        let mut output = Vec::new();
        let outcome = serve(
            &Fake,
            input.as_slice(),
            &mut output,
            ServeOptions::default(),
        );
        assert!(outcome.is_err());
        assert!(
            output.is_empty(),
            "nothing is answered after a corrupt request"
        );
        Ok(())
    }
}
