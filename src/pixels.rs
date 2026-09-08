//! Platform-independent pixel helpers for taskbar icons.
//!
//! The Windows renderer produces a 32-bit **premultiplied BGRA** bitmap for
//! `UpdateLayeredWindow` (`AC_SRC_ALPHA` requires premultiplied channels); the
//! same format `blit_line` already writes for text. Icon pixels have to land
//! in that buffer with a premultiplied source-over composite.
//!
//! Everything here is deliberately free of win32 types so it can be unit
//! tested on any platform — the Windows module is only compiled on Windows,
//! and the premultiplied/straight-alpha distinction is exactly the kind of
//! mistake that only shows up as a faint halo on screen.

/// The pixel the instance bitmap is cleared to before anything is drawn:
/// alpha 1, colour 0. `UpdateLayeredWindow` hit-tests layered windows
/// per-pixel, so a fully transparent pixel would let clicks fall through to
/// the taskbar; alpha 1 is invisible but keeps the whole item clickable.
pub const BACKDROP: u32 = 0x0100_0000;

/// Convert **straight** (non-premultiplied) RGBA — the format the `image`
/// crate decodes into — into the **premultiplied** RGBA that
/// [`blit_icon`] expects.
///
/// SVG rasters are already premultiplied (tiny-skia's `Pixmap` is), so only
/// bitmap sources need this. Running both kinds through the same conversion
/// point is the whole reason the compositing has a single code path.
pub fn to_premultiplied(straight: &[u8]) -> Vec<u8> {
    let mut out = straight.to_vec();
    for px in out.chunks_exact_mut(4) {
        let a = px[3] as u32;
        px[0] = ((px[0] as u32 * a) / 255) as u8;
        px[1] = ((px[1] as u32 * a) / 255) as u8;
        px[2] = ((px[2] as u32 * a) / 255) as u8;
    }
    out
}

/// Width an icon of `src_w x src_h` takes when scaled to `target_h`, keeping
/// the aspect ratio. Used by layout (which needs the width without rasterising
/// anything) as well as by rasterisation.
pub fn width_for_height(src_w: u32, src_h: u32, target_h: i32) -> u32 {
    if src_h == 0 || target_h <= 0 {
        return 0;
    }
    ((src_w as f32 * target_h as f32 / src_h as f32).round() as i32).max(1) as u32
}

/// Composite an icon into the instance bitmap with premultiplied source-over.
///
/// * `dst` — the instance's pixel buffer, premultiplied BGRA
///   (`(A << 24) | B | (G << 8) | (R << 16)`), `win_w * win_h` pixels.
/// * `src` — the icon, `w * h` pixels of **premultiplied RGBA** (byte order
///   R, G, B, A). See [`to_premultiplied`].
/// * `tint` — when `Some`, the icon's own colours are ignored and its alpha is
///   used as coverage for that colour, exactly like glyph coverage is for
///   text. That is what makes a monochrome icon follow the line colour.
///
/// Because both sides are premultiplied the operator collapses to
/// `out = src + dst * (1 - src_a)` — no per-channel multiply of the source.
/// Out-of-range pixels are clipped rather than panicking: a stale cached size
/// must never take the taskbar down.
#[allow(clippy::too_many_arguments)]
pub fn blit_icon(
    dst: &mut [u32],
    win_w: usize,
    win_h: usize,
    off_x: usize,
    off_y: usize,
    w: usize,
    h: usize,
    src: &[u8],
    tint: Option<(u8, u8, u8)>,
) {
    if win_w == 0 || win_h == 0 || w == 0 || h == 0 {
        return;
    }
    for ly in 0..h {
        let dy = off_y + ly;
        if dy >= win_h {
            break;
        }
        for lx in 0..w {
            let dx = off_x + lx;
            if dx >= win_w {
                break;
            }
            let si = (ly * w + lx) * 4;
            if si + 3 >= src.len() {
                return;
            }
            let sa = src[si + 3] as u32;
            if sa == 0 {
                continue;
            }
            let (sr, sg, sb) = match tint {
                Some((tr, tg, tb)) => (
                    (tr as u32 * sa) / 255,
                    (tg as u32 * sa) / 255,
                    (tb as u32 * sa) / 255,
                ),
                None => (src[si] as u32, src[si + 1] as u32, src[si + 2] as u32),
            };
            let idx = dy * win_w + dx;
            if idx >= dst.len() {
                return;
            }
            let dp = dst[idx];
            let d_b = dp & 0xFF;
            let d_g = (dp >> 8) & 0xFF;
            let d_r = (dp >> 16) & 0xFF;
            let d_a = (dp >> 24) & 0xFF;
            let inv = 255 - sa;
            // src is premultiplied, so it is already scaled by sa.
            let out_a = sa + (d_a * inv) / 255;
            let out_r = sr + (d_r * inv) / 255;
            let out_g = sg + (d_g * inv) / 255;
            let out_b = sb + (d_b * inv) / 255;
            dst[idx] = (out_a << 24) | out_b | (out_g << 8) | (out_r << 16);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `(A << 24) | B | (G << 8) | (R << 16)` — the instance bitmap format.
    const fn px(a: u32, r: u32, g: u32, b: u32) -> u32 {
        (a << 24) | b | (g << 8) | (r << 16)
    }

    fn canvas(w: usize, h: usize) -> Vec<u32> {
        vec![BACKDROP; w * h]
    }

    #[test]
    fn straight_alpha_becomes_premultiplied() {
        // Half-transparent white: straight (255,255,255,128) -> premul 128.
        assert_eq!(
            to_premultiplied(&[255, 255, 255, 128]),
            vec![128, 128, 128, 128]
        );
        // Fully transparent colour contributes nothing.
        assert_eq!(to_premultiplied(&[255, 0, 0, 0]), vec![0, 0, 0, 0]);
        // Opaque is unchanged.
        assert_eq!(to_premultiplied(&[10, 20, 30, 255]), vec![10, 20, 30, 255]);
    }

    #[test]
    fn opaque_icon_replaces_the_backdrop() {
        let mut dst = canvas(2, 1);
        // Premultiplied opaque red.
        blit_icon(&mut dst, 2, 1, 0, 0, 1, 1, &[255, 0, 0, 255], None);
        assert_eq!(dst[0], px(255, 255, 0, 0));
        // Untouched neighbour keeps the alpha-1 backdrop.
        assert_eq!(dst[1], BACKDROP);
    }

    #[test]
    fn translucent_icon_blends_over_the_backdrop() {
        let mut dst = canvas(1, 1);
        // Premultiplied black at alpha 128 over a=1 backdrop.
        blit_icon(&mut dst, 1, 1, 0, 0, 1, 1, &[0, 0, 0, 128], None);
        let out = dst[0];
        assert_eq!(out >> 24, 128 + (1 * 127) / 255); // ~128
        assert_eq!((out >> 16) & 0xFF, 0);
        assert_eq!((out >> 8) & 0xFF, 0);
        assert_eq!(out & 0xFF, 0);
    }

    #[test]
    fn transparent_pixels_leave_the_destination_alone() {
        let mut dst = canvas(1, 1);
        blit_icon(&mut dst, 1, 1, 0, 0, 1, 1, &[255, 255, 255, 0], None);
        assert_eq!(dst[0], BACKDROP);
    }

    #[test]
    fn tint_uses_alpha_as_coverage_for_the_line_colour() {
        let mut dst = canvas(2, 1);
        // Fully opaque shape tinted red -> pure red, source colour ignored.
        blit_icon(
            &mut dst,
            2,
            1,
            0,
            0,
            1,
            1,
            &[0, 0, 255, 255],
            Some((255, 0, 0)),
        );
        assert_eq!(dst[0], px(255, 255, 0, 0));
        // Half-covered pixel -> half of red, premultiplied.
        blit_icon(
            &mut dst,
            2,
            1,
            1,
            0,
            1,
            1,
            &[0, 0, 255, 128],
            Some((255, 0, 0)),
        );
        let half = dst[1];
        assert_eq!(half >> 24, 128 + (1 * 127) / 255);
        assert_eq!((half >> 16) & 0xFF, 128);
        assert_eq!((half >> 8) & 0xFF, 0);
    }

    #[test]
    fn offset_and_clipping_are_respected() {
        let green = vec![0u8, 255, 0, 255].repeat(4); // 2x2, opaque green
        // 3x2 canvas, 2x2 icon at (1,1): the bottom row overflows and is
        // clipped away; the right column fits exactly and is painted.
        let mut dst = canvas(3, 2);
        blit_icon(&mut dst, 3, 2, 1, 1, 2, 2, &green, None);
        assert_eq!(dst[1 * 3 + 1], px(255, 0, 255, 0));
        assert_eq!(dst[1 * 3 + 2], px(255, 0, 255, 0));
        assert_eq!(dst[0], BACKDROP);
        assert_eq!(dst[2], BACKDROP);

        // A 2x2 icon at (2,0) must clip its second column, never wrap it onto
        // the next row.
        let mut dst = canvas(3, 2);
        blit_icon(&mut dst, 3, 2, 2, 0, 2, 2, &green, None);
        assert_eq!(dst[0 * 3 + 2], px(255, 0, 255, 0));
        assert_eq!(dst[1 * 3 + 2], px(255, 0, 255, 0));
        assert_eq!(dst[0 * 3 + 1], BACKDROP);
        assert_eq!(dst[1 * 3 + 1], BACKDROP);
    }

    #[test]
    fn width_keeps_aspect_ratio() {
        assert_eq!(width_for_height(64, 32, 16), 32);
        assert_eq!(width_for_height(10, 40, 20), 5);
        // Degenerate inputs must not produce 0 (which would drop the icon).
        assert_eq!(width_for_height(64, 32, 0), 0);
        assert_eq!(width_for_height(64, 0, 16), 0);
        assert_eq!(width_for_height(1, 100, 10), 1);
    }
}
