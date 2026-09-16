use chrono::{Days, Local, NaiveDate};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Window {
    Week,
    #[default]
    Month,
    All,
}
impl Window {
    pub fn next(self) -> Self {
        match self {
            Self::Week => Self::Month,
            Self::Month => Self::All,
            Self::All => Self::Week,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Week => "7 days",
            Self::Month => "30 days",
            Self::All => "all time",
        }
    }
}

pub fn local_date(timestamp: i64) -> Option<NaiveDate> {
    chrono::DateTime::from_timestamp(timestamp, 0)
        .map(|time| time.with_timezone(&Local).date_naive())
}

/// Sparse daily counts avoid allocations proportional to an all-time date span.
#[derive(Default)]
pub struct Activity {
    pub counts: BTreeMap<NaiveDate, u64>,
}

impl Activity {
    pub fn bounds(&self, window: Window, today: NaiveDate) -> (NaiveDate, NaiveDate) {
        let start = match window {
            Window::Week => today.checked_sub_days(Days::new(6)).unwrap_or(today),
            Window::Month => today.checked_sub_days(Days::new(29)).unwrap_or(today),
            Window::All => self
                .counts
                .keys()
                .next()
                .copied()
                .unwrap_or(today)
                .min(today),
        };
        (start, today)
    }

    pub fn count(&self, date: NaiveDate) -> u64 {
        self.counts.get(&date).copied().unwrap_or(0)
    }

    pub fn total(&self, start: NaiveDate, end: NaiveDate) -> u64 {
        self.counts.range(start..=end).map(|(_, count)| count).sum()
    }

    /// Wide charts repeat each day; compressed charts show the maximum daily
    /// count in each column, without inflating the total shown in the header.
    pub fn bars(&self, start: NaiveDate, end: NaiveDate, width: usize) -> Vec<u64> {
        if width == 0 {
            return Vec::new();
        }
        let days = end.signed_duration_since(start).num_days() as usize + 1;
        if days <= width {
            (0..width)
                .map(|column| {
                    self.count(
                        start
                            .checked_add_days(Days::new((column * days / width) as u64))
                            .unwrap_or(end),
                    )
                })
                .collect()
        } else {
            let mut bars = vec![0; width];
            for (date, count) in self.counts.range(start..=end) {
                let day = date.signed_duration_since(start).num_days() as usize;
                let column = day * width / days;
                bars[column] = bars[column].max(*count);
            }
            bars
        }
    }

    pub fn cursor_column(start: NaiveDate, end: NaiveDate, date: NaiveDate, width: usize) -> usize {
        if width == 0 {
            return 0;
        }
        let days = end.signed_duration_since(start).num_days() as usize + 1;
        let day = date
            .clamp(start, end)
            .signed_duration_since(start)
            .num_days() as usize;
        if days <= width {
            // Use the center of the columns occupied by this day.
            let left = (day * width).div_ceil(days);
            let right = ((day + 1) * width).div_ceil(days);
            (left + right.saturating_sub(1)) / 2
        } else {
            (day * width / days).min(width - 1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn date(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, day).unwrap()
    }
    #[test]
    fn window_bounds_include_today_and_counts_are_exact() {
        let mut activity = Activity::default();
        activity.counts.insert(date(1), 2);
        activity.counts.insert(date(17), 5);
        assert_eq!(
            activity.bounds(Window::Week, date(17)),
            (date(11), date(17))
        );
        assert_eq!(activity.bounds(Window::All, date(17)), (date(1), date(17)));
        assert_eq!(activity.total(date(1), date(17)), 7);
        assert_eq!(activity.count(date(2)), 0);
        assert_eq!(activity.bars(date(1), date(17), 2), vec![2, 5]);
        assert_eq!(activity.bars(date(17), date(17), 4), vec![5; 4]);
        assert!(activity.bars(date(1), date(17), 0).is_empty());
    }
    #[test]
    fn stretched_cursor_points_to_its_day_and_compressed_last_day_stays_visible() {
        let mut activity = Activity::default();
        activity.counts.insert(date(17), 8);
        for width in [1, 12, 17, 43, 118] {
            let bars = activity.bars(date(1), date(17), width);
            let column = Activity::cursor_column(date(1), date(17), date(17), width);
            assert_eq!(bars[column], 8);
        }
    }
}
