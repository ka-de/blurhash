use std::cmp::Ordering;
use std::f64::consts::PI;
use thiserror::*;


#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum EncodingError {
    #[error("cannot encode this number of components")]
    ComponentsNumberInvalid,
    #[error("the bytes per pixel does not match the pixel count")]
    BytesPerPixelMismatch,
}

fn sign_pow(value: f64, exp: f64) -> f64 {
    value.abs().powf(exp).copysign(value)
}

fn linear_to_srgb(value: f64) -> usize {
    let v = value.clamp(0f64, 1f64);
    if v <= 0.003_130_8 {
        (v * 12.92 * 255f64 + 0.5) as usize
    } else {
        ((1.055 * f64::powf(v, 1f64 / 2.4) - 0.055) * 255f64 + 0.5) as usize
    }
}

fn srgb_to_linear(value: usize) -> f64 {
    let v = (value as f64) / 255f64;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

// Encode

// TODO: Think about argument order here...
// What is more common in Rust? Data or config first?
pub fn encode(
    pixels: Vec<u8>,
    cx: usize,
    cy: usize,
    width: usize,
    height: usize,
) -> Result<String, EncodingError> {
    // Should we assume RGBA for round-trips? Or does it not matter?
    let bytes_per_row = width * 4;
    let bytes_per_pixel = 4;

    // NOTE: We could clamp instead of Err.
    // The TS version does that. Not sure which one is better.
    // We also could (should?) be checking for the color space
    if !(1..=9).contains(&cx) || !(1..=9).contains(&cy) {
        return Err(EncodingError::ComponentsNumberInvalid);
    }

    if width * height * 4 != pixels.len() {
        return Err(EncodingError::BytesPerPixelMismatch);
    }

    let mut dc: [f64; 3] = [0., 0., 0.];
    let mut ac: Vec<[f64; 3]> = Vec::with_capacity(cy * cx - 1);

    for y in 0..cy {
        for x in 0..cx {
            let normalisation = if x == 0 && y == 0 { 1f64 } else { 2f64 };
            let factor = multiply_basis_function(
                &pixels,
                width,
                height,
                bytes_per_row,
                bytes_per_pixel,
                0,
                |a, b| {
                    normalisation
                        * f64::cos((PI * x as f64 * a) / width as f64)
                        * f64::cos((PI * y as f64 * b) / height as f64)
                },
            );

            if x == 0 && y == 0 {
                // The first iteration is the dc
                dc = factor;
            } else {
                // All others are the ac
                ac.push(factor);
            }
        }
    }

    let mut hash = String::new();

    let size_flag = (cx - 1) + (cy - 1) * 9;
    hash += &encode_base83_string(size_flag, 1);

    let maximum_value: f64;

    if !ac.is_empty() {
        // I'm sure there's a better way to write this; following the Swift atm :)
        let actual_maximum_value = ac
            .clone()
            .into_iter()
            .map(|[a, b, c]| f64::max(f64::max(f64::abs(a), f64::abs(b)), f64::abs(c)))
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal))
            .unwrap();
        let quantised_maximum_value = usize::max(
            0,
            usize::min(82, f64::floor(actual_maximum_value * 166f64 - 0.5) as usize),
        );
        maximum_value = ((quantised_maximum_value + 1) as f64) / 166f64;
        hash += &encode_base83_string(quantised_maximum_value, 1);
    } else {
        maximum_value = 1f64;
        hash += &encode_base83_string(0, 1);
    }

    hash += &encode_base83_string(encode_dc(dc), 4);

    for factor in ac {
        hash += &encode_base83_string(encode_ac(factor, maximum_value), 2);
    }

    Ok(hash)
}

fn multiply_basis_function<F>(
    pixels: &[u8],
    width: usize,
    height: usize,
    bytes_per_row: usize,
    bytes_per_pixel: usize,
    pixel_offset: usize,
    basis_function: F,
) -> [f64; 3]
where
    F: Fn(f64, f64) -> f64,
{
    let mut r = 0f64;
    let mut g = 0f64;
    let mut b = 0f64;

    for x in 0..width {
        for y in 0..height {
            let basis = basis_function(x as f64, y as f64);
            r += basis
                * srgb_to_linear(
                    usize::from(pixels[bytes_per_pixel * x + pixel_offset + y * bytes_per_row]),
                );
            g += basis
                * srgb_to_linear(
                    usize::from(
                        pixels[bytes_per_pixel * x + pixel_offset + 1 + y * bytes_per_row],
                    ),
                );
            b += basis
                * srgb_to_linear(
                    usize::from(
                        pixels[bytes_per_pixel * x + pixel_offset + 2 + y * bytes_per_row],
                    ),
                );
        }
    }

    let scale = 1f64 / ((width * height) as f64);

    [r * scale, g * scale, b * scale]
}

fn encode_dc(value: [f64; 3]) -> usize {
    let rounded_r = linear_to_srgb(value[0]);
    let rounded_g = linear_to_srgb(value[1]);
    let rounded_b = linear_to_srgb(value[2]);
    (rounded_r << 16) + (rounded_g << 8) + rounded_b
}

fn encode_ac(value: [f64; 3], maximum_value: f64) -> usize {
    let quant_r = f64::floor(sign_pow(value[0] / maximum_value, 0.5) * 9f64 + 9.5).clamp(0f64, 18f64);
    let quant_g = f64::floor(sign_pow(value[1] / maximum_value, 0.5) * 9f64 + 9.5).clamp(0f64, 18f64);
    let quant_b = f64::floor(sign_pow(value[2] / maximum_value, 0.5) * 9f64 + 9.5).clamp(0f64, 18f64);

    (quant_r * 19f64 * 19f64 + quant_g * 19f64 + quant_b) as usize
}

// Base83

// I considered using lazy_static! for this, but other implementations
// seem to hard-code these as well. Doing that for consistency.
static ENCODE_CHARACTERS: [char; 83] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I',
    'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', 'a', 'b',
    'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u',
    'v', 'w', 'x', 'y', 'z', '#', '$', '%', '*', '+', ',', '-', '.', ':', ';', '=', '?', '@', '[',
    ']', '^', '_', '{', '|', '}', '~',
];


fn encode_base83_string(value: usize, length: u32) -> String {
    (1..=length)
        .map(|i| (value / usize::pow(83, length - i)) % 83)
        .map(|digit| ENCODE_CHARACTERS[digit])
        .collect()
}
