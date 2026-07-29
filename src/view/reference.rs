use super::{
    category_color, scalar_color, DisplayPrepareError, LinearRgba, PreparedFieldDisplay,
    PreparedFieldKind, DIAGNOSTIC_ERROR_COLOR, DIAGNOSTIC_INFO_COLOR, DIAGNOSTIC_WARNING_COLOR,
};

/// Deterministic RGBA8 output produced without a graphics API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceImage {
    width: u32,
    height: u32,
    rgba8: Vec<u8>,
}

impl ReferenceImage {
    /// Returns the image width in pixels.
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Returns the image height in pixels.
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Borrows tightly packed, top-to-bottom RGBA8 pixels.
    pub fn rgba8(&self) -> &[u8] {
        &self.rgba8
    }
}

/// Rasterizes one validated display packet with a deterministic CPU reference path.
pub fn rasterize_reference(
    packet: &PreparedFieldDisplay,
    width: u32,
    height: u32,
) -> Result<ReferenceImage, DisplayPrepareError> {
    let byte_len = checked_image_len(width, height)?;
    let mut rgba8 = Vec::new();
    rgba8
        .try_reserve_exact(byte_len)
        .map_err(|_| DisplayPrepareError::ReferenceImageAllocationFailed { bytes: byte_len })?;
    rgba8.resize(byte_len, 0);

    let vertices = packet.mesh().vertices();
    let indices = packet.mesh().indices();
    for pixel_y in 0..height {
        for pixel_x in 0..width {
            let point = [
                (pixel_x as f64 + 0.5) / f64::from(width),
                (pixel_y as f64 + 0.5) / f64::from(height),
            ];
            let cell = indices.chunks_exact(3).find_map(|triangle| {
                let a = vertices[triangle[0] as usize];
                let b = vertices[triangle[1] as usize];
                let c = vertices[triangle[2] as usize];
                triangle_contains(
                    point,
                    to_f64(a.position),
                    to_f64(b.position),
                    to_f64(c.position),
                )
                .then_some(a.cell)
            });
            let Some(cell) = cell else {
                continue;
            };
            let color = cell_color(packet, cell as usize);
            let offset = (pixel_y as usize * width as usize + pixel_x as usize) * 4;
            rgba8[offset..offset + 4].copy_from_slice(&linear_to_srgba8(color));
        }
    }

    Ok(ReferenceImage {
        width,
        height,
        rgba8,
    })
}

fn checked_image_len(width: u32, height: u32) -> Result<usize, DisplayPrepareError> {
    if width == 0 || height == 0 {
        return Err(DisplayPrepareError::InvalidReferenceImageDimensions { width, height });
    }
    usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(DisplayPrepareError::IntegerOverflow {
            context: "reference image byte count",
        })
}

fn cell_color(packet: &PreparedFieldDisplay, cell: usize) -> LinearRgba {
    if packet.diagnostics_enabled() {
        match packet.diagnostics().cells()[cell] {
            1 => return DIAGNOSTIC_INFO_COLOR,
            2 => return DIAGNOSTIC_WARNING_COLOR,
            severity if severity >= 3 => return DIAGNOSTIC_ERROR_COLOR,
            _ => {}
        }
    }

    let raw = packet.field().raw_values()[cell];
    match packet.field().kind() {
        PreparedFieldKind::Scalar => scalar_color(
            f32::from_bits(raw),
            packet
                .display_range()
                .expect("validated scalar packets have a display range"),
            packet.palette(),
        ),
        PreparedFieldKind::Category => category_color(raw, packet.palette()),
    }
}

fn triangle_contains(point: [f64; 2], a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> bool {
    [(a, b), (b, c), (c, a)].into_iter().all(|(start, end)| {
        let edge = edge_function(start, end, point);
        edge > 0.0 || (edge == 0.0 && is_top_left(start, end))
    })
}

fn edge_function(a: [f64; 2], b: [f64; 2], point: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0])
}

fn is_top_left(a: [f64; 2], b: [f64; 2]) -> bool {
    let dy = b[1] - a[1];
    dy > 0.0 || (dy == 0.0 && b[0] < a[0])
}

fn to_f64(point: [f32; 2]) -> [f64; 2] {
    [f64::from(point[0]), f64::from(point[1])]
}

fn linear_to_srgba8(color: LinearRgba) -> [u8; 4] {
    let [red, green, blue, alpha] = color.components();
    [
        linear_channel_to_srgb8(red),
        linear_channel_to_srgb8(green),
        linear_channel_to_srgb8(blue),
        unit_to_u8(alpha),
    ]
}

fn linear_channel_to_srgb8(linear: f32) -> u8 {
    let linear = linear.clamp(0.0, 1.0);
    let srgb = if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    unit_to_u8(srgb)
}

fn unit_to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::checked_image_len;
    use crate::view::DisplayPrepareError;

    #[test]
    fn image_length_rejects_zero_dimensions_and_checked_overflow() {
        assert!(matches!(
            checked_image_len(0, 64),
            Err(DisplayPrepareError::InvalidReferenceImageDimensions {
                width: 0,
                height: 64
            })
        ));
        assert!(matches!(
            checked_image_len(u32::MAX, u32::MAX),
            Err(DisplayPrepareError::IntegerOverflow {
                context: "reference image byte count"
            })
        ));
    }

    #[test]
    fn exact_top_left_rule_assigns_shared_edges_once() {
        let point = [0.5, 0.5];
        let first = super::triangle_contains(point, [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]);
        let second = super::triangle_contains(point, [0.0, 0.0], [1.0, 1.0], [0.0, 1.0]);
        assert_ne!(first, second);
        assert!(first || second);
    }
}
