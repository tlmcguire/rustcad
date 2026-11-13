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
