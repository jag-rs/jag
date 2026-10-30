//! Direct bitmap replacement; independent of path, transform, paint and clip.
use super::Raster;
impl Raster {
    /// Replace a clipped dirty region from premultiplied RGBA8 samples.
    /// The caller supplies opaque samples for an alpha-disabled destination.
    pub fn put_pixels(
        &mut self,
        source: &[u8],
        size: [u32; 2],
        destination: [i64; 2],
        dirty: [i64; 4],
    ) -> bool {
        let [w, h] = size;
        if (w as usize)
            .checked_mul(h as usize)
            .and_then(|n| n.checked_mul(4))
            != Some(source.len())
        {
            return false;
        }
        let [dx, dy] = destination;
        let [mut x, mut y, mut dw, mut dh] = dirty;
        if dw < 0 {
            x = x.saturating_add(dw);
            dw = dw.saturating_abs();
        }
        if dh < 0 {
            y = y.saturating_add(dh);
            dh = dh.saturating_abs();
        }
        let x0 = x.max(0).max(dx.saturating_neg());
        let y0 = y.max(0).max(dy.saturating_neg());
        let x1 = x
            .saturating_add(dw)
            .min(i64::from(w))
            .min(i64::from(self.width).saturating_sub(dx));
        let y1 = y
            .saturating_add(dh)
            .min(i64::from(h))
            .min(i64::from(self.height).saturating_sub(dy));
        if x1 <= x0 || y1 <= y0 {
            return false;
        }
        let length = (x1 - x0) as usize * 4;
        for sy in y0..y1 {
            let src = (sy as usize * w as usize + x0 as usize) * 4;
            let dst = ((dy + sy) as usize * self.width as usize + (dx + x0) as usize) * 4;
            self.pixels[dst..dst + length].copy_from_slice(&source[src..src + length]);
        }
        true
    }
}
