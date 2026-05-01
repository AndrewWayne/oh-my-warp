//! QR rendering helpers for the pair URL.
//!
//! Wired by Wiring 5, Gap 2 (pair-modal foundation). Pure helpers; no UI deps.
//! SVG is preferred over PNG so the modal can scale crisply at any DPI without
//! shipping a raster encoder. The grid form is exposed for native-rendered
//! display backends that don't take SVG.

use qrcode::render::svg;
use qrcode::types::QrError;
use qrcode::{EcLevel, QrCode};

/// Render `url` as an SVG string suitable for inline embedding in a UI.
///
/// Uses `EcLevel::M` (medium error correction, ~15% recovery) which is the
/// pairing-flow norm — enough to survive a phone-camera tilt or partial glare,
/// but not so strict that the matrix gets dense for a long URL.
pub fn render_pair_url_svg(url: &str) -> Result<String, QrError> {
    let code = QrCode::with_error_correction_level(url.as_bytes(), EcLevel::M)?;
    Ok(code
        .render::<svg::Color<'_>>()
        .min_dimensions(200, 200)
        .quiet_zone(true)
        .build())
}

/// Render `url` as a 2D bool grid. Outer Vec is rows, inner Vec is columns;
/// `true` = dark module. Useful for native renderers that paint quads.
pub fn render_pair_url_grid(url: &str) -> Result<Vec<Vec<bool>>, QrError> {
    let code = QrCode::with_error_correction_level(url.as_bytes(), EcLevel::M)?;
    let width = code.width();
    let modules = code.to_colors();
    let mut grid = Vec::with_capacity(width);
    for row in 0..width {
        let mut cols = Vec::with_capacity(width);
        for col in 0..width {
            let module = modules[row * width + col];
            cols.push(module == qrcode::Color::Dark);
        }
        grid.push(cols);
    }
    Ok(grid)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SVG output should be a parsable XML fragment starting with `<?xml` or
    /// `<svg`. The `qrcode` crate emits the latter.
    #[test]
    fn qr_render_basic_url_svg_starts_with_svg_tag() {
        let svg = render_pair_url_svg("https://example.ts.net/pair?t=abc")
            .expect("render must succeed for a short URL");
        assert!(
            svg.contains("<svg"),
            "expected an <svg tag in output, got: {}",
            &svg[..svg.len().min(80)]
        );
    }

    /// The grid is square: every row has the same column count, equal to the
    /// total row count. This is a structural invariant of QR matrices.
    #[test]
    fn qr_render_grid_is_square() {
        let grid = render_pair_url_grid("https://example.ts.net/pair?t=abc")
            .expect("render must succeed");
        let n = grid.len();
        assert!(n > 0, "grid should be non-empty");
        for (i, row) in grid.iter().enumerate() {
            assert_eq!(row.len(), n, "row {i} length {} != grid size {n}", row.len());
        }
    }
}
