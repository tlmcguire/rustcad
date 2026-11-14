//! A 5 x 7 pixel font for labels and stats overlays in example images. No dependencies.
#![allow(dead_code)]

use cad_render::Image;

/// Rows of each glyph, top to bottom; the five low bits of each row, left to right.
fn glyph(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [7, 2, 2, 2, 2, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 17, 25, 21, 19, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 10, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [31, 2, 4, 2, 1, 17, 14],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 30, 1, 1, 17, 14],
        '6' => [6, 8, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 2, 12],
        '.' => [0, 0, 0, 0, 0, 12, 12],
        ',' => [0, 0, 0, 0, 12, 4, 8],
        ':' => [0, 12, 12, 0, 12, 12, 0],
        '/' => [1, 2, 2, 4, 8, 8, 16],
        '%' => [24, 25, 2, 4, 8, 19, 3],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '+' => [0, 4, 4, 31, 4, 4, 0],
        '(' => [2, 4, 8, 8, 8, 4, 2],
        ')' => [8, 4, 2, 2, 2, 4, 8],
        '=' => [0, 0, 31, 0, 31, 0, 0],
        '>' => [8, 4, 2, 1, 2, 4, 8],
        '<' => [2, 4, 8, 16, 8, 4, 2],
        '_' => [0, 0, 0, 0, 0, 0, 31],
        '#' => [10, 10, 31, 10, 31, 10, 10],
        '\'' => [4, 4, 8, 0, 0, 0, 0],
        _ => [0; 7],
    }
}

/// Width in pixels of `text` at `scale`.
pub fn width(text: &str, scale: u32) -> u32 {
    text.chars().count() as u32 * 6 * scale
}

/// Height in pixels of a line at `scale`.
pub fn height(scale: u32) -> u32 {
    7 * scale
}

fn put(image: &mut Image, x: u32, y: u32, color: [u8; 4]) {
    if x < image.width && y < image.height {
        let k = (y as usize * image.width as usize + x as usize) * 4;
        image.rgba[k..k + 4].copy_from_slice(&color);
    }
}

/// Draws `text` with its top left corner at `(x, y)`, with a one-pixel dark shadow.
pub fn draw(image: &mut Image, x: u32, y: u32, scale: u32, color: [u8; 4], text: &str) {
    for (pass, ink, offset) in [(0, [0, 0, 0, 255], scale.max(1)), (1, color, 0)] {
        let _ = pass;
        for (i, c) in text.chars().enumerate() {
            let left = x + i as u32 * 6 * scale + offset;
            for (row, bits) in glyph(c).iter().enumerate() {
                for col in 0..5u32 {
                    if bits & (16 >> col) == 0 {
                        continue;
                    }
                    for dy in 0..scale {
                        for dx in 0..scale {
                            put(
                                image,
                                left + col * scale + dx,
                                y + offset + row as u32 * scale + dy,
                                ink,
                            );
                        }
                    }
                }
            }
        }
    }
}

/// Darkens a rectangle, for text to sit on.
pub fn shade(image: &mut Image, x: u32, y: u32, w: u32, h: u32, alpha: f32) {
    for yy in y..(y + h).min(image.height) {
        for xx in x..(x + w).min(image.width) {
            let k = (yy as usize * image.width as usize + xx as usize) * 4;
            for c in &mut image.rgba[k..k + 3] {
                *c = (f32::from(*c) * (1.0 - alpha)) as u8;
            }
        }
    }
}

/// A label in the style of engine view-mode captions: white capitals on a dark box.
pub fn label(image: &mut Image, x: u32, y: u32, text: &str) {
    let scale = 2;
    shade(
        image,
        x,
        y,
        width(text, scale) + 10,
        height(scale) + 10,
        0.7,
    );
    draw(image, x + 5, y + 5, scale, [240, 240, 240, 255], text);
}

/// Groups digits in threes: 16000000 becomes 16,000,000.
pub fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}
