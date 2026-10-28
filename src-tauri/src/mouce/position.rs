//! Pure placement math for the mouce window. All rects are physical pixels.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// Bottom-right corner of the work area, 16 logical px margin. Result is physical.
pub fn default_position(work_area: Rect, scale: f64, size_logical: u32) -> (i32, i32) {
    let margin = (16.0 * scale).round() as i32;
    let size = (size_logical as f64 * scale).round() as i32;
    (
        work_area.x + work_area.w as i32 - margin - size,
        work_area.y + work_area.h as i32 - margin - size,
    )
}

/// `Some(pos)` only if the whole mouce fits inside some work area.
pub fn clamp_to_monitors(
    pos: (i32, i32),
    size_physical: u32,
    work_areas: &[Rect],
) -> Option<(i32, i32)> {
    let size = size_physical as i64;
    let (px, py) = (pos.0 as i64, pos.1 as i64);
    work_areas
        .iter()
        .any(|r| {
            let (rx, ry) = (r.x as i64, r.y as i64);
            px >= rx && py >= ry && px + size <= rx + r.w as i64 && py + size <= ry + r.h as i64
        })
        .then_some(pos)
}

/// Shift `pos` the minimum distance so a square of `size_physical` lies inside `work_area`.
/// If the square is larger than the work area, it is pinned to the top-left corner.
pub fn keep_inside(pos: (i32, i32), size_physical: u32, work_area: Rect) -> (i32, i32) {
    let fit = |p: i32, origin: i32, extent: u32| {
        let max = origin + extent as i32 - size_physical as i32;
        p.min(max).max(origin)
    };
    (fit(pos.0, work_area.x, work_area.w), fit(pos.1, work_area.y, work_area.h))
}

/// Where a `panel_w` x `panel_h` panel goes next to `anchor` (the mouce): to its left if it
/// fits, else to its right, bottom-aligned with it, and always inside `work_area`.
pub fn panel_position(
    anchor: Rect,
    panel_w: u32,
    panel_h: u32,
    gap: i32,
    work_area: Rect,
) -> (i32, i32) {
    let (left_edge, right_edge) = (work_area.x, work_area.x + work_area.w as i32);
    let left = anchor.x - gap - panel_w as i32;
    let right = anchor.x + anchor.w as i32 + gap;
    let x = if left >= left_edge {
        left
    } else if right + panel_w as i32 <= right_edge {
        right
    } else {
        left.max(left_edge)
    };
    let y = anchor.y + anchor.h as i32 - panel_h as i32;
    let max_y = work_area.y + work_area.h as i32 - panel_h as i32;
    (x.min(right_edge - panel_w as i32).max(left_edge), y.min(max_y).max(work_area.y))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WA: Rect = Rect { x: 0, y: 0, w: 1920, h: 1040 };

    #[test]
    fn panel_goes_left_of_a_mouce_in_the_bottom_right_corner() {
        let mouce = Rect { x: 1840, y: 960, w: 64, h: 64 };
        assert_eq!(panel_position(mouce, 380, 520, 8, WA), (1452, 504));
    }

    #[test]
    fn panel_goes_right_when_there_is_no_room_on_the_left() {
        let mouce = Rect { x: 20, y: 600, w: 64, h: 64 };
        assert_eq!(panel_position(mouce, 380, 520, 8, WA), (92, 144));
    }

    #[test]
    fn panel_stays_inside_the_work_area() {
        let near_top = Rect { x: 1000, y: 10, w: 64, h: 64 };
        assert_eq!(panel_position(near_top, 380, 520, 8, WA), (612, 0));
        let squeezed = Rect { x: 100, y: 500, w: 64, h: 64 };
        let narrow = Rect { x: 0, y: 0, w: 400, h: 1040 };
        assert_eq!(panel_position(squeezed, 380, 520, 8, narrow), (0, 44));
        let offset = Rect { x: 1920, y: 0, w: 1280, h: 1000 };
        let mouce = Rect { x: 3100, y: 900, w: 64, h: 64 };
        assert_eq!(panel_position(mouce, 380, 520, 8, offset), (2712, 444));
    }

    #[test]
    fn keep_inside_pulls_overflowing_mouce_back() {
        let wa = Rect { x: 0, y: 0, w: 1920, h: 1040 };
        assert_eq!(keep_inside((1900, 1030), 72, wa), (1848, 968));
        assert_eq!(keep_inside((-5, 20), 72, wa), (0, 20));
        assert_eq!(keep_inside((100, 100), 72, wa), (100, 100));
    }

    #[test]
    fn default_position_bottom_right_100pct() {
        let wa = Rect { x: 0, y: 0, w: 1920, h: 1040 };
        assert_eq!(default_position(wa, 1.0, 48), (1856, 976));
    }

    #[test]
    fn default_position_respects_offset_work_area_and_scale() {
        let wa = Rect { x: 60, y: 0, w: 2500, h: 1600 };
        assert_eq!(default_position(wa, 1.5, 48), (2464, 1504));
    }

    #[test]
    fn clamp_keeps_visible_position() {
        let wa = Rect { x: 0, y: 0, w: 1920, h: 1040 };
        assert_eq!(clamp_to_monitors((100, 100), 72, &[wa]), Some((100, 100)));
    }

    #[test]
    fn clamp_rejects_position_off_all_monitors() {
        let wa = Rect { x: 0, y: 0, w: 1920, h: 1040 };
        assert_eq!(clamp_to_monitors((3000, 200), 72, &[wa]), None);
        assert_eq!(clamp_to_monitors((1900, 100), 72, &[wa]), None);
    }
}
