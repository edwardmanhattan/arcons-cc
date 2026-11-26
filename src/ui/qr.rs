//! **The pairing QR, drawn as the wall's own squares** (#270).
//!
//! The payload and the module grid are [`crate::ui::pairing`]'s: this module turns a cached matrix into
//! elements and nothing more. A frame draws squares; it does not build a QR (`INV-05`), and a capture never
//! reaches for an encoder or a socket (`INV-11`) — the picture is a function of the digits and the relay's
//! string, both of which are values on the state.
//!
//! Three decisions are this file's, and the tokens they land on are `tokens`':
//!
//! - **Dark on light.** The QR is a light block with dark modules, the standard's own polarity: a scanner
//!   expects it, and a white square is the one thing on a dark wall a camera finds across a room.
//! - **Whole-point modules.** One module is the largest whole number of points that fits the panel's square,
//!   so module edges land on the pixel grid instead of in the middle of a rasteriser's antialiasing.
//! - **The quiet zone is paper, not panel.** The light block is drawn as wide as the modules plus their four
//!   modules of margin, because a quiet zone has to be light to be a quiet zone.
//!
//! One element per **run** of dark modules in a row rather than one per module: an 1 700-module grid as 1 700
//! children is a frame's worth of elements for a picture that is the same pixels either way.

use rune::{El, el};

use crate::tokens;
use crate::ui::pairing::Matrix;

/// One module's side in points: the largest whole number that fits [`tokens::QR_SIDE`], and at least one so a
/// payload that outgrows the square degrades to a dense picture rather than to nothing.
pub fn module(matrix: &Matrix) -> f32 {
    let total = (matrix.size() + tokens::QR_QUIET * 2) as f32;
    (tokens::QR_SIDE / total).floor().max(1.0)
}

/// The quiet zone's width in points: four modules of paper around the grid.
pub fn quiet(matrix: &Matrix) -> f32 {
    module(matrix) * tokens::QR_QUIET as f32
}

/// The side of the drawn block in points: the modules and their quiet zone, never wider than the token.
pub fn side(matrix: &Matrix) -> f32 {
    module(matrix) * (matrix.size() + tokens::QR_QUIET * 2) as f32
}

/// **The square the panel draws**: a light block on a whole point, with a dark rectangle per run of dark
/// modules. The wrapper is the token's square whatever the grid's version is, so the panel does not jump when a
/// longer relay string pushes the encoder to the next version.
pub fn square(matrix: &Matrix) -> El {
    let module = module(matrix);
    let side = side(matrix);
    let quiet = quiet(matrix);
    // **On a whole point**: half a module edge is a blurred module, and a blurred module is a decoder's guess.
    let inset = ((tokens::QR_SIDE - side) / 2.0).round();
    let mut block = el()
        .key("pairing-qr")
        .absolute(inset, inset)
        .size(side, side)
        .fill(tokens::qr_paper());
    for (y, start, len) in runs(matrix) {
        block = block.child(
            el().absolute(quiet + start as f32 * module, quiet + y as f32 * module)
                .size(len as f32 * module, module)
                .fill(tokens::qr_ink()),
        );
    }
    el().key("pairing-qr-square")
        .size(tokens::QR_SIDE, tokens::QR_SIDE)
        .child(block)
}

/// The dark runs of the grid, in draw order: `(row, first module, how many)`. One rectangle per run is the
/// whole picture, and this is the one place that decides where a run begins and ends.
fn runs(matrix: &Matrix) -> Vec<(usize, usize, usize)> {
    let mut runs = Vec::new();
    for y in 0..matrix.size() {
        let mut x = 0;
        while x < matrix.size() {
            if !matrix.module(x, y) {
                x += 1;
                continue;
            }
            let start = x;
            while x < matrix.size() && matrix.module(x, y) {
                x += 1;
            }
            runs.push((y, start, x - start));
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The payload a realistic wall draws: a TCP relay and the scene's code.
    const PAYLOAD: &str = "arcons://pair?relay=tcp://relay.arcons.test:7030&code=481902";

    fn matrix() -> Matrix {
        Matrix::encode(PAYLOAD).expect("a realistic payload is encodable")
    }

    /// **The square fits the panel's token, quiet zone inside it** (#270): the modules are whole points, the
    /// light block is never wider than the square it is given, and the quiet zone is four modules of paper.
    #[test]
    fn the_square_fits_its_token_and_its_quiet_zone_is_four_modules() {
        let matrix = matrix();
        let module = module(&matrix);
        assert_eq!(
            module,
            module.trunc(),
            "a module is a whole number of points, or its edges are the rasteriser's guess"
        );
        assert!(
            module >= 2.0,
            "a module of {module} pt is a dot, not a code"
        );
        assert!(
            side(&matrix) <= tokens::QR_SIDE,
            "the drawn square ({} pt) is wider than the {} pt the panel gives it — it would touch the panel's \
             edge and lose its quiet zone",
            side(&matrix),
            tokens::QR_SIDE
        );
        assert!(
            side(&matrix) + 2.0 > tokens::QR_SIDE,
            "the square leaves {} pt of the panel's {} unused: a module size that does not fill the panel is a \
             code read from further away than it has to be",
            tokens::QR_SIDE - side(&matrix),
            tokens::QR_SIDE
        );
        assert_eq!(
            quiet(&matrix),
            module * tokens::QR_QUIET as f32,
            "the quiet zone is four modules, the standard's margin"
        );
    }

    /// **The runs are the grid's own picture** (#270): every dark module is inside exactly one run of its own
    /// row, and no run covers a light one. A run merged past the row's end or a start off by one draws a code
    /// that looks like a QR and decodes to something else, which is the failure a golden cannot see.
    #[test]
    fn the_runs_cover_every_dark_module_and_no_light_one() {
        let matrix = matrix();
        let mut covered = vec![0_u8; matrix.size() * matrix.size()];
        for (y, start, len) in runs(&matrix) {
            for x in start..start + len {
                covered[y * matrix.size() + x] += 1;
            }
        }
        for y in 0..matrix.size() {
            for x in 0..matrix.size() {
                let want = u8::from(matrix.module(x, y));
                assert_eq!(
                    covered[y * matrix.size() + x],
                    want,
                    "module ({x}, {y}) of the {}-module grid is dark {want} and drawn {} time(s)",
                    matrix.size(),
                    covered[y * matrix.size() + x]
                );
            }
        }
    }

    /// **The modules end inside the block** (#270): the quiet zone is the block's margin on all four sides, so
    /// the last module's far edge is never past the paper's own edge.
    #[test]
    fn the_modules_end_inside_the_light_block() {
        let matrix = matrix();
        let quiet = quiet(&matrix);
        let last = quiet + matrix.size() as f32 * module(&matrix);
        assert!(
            last <= side(&matrix) + 0.001,
            "the modules end at {last} pt and the block at {} pt: a module drawn past the paper loses its quiet \
             zone",
            side(&matrix)
        );
    }
}
