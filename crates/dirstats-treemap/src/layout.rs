// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// Ported from WinDirStat windirstat/Controls/TreeMapLayout.cpp
// by WinDirStat Team (https://windirstat.net), GPL-2.0-or-later.
// The rows algorithm originates in KDirStat/WinDirStat by Bernhard Seifert;
// squarified layout: Bruls, Huizing, van Wijk, "Squarified Treemaps" (2000).

//! Treemap layout of one sibling list.

/// Integer rectangle, right/bottom exclusive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    #[must_use]
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self { left, top, right, bottom }
    }

    #[must_use]
    pub const fn width(&self) -> i32 {
        self.right - self.left
    }

    #[must_use]
    pub const fn height(&self) -> i32 {
        self.bottom - self.top
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.width() <= 0 || self.height() <= 0
    }

    #[must_use]
    pub const fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Style {
    /// KDirStat-style rows: fill a row until children get too thin.
    #[default]
    Rows,
    /// Squarified: grow a strip while its worst aspect ratio improves.
    Squarified,
}

/// Arrange children with `weights` inside `bounds`; `out[i]` receives child `i`'s rectangle.
///
/// `weights` must be non-increasing (zeroes last) and sum to `parent_weight`.
pub fn arrange(style: Style, bounds: Rect, parent_weight: u64, weights: &[u64], out: &mut Vec<Rect>) {
    debug_assert!(weights.windows(2).all(|w| w[0] >= w[1]));
    debug_assert_eq!(weights.iter().sum::<u64>(), parent_weight);
    out.clear();
    out.resize(weights.len(), Rect::default());
    if weights.is_empty() || bounds.is_empty() {
        return;
    }
    if parent_weight == 0 {
        arrange_equal_rows(bounds, out);
        return;
    }
    match style {
        Style::Rows => arrange_rows(bounds, parent_weight, weights, out),
        Style::Squarified => arrange_squarified(bounds, parent_weight, weights, out),
    }
    // Unlike WinDirStat, keep zero-weight children from overlapping their neighbours.
    for (rect, _) in out.iter_mut().zip(weights).filter(|(_, w)| **w == 0) {
        *rect = Rect::default();
    }
}

fn is_last_in_row(weights: &[u64], i: usize, row_end: usize) -> bool {
    i + 1 == row_end || (i + 1 < weights.len() && weights[i + 1] == 0)
}

fn arrange_equal_rows(bounds: Rect, out: &mut [Rect]) {
    let count = out.len();
    let width = f64::from(bounds.width()) / count as f64;
    let mut left = f64::from(bounds.left);
    for (i, rect) in out.iter_mut().enumerate() {
        let next = left + width;
        let right = if i + 1 == count { bounds.right } else { next as i32 };
        *rect = Rect::new(left as i32, bounds.top, right, bounds.bottom);
        left = next;
    }
}

fn arrange_rows(bounds: Rect, parent_weight: u64, weights: &[u64], out: &mut [Rect]) {
    const MIN_PROPORTION: f64 = 0.4;
    let horizontal = bounds.width() >= bounds.height();
    let (width, height) = (f64::from(bounds.width()), f64::from(bounds.height()));
    let normalized_width = if horizontal { width / height } else { height / width };
    let row_extent = if horizontal { height } else { width };
    let column_extent = if horizontal { width } else { height };
    let row_far = if horizontal { bounds.bottom } else { bounds.right };
    let column_near = f64::from(if horizontal { bounds.left } else { bounds.top });
    let column_far = if horizontal { bounds.right } else { bounds.bottom };
    let parent_weight = parent_weight as f64;

    let mut top = f64::from(if horizontal { bounds.top } else { bounds.left });
    let mut row_begin = 0;
    while row_begin < weights.len() {
        let mut row_weight = 0u64;
        let mut row_fraction = 0.0;
        let mut row_end = row_begin;
        while row_end < weights.len() {
            let child_weight = weights[row_end];
            if child_weight == 0 {
                break;
            }
            let candidate_fraction = (row_weight + child_weight) as f64 / parent_weight;
            let child_width =
                child_weight as f64 / parent_weight * normalized_width / candidate_fraction;
            if child_width / candidate_fraction < MIN_PROPORTION {
                break;
            }
            row_weight += child_weight;
            row_fraction = candidate_fraction;
            row_end += 1;
        }

        debug_assert!(row_end > row_begin);
        if row_end == row_begin {
            return;
        }
        while row_end < weights.len() && weights[row_end] == 0 {
            row_end += 1;
        }

        let next_top = top + row_fraction * row_extent;
        let bottom = if row_end == weights.len() { row_far } else { next_top as i32 };
        let mut left = column_near;
        for i in row_begin..row_end {
            let next_left = left + weights[i] as f64 / row_weight as f64 * column_extent;
            let right = if is_last_in_row(weights, i, row_end) {
                column_far
            } else {
                next_left as i32
            };
            out[i] = if horizontal {
                Rect::new(left as i32, top as i32, right, bottom)
            } else {
                Rect::new(top as i32, left as i32, bottom, right)
            };
            left = next_left;
        }

        top = next_top;
        row_begin = row_end;
    }
}

fn arrange_squarified(bounds: Rect, parent_weight: u64, weights: &[u64], out: &mut [Rect]) {
    let mut remaining = bounds;
    let mut remaining_weight = parent_weight;
    let weight_per_pixel =
        remaining_weight as f64 / f64::from(remaining.width()) / f64::from(remaining.height());

    let mut head = 0;
    while head < weights.len() {
        if remaining.is_empty() {
            break;
        }

        let horizontal = remaining.width() >= remaining.height();
        let thickness = f64::from(if horizontal { remaining.height() } else { remaining.width() });
        let squared_row_weight = thickness * thickness * weight_per_pixel;

        let row_begin = head;
        let mut row_end = head;
        let mut worst = f64::MAX;
        let largest = weights[row_begin] as f64;
        let mut row_weight = 0u64;
        while row_end < weights.len() {
            let child_weight = weights[row_end];
            if child_weight == 0 {
                // Zero-weight children ride along with the final row.
                row_end = weights.len();
                break;
            }
            let next_weight = (row_weight + child_weight) as f64;
            let squared_weight = next_weight * next_weight;
            let next_worst = (squared_row_weight * largest / squared_weight)
                .max(squared_weight / squared_row_weight / child_weight as f64);
            if next_worst > worst {
                break;
            }
            row_weight += child_weight;
            row_end += 1;
            worst = next_worst;
        }
        if row_weight == 0 {
            break;
        }

        let remaining_extent = if horizontal { remaining.width() } else { remaining.height() };
        let row_width = if row_weight < remaining_weight {
            ((row_weight as f64 / remaining_weight as f64 * f64::from(remaining_extent)) as i32)
                .clamp(1, remaining_extent)
        } else {
            remaining_extent
        };
        let mut row = remaining;
        if horizontal {
            row.right = row.left + row_width;
        } else {
            row.bottom = row.top + row_width;
        }

        let span = f64::from(if horizontal { row.height() } else { row.width() });
        let mut begin = f64::from(if horizontal { row.top } else { row.left });
        for i in row_begin..row_end {
            let next = begin + weights[i] as f64 / row_weight as f64 * span;
            let end = if is_last_in_row(weights, i, row_end) {
                if horizontal { row.bottom } else { row.right }
            } else {
                next as i32
            };
            out[i] = if horizontal {
                Rect::new(row.left, begin as i32, row.right, end)
            } else {
                Rect::new(begin as i32, row.top, end, row.bottom)
            };
            begin = next;
        }

        if horizontal {
            remaining.left += row_width;
        } else {
            remaining.top += row_width;
        }
        remaining_weight -= row_weight;
        head = row_end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(r: &Rect) -> i64 {
        i64::from(r.width().max(0)) * i64::from(r.height().max(0))
    }

    fn check(style: Style, bounds: Rect, weights: &[u64]) -> Vec<Rect> {
        let total = weights.iter().sum();
        let mut out = Vec::new();
        arrange(style, bounds, total, weights, &mut out);
        assert_eq!(out.len(), weights.len());
        for r in out.iter().filter(|r| !r.is_empty()) {
            assert!(r.left >= bounds.left && r.right <= bounds.right, "{r:?}");
            assert!(r.top >= bounds.top && r.bottom <= bounds.bottom, "{r:?}");
        }
        // Rectangles tile the bounds exactly.
        assert_eq!(out.iter().map(area).sum::<i64>(), area(&bounds));
        for (i, a) in out.iter().enumerate() {
            for b in &out[i + 1..] {
                let overlap = a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;
                assert!(!overlap, "{a:?} overlaps {b:?}");
            }
        }
        out
    }

    #[test]
    fn areas_follow_weights() {
        let weights = [500, 250, 125, 60, 40, 20, 5];
        for style in [Style::Rows, Style::Squarified] {
            let bounds = Rect::new(0, 0, 400, 300);
            let out = check(style, bounds, &weights);
            let total: u64 = weights.iter().sum();
            for (w, r) in weights.iter().zip(&out) {
                let expected = *w as f64 / total as f64 * area(&bounds) as f64;
                let actual = area(r) as f64;
                assert!((actual - expected).abs() <= expected * 0.1 + 400.0, "{style:?} {w}: {actual} vs {expected}");
            }
        }
    }

    #[test]
    fn handles_zero_weights_and_tall_bounds() {
        for style in [Style::Rows, Style::Squarified] {
            check(style, Rect::new(10, 20, 60, 400), &[30, 20, 10, 0, 0]);
            check(style, Rect::new(0, 0, 7, 3), &[0, 0, 0]);
        }
    }

    #[test]
    fn squarified_is_squarer() {
        let weights: Vec<u64> = (1..=40).rev().map(|w| w * w).collect();
        let bounds = Rect::new(0, 0, 800, 600);
        let worst = |out: &[Rect]| {
            out.iter()
                .filter(|r| !r.is_empty())
                .map(|r| f64::from(r.width().max(r.height())) / f64::from(r.width().min(r.height())))
                .sum::<f64>()
        };
        let rows = check(Style::Rows, bounds, &weights);
        let squarified = check(Style::Squarified, bounds, &weights);
        assert!(worst(&squarified) <= worst(&rows));
    }
}
