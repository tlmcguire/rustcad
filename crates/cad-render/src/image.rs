use std::path::Path;

use cad_kernel::FaceIndex;

use crate::RenderError;

/// An 8-bit sRGB image with alpha, rows top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes in RGBA order.
    pub rgba: Vec<u8>,
}

impl Image {
    /// The colour at a pixel, or `None` outside the image. The origin is the top left corner.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        let start = index(self.width, self.height, x, y)?.checked_mul(4)?;
        let bytes = self.rgba.get(start..start.checked_add(4)?)?;
        bytes.try_into().ok()
    }

    /// Encodes the image as a PNG file in memory.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Encode`] when the pixel data does not match the dimensions.
    pub fn to_png(&self) -> Result<Vec<u8>, RenderError> {
        let encode = |e: png::EncodingError| RenderError::Encode(e.to_string());
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(encode)?;
        writer.write_image_data(&self.rgba).map_err(encode)?;
        writer.finish().map_err(encode)?;
        Ok(out)
    }

    /// Decodes a PNG file held in memory.
    ///
    /// Only what [`Image::to_png`] writes is accepted: 8-bit RGBA. Other PNGs are rejected
    /// rather than converted, so a decoded image always holds the bytes that were stored.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Decode`] when the data is not such a PNG.
    pub fn from_png(data: &[u8]) -> Result<Self, RenderError> {
        let decode = |e: png::DecodingError| RenderError::Decode(e.to_string());
        let mut reader = png::Decoder::new(std::io::Cursor::new(data))
            .read_info()
            .map_err(decode)?;
        let format = reader.output_color_type();
        if format != (png::ColorType::Rgba, png::BitDepth::Eight) {
            return Err(RenderError::Decode(format!(
                "expected 8-bit RGBA, found {:?} at {:?} bits",
                format.0, format.1
            )));
        }
        let size = reader
            .output_buffer_size()
            .ok_or_else(|| RenderError::Decode("image is too large".into()))?;
        let mut rgba = vec![0; size];
        let info = reader.next_frame(&mut rgba).map_err(decode)?;
        rgba.truncate(info.buffer_size());
        Ok(Self {
            width: info.width,
            height: info.height,
            rgba,
        })
    }

    /// Reads a PNG file.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Io`] when the file cannot be read, otherwise as
    /// [`Image::from_png`].
    pub fn read_png(path: impl AsRef<Path>) -> Result<Self, RenderError> {
        let path = path.as_ref();
        let data =
            std::fs::read(path).map_err(|e| RenderError::Io(format!("{}: {e}", path.display())))?;
        Self::from_png(&data)
    }

    /// Writes the image to a PNG file.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Encode`] or [`RenderError::Io`].
    pub fn write_png(&self, path: impl AsRef<Path>) -> Result<(), RenderError> {
        let path = path.as_ref();
        std::fs::write(path, self.to_png()?)
            .map_err(|e| RenderError::Io(format!("{}: {e}", path.display())))
    }
}

/// The result of an offscreen render: the picture and the face under every pixel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The shaded picture.
    pub image: Image,
    /// One entry per pixel, rows top to bottom: face index plus one, or zero for background.
    pub ids: Vec<u32>,
}

impl Frame {
    /// The face visible at a pixel, or `None` for background and for pixels outside the frame.
    /// The origin is the top left corner.
    #[must_use]
    pub fn pick(&self, x: u32, y: u32) -> Option<FaceIndex> {
        let id = *self
            .ids
            .get(index(self.image.width, self.image.height, x, y)?)?;
        decode_id(id)
    }
}

/// Converts an ID buffer value to a face.
pub(crate) fn decode_id(id: u32) -> Option<FaceIndex> {
    id.checked_sub(1).map(FaceIndex)
}

fn index(width: u32, height: u32, x: u32, y: u32) -> Option<usize> {
    if x >= width || y >= height {
        return None;
    }
    let (x, y, width) = (
        usize::try_from(x).ok()?,
        usize::try_from(y).ok()?,
        usize::try_from(width).ok()?,
    );
    y.checked_mul(width)?.checked_add(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn sample() -> Image {
        let (width, height) = (5u32, 3u32);
        let rgba = (0..width * height * 4)
            .map(|i| u8::try_from(i * 7 % 256).unwrap_or(0))
            .collect();
        Image {
            width,
            height,
            rgba,
        }
    }

    #[test]
    fn png_round_trip_is_exact() -> TestResult {
        let image = sample();
        assert_eq!(Image::from_png(&image.to_png()?)?, image);
        Ok(())
    }

    #[test]
    fn data_that_is_not_a_png_is_rejected() -> TestResult {
        let png = sample().to_png()?;
        for bad in [&b""[..], &b"not a png"[..], &png[..png.len() / 2]] {
            assert!(matches!(Image::from_png(bad), Err(RenderError::Decode(_))));
        }
        Ok(())
    }

    #[test]
    fn a_png_that_is_not_rgba_is_rejected() -> TestResult {
        let mut data = Vec::new();
        let mut encoder = png::Encoder::new(&mut data, 2, 2);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&[0, 64, 128, 255])?;
        writer.finish()?;
        match Image::from_png(&data) {
            Err(RenderError::Decode(message)) => assert!(message.contains("RGBA"), "{message}"),
            other => return Err(format!("expected a decode error, got {other:?}").into()),
        }
        Ok(())
    }

    #[test]
    fn a_missing_file_is_an_io_error() {
        let missing = std::env::temp_dir().join("cad-render-no-such-file.png");
        assert!(matches!(Image::read_png(missing), Err(RenderError::Io(_))));
    }
}
