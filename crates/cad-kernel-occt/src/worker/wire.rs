//! The binary protocol between a kernel client and a worker process.
//!
//! Every message is a frame: a little-endian `u32` length, then that many bytes. A request
//! body is one opcode byte and its arguments. A response body is a status byte (0 ok, 1 error)
//! and the result or the error. All numbers are little-endian. There is no versioning yet: the
//! client and worker are built together.

use std::io::{self, Read, Write};

use cad_kernel::{
    BoolOp, FaceFate, FaceHistory, FaceIndex, HistoryMap, KernelError, MassProps, Mesh, ShapeId,
};
use cad_math::Vec3;

/// Largest frame either side will accept. A length above this means the stream is corrupt.
const MAX_FRAME: u32 = 1 << 30;

/// One kernel call.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Request {
    MakeBox {
        origin: Vec3,
        size: Vec3,
    },
    Boolean {
        op: BoolOp,
        a: ShapeId,
        b: ShapeId,
    },
    FaceCount(ShapeId),
    Tessellate {
        shape: ShapeId,
        linear: f64,
        angular: f64,
    },
    MassProps(ShapeId),
    Release(ShapeId),
    ToBrep(ShapeId),
    FromBrep(Vec<u8>),
    ExportStep(ShapeId),
    ImportStep(Vec<u8>),
}

/// The successful result of a call.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Reply {
    Shape(ShapeId),
    /// History entries name their source as operand 0 or 1, not by shape handle.
    Boolean {
        shape: ShapeId,
        history: HistoryMap,
    },
    Count(u64),
    Mesh(Mesh),
    MassProps(MassProps),
    Bytes(Vec<u8>),
    Done,
}

fn corrupt(what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("corrupt kernel message: {what}"),
    )
}

/// Writes one frame and flushes.
pub(crate) fn write_frame(out: &mut impl Write, body: &[u8]) -> io::Result<()> {
    let len = u32::try_from(body.len())
        .ok()
        .filter(|len| *len <= MAX_FRAME);
    let len = len.ok_or_else(|| corrupt("frame too large"))?;
    out.write_all(&len.to_le_bytes())?;
    out.write_all(body)?;
    out.flush()
}

/// Reads one frame. `Ok(None)` means the stream ended cleanly between frames.
pub(crate) fn read_frame(input: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    let mut filled = 0;
    while filled < len.len() {
        match input.read(&mut len[filled..]) {
            Ok(0) if filled == 0 => return Ok(None),
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    let len = u32::from_le_bytes(len);
    if len > MAX_FRAME {
        return Err(corrupt("frame too large"));
    }
    let mut body = vec![0u8; len as usize];
    input.read_exact(&mut body)?;
    Ok(Some(body))
}

/// Appends values to a message.
#[derive(Default)]
struct Encoder(Vec<u8>);

impl Encoder {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn vec3(&mut self, v: Vec3) {
        self.f64(v.x);
        self.f64(v.y);
        self.f64(v.z);
    }
    fn len(&mut self, n: usize) {
        // Lengths beyond u32 cannot fit in a frame anyway; `write_frame` rejects those.
        self.u32(u32::try_from(n).unwrap_or(u32::MAX));
    }
    fn text(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
    fn bytes(&mut self, b: &[u8]) {
        self.len(b.len());
        self.0.extend_from_slice(b);
    }
}

/// Reads values from a message.
struct Decoder<'a>(&'a [u8]);

impl Decoder<'_> {
    fn take<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        let (head, rest) = self
            .0
            .split_first_chunk::<N>()
            .ok_or_else(|| corrupt("too short"))?;
        self.0 = rest;
        Ok(*head)
    }
    fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take::<1>()?[0])
    }
    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take()?))
    }
    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.take()?))
    }
    fn f64(&mut self) -> io::Result<f64> {
        Ok(f64::from_le_bytes(self.take()?))
    }
    fn vec3(&mut self) -> io::Result<Vec3> {
        Ok(Vec3::new(self.f64()?, self.f64()?, self.f64()?))
    }
    /// A count of items that each take at least `item_bytes`, checked against what is left so
    /// a corrupt count cannot ask for a huge allocation.
    fn len(&mut self, item_bytes: usize) -> io::Result<usize> {
        let n = self.u32()? as usize;
        match n.checked_mul(item_bytes) {
            Some(bytes) if bytes <= self.0.len() => Ok(n),
            _ => Err(corrupt("count exceeds message")),
        }
    }
    fn text(&mut self) -> io::Result<String> {
        String::from_utf8(self.bytes()?).map_err(|_| corrupt("text is not UTF-8"))
    }
    fn bytes(&mut self) -> io::Result<Vec<u8>> {
        let n = self.len(1)?;
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head.to_vec())
    }
    fn list<T>(
        &mut self,
        item_bytes: usize,
        mut item: impl FnMut(&mut Self) -> io::Result<T>,
    ) -> io::Result<Vec<T>> {
        let n = self.len(item_bytes)?;
        (0..n).map(|_| item(self)).collect()
    }
    fn end(&self) -> io::Result<()> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(corrupt("trailing bytes"))
        }
    }
}

impl Request {
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut e = Encoder::default();
        match self {
            Self::MakeBox { origin, size } => {
                e.u8(1);
                e.vec3(*origin);
                e.vec3(*size);
            }
            Self::Boolean { op, a, b } => {
                e.u8(2);
                e.u8(match op {
                    BoolOp::Union => 0,
                    BoolOp::Subtract => 1,
                    BoolOp::Intersect => 2,
                });
                e.u64(a.0);
                e.u64(b.0);
            }
            Self::FaceCount(shape) => {
                e.u8(3);
                e.u64(shape.0);
            }
            Self::Tessellate {
                shape,
                linear,
                angular,
            } => {
                e.u8(4);
                e.u64(shape.0);
                e.f64(*linear);
                e.f64(*angular);
            }
            Self::MassProps(shape) => {
                e.u8(5);
                e.u64(shape.0);
            }
            Self::Release(shape) => {
                e.u8(6);
                e.u64(shape.0);
            }
            Self::ToBrep(shape) => {
                e.u8(7);
                e.u64(shape.0);
            }
            Self::FromBrep(bytes) => {
                e.u8(8);
                e.bytes(bytes);
            }
            Self::ExportStep(shape) => {
                e.u8(9);
                e.u64(shape.0);
            }
            Self::ImportStep(bytes) => {
                e.u8(10);
                e.bytes(bytes);
            }
        }
        e.0
    }

    pub(crate) fn decode(body: &[u8]) -> io::Result<Self> {
        let mut d = Decoder(body);
        let request = match d.u8()? {
            1 => Self::MakeBox {
                origin: d.vec3()?,
                size: d.vec3()?,
            },
            2 => {
                let op = match d.u8()? {
                    0 => BoolOp::Union,
                    1 => BoolOp::Subtract,
                    2 => BoolOp::Intersect,
                    _ => return Err(corrupt("unknown boolean operation")),
                };
                Self::Boolean {
                    op,
                    a: ShapeId(d.u64()?),
                    b: ShapeId(d.u64()?),
                }
            }
            3 => Self::FaceCount(ShapeId(d.u64()?)),
            4 => Self::Tessellate {
                shape: ShapeId(d.u64()?),
                linear: d.f64()?,
                angular: d.f64()?,
            },
            5 => Self::MassProps(ShapeId(d.u64()?)),
            6 => Self::Release(ShapeId(d.u64()?)),
            7 => Self::ToBrep(ShapeId(d.u64()?)),
            8 => Self::FromBrep(d.bytes()?),
            9 => Self::ExportStep(ShapeId(d.u64()?)),
            10 => Self::ImportStep(d.bytes()?),
            _ => return Err(corrupt("unknown request")),
        };
        d.end()?;
        Ok(request)
    }
}

/// Encodes the outcome of a call as a response body.
pub(crate) fn encode_response(result: &Result<Reply, KernelError>) -> Vec<u8> {
    let mut e = Encoder::default();
    match result {
        Ok(reply) => {
            e.u8(0);
            match reply {
                Reply::Shape(shape) => e.u64(shape.0),
                Reply::Boolean { shape, history } => {
                    e.u64(shape.0);
                    e.len(history.faces.len());
                    for entry in &history.faces {
                        e.u64(entry.source_shape.0);
                        e.u32(entry.source_face.0);
                        e.u8(match entry.fate {
                            FaceFate::Modified => 0,
                            FaceFate::Split => 1,
                            FaceFate::Deleted => 2,
                        });
                        e.len(entry.result_faces.len());
                        entry.result_faces.iter().for_each(|f| e.u32(f.0));
                    }
                    e.len(history.generated_faces.len());
                    history.generated_faces.iter().for_each(|f| e.u32(f.0));
                }
                Reply::Count(n) => e.u64(*n),
                Reply::Mesh(mesh) => {
                    e.0.reserve(mesh.positions.len() * 48 + mesh.triangles.len() * 16 + 16);
                    e.len(mesh.positions.len());
                    mesh.positions.iter().for_each(|p| e.vec3(*p));
                    e.len(mesh.normals.len());
                    mesh.normals.iter().for_each(|n| e.vec3(*n));
                    e.len(mesh.triangles.len());
                    mesh.triangles.iter().flatten().for_each(|i| e.u32(*i));
                    e.len(mesh.triangle_faces.len());
                    mesh.triangle_faces.iter().for_each(|f| e.u32(f.0));
                }
                Reply::MassProps(props) => {
                    e.f64(props.volume);
                    e.f64(props.area);
                    e.vec3(props.center);
                }
                Reply::Bytes(bytes) => e.bytes(bytes),
                Reply::Done => {}
            }
        }
        Err(error) => {
            e.u8(1);
            let (kind, id, text) = match error {
                KernelError::InvalidInput(m) => (0, 0, m.as_str()),
                KernelError::UnknownShape(id) => (1, id.0, ""),
                KernelError::OperationFailed(m) => (2, 0, m.as_str()),
                KernelError::BackendLost(m) => (3, 0, m.as_str()),
                KernelError::Unsupported(op) => (4, 0, *op),
            };
            e.u8(kind);
            e.u64(id);
            e.text(text);
        }
    }
    e.0
}

/// Decodes a response body. `request` says which kind of reply to expect.
pub(crate) fn decode_response(
    request: &Request,
    body: &[u8],
) -> io::Result<Result<Reply, KernelError>> {
    let mut d = Decoder(body);
    let outcome = match d.u8()? {
        0 => Ok(match request {
            Request::MakeBox { .. } | Request::FromBrep(_) | Request::ImportStep(_) => {
                Reply::Shape(ShapeId(d.u64()?))
            }
            Request::ToBrep(_) | Request::ExportStep(_) => Reply::Bytes(d.bytes()?),
            Request::Boolean { .. } => {
                let shape = ShapeId(d.u64()?);
                let faces = d.list(17, |d| {
                    Ok(FaceHistory {
                        source_shape: ShapeId(d.u64()?),
                        source_face: FaceIndex(d.u32()?),
                        fate: match d.u8()? {
                            0 => FaceFate::Modified,
                            1 => FaceFate::Split,
                            2 => FaceFate::Deleted,
                            _ => return Err(corrupt("unknown face fate")),
                        },
                        result_faces: d.list(4, |d| d.u32().map(FaceIndex))?,
                    })
                })?;
                let generated_faces = d.list(4, |d| d.u32().map(FaceIndex))?;
                Reply::Boolean {
                    shape,
                    history: HistoryMap {
                        faces,
                        generated_faces,
                    },
                }
            }
            Request::FaceCount(_) => Reply::Count(d.u64()?),
            Request::Tessellate { .. } => Reply::Mesh(Mesh {
                positions: d.list(24, Decoder::vec3)?,
                normals: d.list(24, Decoder::vec3)?,
                triangles: d.list(12, |d| Ok([d.u32()?, d.u32()?, d.u32()?]))?,
                triangle_faces: d.list(4, |d| d.u32().map(FaceIndex))?,
            }),
            Request::MassProps(_) => Reply::MassProps(MassProps {
                volume: d.f64()?,
                area: d.f64()?,
                center: d.vec3()?,
            }),
            Request::Release(_) => Reply::Done,
        }),
        1 => {
            let (kind, id, text) = (d.u8()?, d.u64()?, d.text()?);
            Err(match kind {
                0 => KernelError::InvalidInput(text),
                1 => KernelError::UnknownShape(ShapeId(id)),
                2 => KernelError::OperationFailed(text),
                3 => KernelError::BackendLost(text),
                // The operation name cannot cross the process boundary as a static string.
                4 => KernelError::Unsupported("an operation the worker's backend lacks"),
                _ => return Err(corrupt("unknown error kind")),
            })
        }
        _ => return Err(corrupt("unknown status")),
    };
    d.end()?;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(request: Request, result: Result<Reply, KernelError>) {
        assert_eq!(
            Request::decode(&request.encode()).ok(),
            Some(request.clone())
        );
        let decoded = decode_response(&request, &encode_response(&result));
        assert_eq!(decoded.ok(), Some(result));
    }

    #[test]
    fn every_message_survives_a_round_trip() {
        let s = ShapeId;
        let v = Vec3::new(1.5, -2.0, 3.25);
        round_trip(
            Request::MakeBox { origin: v, size: v },
            Ok(Reply::Shape(s(7))),
        );
        round_trip(Request::FaceCount(s(1)), Ok(Reply::Count(11)));
        round_trip(Request::Release(s(u64::MAX)), Ok(Reply::Done));
        round_trip(
            Request::MassProps(s(2)),
            Ok(Reply::MassProps(MassProps {
                volume: 8000.0,
                area: 2800.0,
                center: v,
            })),
        );
        let history = HistoryMap {
            faces: vec![
                FaceHistory {
                    source_shape: s(0),
                    source_face: FaceIndex(3),
                    fate: FaceFate::Split,
                    result_faces: vec![FaceIndex(1), FaceIndex(4)],
                },
                FaceHistory {
                    source_shape: s(1),
                    source_face: FaceIndex(0),
                    fate: FaceFate::Deleted,
                    result_faces: Vec::new(),
                },
            ],
            generated_faces: vec![FaceIndex(9)],
        };
        for op in [BoolOp::Union, BoolOp::Subtract, BoolOp::Intersect] {
            let reply = Reply::Boolean {
                shape: s(5),
                history: history.clone(),
            };
            round_trip(
                Request::Boolean {
                    op,
                    a: s(3),
                    b: s(4),
                },
                Ok(reply),
            );
        }
        let mesh = Mesh {
            positions: vec![v, Vec3::ZERO, Vec3::X],
            normals: vec![Vec3::Z; 3],
            triangles: vec![[0, 1, 2]],
            triangle_faces: vec![FaceIndex(6)],
        };
        let tessellate = Request::Tessellate {
            shape: s(1),
            linear: 1e-6,
            angular: 0.5,
        };
        round_trip(tessellate.clone(), Ok(Reply::Mesh(mesh)));
        round_trip(tessellate, Ok(Reply::Mesh(Mesh::default())));
    }

    #[test]
    fn exchange_messages_survive_a_round_trip() {
        let s = ShapeId;
        let data: Vec<u8> = (0..=255).collect();
        round_trip(Request::ToBrep(s(3)), Ok(Reply::Bytes(data.clone())));
        round_trip(Request::ExportStep(s(4)), Ok(Reply::Bytes(Vec::new())));
        round_trip(Request::FromBrep(data.clone()), Ok(Reply::Shape(s(5))));
        round_trip(Request::ImportStep(data), Ok(Reply::Shape(s(6))));
        round_trip(Request::ImportStep(Vec::new()), Ok(Reply::Shape(s(7))));

        // A byte count larger than the message is corrupt, not a huge allocation.
        assert!(Request::decode(&[8, 0xFF, 0xFF, 0xFF, 0x7F, 1]).is_err());
        let lying = [0u8, 0xFF, 0xFF, 0xFF, 0x7F];
        assert!(decode_response(&Request::ToBrep(s(1)), &lying).is_err());
    }

    #[test]
    fn errors_survive_a_round_trip() {
        let request = Request::FaceCount(ShapeId(1));
        for error in [
            KernelError::InvalidInput("bad size".into()),
            KernelError::UnknownShape(ShapeId(42)),
            KernelError::OperationFailed("boolean failed".into()),
            KernelError::BackendLost("gone".into()),
        ] {
            round_trip(request.clone(), Err(error));
        }
        let unsupported = encode_response(&Err(KernelError::Unsupported("boolean")));
        let decoded = decode_response(&request, &unsupported).ok();
        assert!(matches!(decoded, Some(Err(KernelError::Unsupported(_)))));
    }

    #[test]
    fn corrupt_messages_are_rejected_not_trusted() {
        assert!(Request::decode(&[]).is_err());
        assert!(Request::decode(&[99]).is_err());
        assert!(Request::decode(&[3, 1, 2]).is_err(), "truncated handle");
        let mut long = Request::FaceCount(ShapeId(1)).encode();
        long.push(0);
        assert!(Request::decode(&long).is_err(), "trailing byte");

        let request = Request::Tessellate {
            shape: ShapeId(1),
            linear: 1e-3,
            angular: 0.5,
        };
        // Status ok, then a vertex count of four billion with no data behind it.
        let lying = [0u8, 0xFF, 0xFF, 0xFF, 0xFF];
        assert!(decode_response(&request, &lying).is_err());
        assert!(decode_response(&request, &[7]).is_err(), "unknown status");
    }

    #[test]
    fn frames_carry_their_length() -> io::Result<()> {
        let mut stream = Vec::new();
        write_frame(&mut stream, b"hello")?;
        write_frame(&mut stream, b"")?;
        let mut input = stream.as_slice();
        assert_eq!(read_frame(&mut input)?.as_deref(), Some(&b"hello"[..]));
        assert_eq!(read_frame(&mut input)?.as_deref(), Some(&b""[..]));
        assert_eq!(read_frame(&mut input)?, None, "clean end of stream");

        // A stream that ends inside a frame is an error, not a clean end.
        let mut cut = &stream[..6];
        assert!(read_frame(&mut cut).is_err());
        let mut cut = &stream[..2];
        assert!(read_frame(&mut cut).is_err());
        let mut huge = &[0xFF, 0xFF, 0xFF, 0xFF][..];
        assert!(read_frame(&mut huge).is_err());
        Ok(())
    }
}
