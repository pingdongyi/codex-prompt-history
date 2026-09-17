use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

const LIMIT: usize = 10_000;
pub type Key = (u64, Option<usize>, bool, usize);
#[derive(Clone)]
struct Piece {
    hit: usize,
    bytes: Range<usize>,
}
struct Row {
    text: String,
    logical: usize,
    start: usize,
    style: Style,
    pieces: Vec<Piece>,
}
struct Document {
    rows: Vec<Row>,
    hits: Vec<usize>,
    truncated: bool,
    max_width: usize,
}

/// Map case-insensitive matches back to complete original graphemes. Mapping is
/// streamed so Unicode case expansion does not require a per-byte index table.
fn ranges(text: &str, query: &str, limit: usize) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    let lower = text.to_lowercase();
    let found: Vec<_> = lower
        .match_indices(query)
        .take(limit + 1)
        .map(|(start, part)| start..start + part.len())
        .collect();
    if text.is_ascii() {
        return found;
    }
    let mut result = Vec::new();
    let mut index = 0;
    let mut folded = 0;
    let mut start = None;
    for (offset, grapheme) in text.grapheme_indices(true) {
        let end = folded + grapheme.to_lowercase().len();
        while let Some(hit) = found.get(index).filter(|hit| hit.start < end) {
            start.get_or_insert(offset);
            if hit.end > end {
                break;
            }
            let hit = start.take().unwrap()..offset + grapheme.len();
            if result.last() != Some(&hit) {
                result.push(hit);
            }
            index += 1;
        }
        folded = end;
        if index == found.len() {
            break;
        }
    }
    result
}

impl Document {
    fn new(lines: Vec<Line<'static>>, width: usize, query: &str) -> Self {
        let mut result = Self {
            rows: Vec::new(),
            hits: Vec::new(),
            truncated: false,
            max_width: 0,
        };
        let query = query.to_lowercase();
        for (logical, line) in lines.into_iter().enumerate() {
            let text: String = line
                .spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect();
            let mut matches = if result.truncated {
                Vec::new()
            } else {
                ranges(&text, &query, LIMIT - result.hits.len())
            };
            if matches.len() > LIMIT - result.hits.len() {
                result.truncated = true;
                matches.truncate(LIMIT - result.hits.len());
            }
            let base = result.hits.len();
            result.hits.resize(base + matches.len(), result.rows.len());
            let mut assigned = vec![false; matches.len()];
            let indent = " ".repeat(
                text.chars()
                    .take_while(|&c| c == ' ')
                    .count()
                    .min(width.saturating_sub(1)),
            );
            let parts = if width == 0 {
                vec![std::borrow::Cow::Borrowed(text.as_str())]
            } else {
                textwrap::wrap(
                    &text,
                    textwrap::Options::new(width).subsequent_indent(&indent),
                )
            };
            let mut consumed = 0;
            let mut first_match = 0;
            for part in parts {
                let part = part.into_owned();
                let (prefix, start, end) = if text[consumed..].starts_with(&part) {
                    (0, consumed, consumed + part.len())
                } else {
                    let content = part.trim_start_matches(' ');
                    let prefix = part.len() - content.len();
                    if let Some(offset) = text[consumed..].find(content) {
                        let start = consumed + offset;
                        (prefix, start, start + content.len())
                    } else {
                        (part.len(), consumed, consumed)
                    }
                };
                consumed = end;
                while first_match < matches.len() && matches[first_match].end <= start {
                    first_match += 1;
                }
                let mut pieces = Vec::new();
                for (index, hit) in matches
                    .iter()
                    .enumerate()
                    .skip(first_match)
                    .take_while(|(_, hit)| hit.start < end)
                {
                    let bytes =
                        prefix + hit.start.max(start) - start..prefix + hit.end.min(end) - start;
                    if bytes.start < bytes.end
                        && bytes.end <= part.len()
                        && part.is_char_boundary(bytes.start)
                        && part.is_char_boundary(bytes.end)
                    {
                        if !assigned[index] {
                            result.hits[base + index] = result.rows.len();
                            assigned[index] = true;
                        }
                        pieces.push(Piece {
                            hit: base + index,
                            bytes,
                        });
                    }
                }
                result.max_width = result
                    .max_width
                    .max(unicode_width::UnicodeWidthStr::width(part.as_str()));
                result.rows.push(Row {
                    text: part,
                    logical,
                    start,
                    style: line.style,
                    pieces,
                });
            }
        }
        result
    }
    fn render(&self, offset: usize, height: usize, active: Option<usize>) -> Vec<Line<'static>> {
        self.rows
            .iter()
            .skip(offset)
            .take(height)
            .map(|row| {
                // Case expansion can map disjoint text matches onto overlapping
                // original graphemes. Paint their union without repeating text.
                if row
                    .pieces
                    .windows(2)
                    .any(|pair| pair[1].bytes.start < pair[0].bytes.end)
                {
                    let mut boundaries = vec![0, row.text.len()];
                    for piece in &row.pieces {
                        boundaries.extend([piece.bytes.start, piece.bytes.end]);
                    }
                    boundaries.sort_unstable();
                    boundaries.dedup();
                    let spans: Vec<Span> = boundaries
                        .windows(2)
                        .map(|range| {
                            let covering = |piece: &&Piece| {
                                piece.bytes.start <= range[0] && piece.bytes.end >= range[1]
                            };
                            let style = if row
                                .pieces
                                .iter()
                                .filter(covering)
                                .any(|piece| active == Some(piece.hit))
                            {
                                Style::default()
                                    .fg(Color::Black)
                                    .bg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD)
                            } else if row.pieces.iter().any(|piece| covering(&piece)) {
                                Style::default().fg(Color::Yellow).bg(Color::DarkGray)
                            } else {
                                Style::default()
                            };
                            Span::styled(row.text[range[0]..range[1]].to_owned(), style)
                        })
                        .collect();
                    return Line::from(spans).style(row.style);
                }
                let mut spans = Vec::new();
                let mut position = 0;
                for piece in &row.pieces {
                    if piece.bytes.start > position {
                        spans.push(Span::raw(row.text[position..piece.bytes.start].to_owned()));
                    }
                    spans.push(Span::styled(
                        row.text[piece.bytes.clone()].to_owned(),
                        if active == Some(piece.hit) {
                            Style::default()
                                .fg(Color::Black)
                                .bg(Color::Yellow)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::Yellow).bg(Color::DarkGray)
                        },
                    ));
                    position = piece.bytes.end;
                }
                if position < row.text.len() {
                    spans.push(Span::raw(row.text[position..].to_owned()));
                }
                Line::from(spans).style(row.style)
            })
            .collect()
    }
}

#[derive(Default)]
pub struct Find {
    pub query: String,
    pub editor: crate::search::Editor,
    pub editing: bool,
    key: Option<Key>,
    cached_query: String,
    document: Option<Document>,
    current: Option<usize>,
    pending: Option<bool>,
    preserve_scroll: bool,
    reveal_current: bool,
    pan_to_current: bool,
    reset_horizontal: bool,
    saved: Option<(String, usize, crate::app::Focus, Option<usize>)>,
}
impl Find {
    pub fn begin(&mut self, scroll: usize, focus: crate::app::Focus) {
        self.saved = Some((self.query.clone(), scroll, focus, self.current));
        self.editor.begin(&self.query);
        self.editing = true;
    }
    pub fn confirm(&mut self) {
        self.editor.remember(&self.query);
        self.editing = false;
        self.saved = None;
    }
    pub fn cancel(&mut self) -> Option<(usize, crate::app::Focus)> {
        self.editing = false;
        self.saved.take().map(|(query, scroll, focus, current)| {
            self.query = query;
            self.current = current;
            self.preserve_scroll = true;
            self.pending = None;
            (scroll, focus)
        })
    }
    pub fn changed(&mut self) {
        self.current = None;
        self.pending = Some(true);
    }
    pub fn preserve_scroll(&mut self) {
        self.preserve_scroll = true;
        self.pending = None;
        self.current = None;
    }
    pub fn clear(&mut self) {
        self.query.clear();
        self.current = None;
        self.pending = None;
        self.preserve_scroll = true;
    }
    pub fn next(&mut self, forward: bool) {
        self.pending = Some(forward);
    }
    pub fn cached(&self, key: Key, enabled: bool) -> bool {
        self.key == Some(key) && self.cached_query == if enabled { self.query.as_str() } else { "" }
    }
    pub fn rebuild_at(
        &mut self,
        key: Key,
        lines: Vec<Line<'static>>,
        enabled: bool,
        scroll: &mut usize,
    ) {
        let same_record = self
            .key
            .is_some_and(|old| old.0 == key.0 && old.1 == key.1 && old.2 == key.2);
        self.reset_horizontal = !same_record;
        let anchor = same_record
            .then(|| {
                self.document
                    .as_ref()
                    .and_then(|doc| doc.rows.get(*scroll))
                    .map(|row| (row.logical, row.start))
            })
            .flatten();
        self.rebuild(key, lines, enabled);
        if let Some((logical, start)) = anchor {
            *scroll = self
                .document
                .as_ref()
                .and_then(|doc| {
                    doc.rows
                        .iter()
                        .rposition(|row| row.logical == logical && row.start <= start)
                })
                .unwrap_or(*scroll);
        }
    }

    pub fn reveal_horizontal(&mut self, offset: usize, width: usize) -> usize {
        use unicode_width::UnicodeWidthStr;
        let Some(doc) = &self.document else {
            return 0;
        };
        let maximum = doc.max_width.saturating_sub(width).min(u16::MAX as usize);
        let mut offset = if self.reset_horizontal {
            0
        } else {
            offset.min(maximum)
        };
        self.reset_horizontal = false;
        if self.pan_to_current {
            self.pan_to_current = false;
            if let Some(hit) = self.current
                && let Some(row) = doc.hits.get(hit).and_then(|&index| doc.rows.get(index))
                && let Some(piece) = row.pieces.iter().find(|piece| piece.hit == hit)
            {
                let start = row.text[..piece.bytes.start].width();
                let end = row.text[..piece.bytes.end].width();
                if start < offset || end > offset + width {
                    offset = start.min(maximum);
                }
            }
        }
        offset
    }

    pub fn rebuild(&mut self, key: Key, lines: Vec<Line<'static>>, enabled: bool) {
        let query = if enabled { self.query.as_str() } else { "" };
        let reflow = self
            .key
            .is_some_and(|old| old.0 == key.0 && old.1 == key.1 && old.2 == key.2)
            && self.cached_query == query;
        if !self.preserve_scroll && !reflow {
            self.current = None;
        }
        self.document = None;
        self.document = Some(Document::new(lines, key.3, query));
        self.cached_query = query.to_owned();
        self.key = Some(key);
        self.reveal_current = reflow && !self.preserve_scroll && self.current.is_some();
        if !query.is_empty() && !self.preserve_scroll && !reflow && self.pending.is_none() {
            self.pending = Some(true);
        }
    }
    pub fn prepare(&mut self, scroll: &mut usize, height: usize) -> usize {
        let Some(document) = &self.document else {
            return 0;
        };
        let maximum = document.rows.len().saturating_sub(height);
        self.pan_to_current = self.reveal_current || self.pending.is_some();
        if self.reveal_current {
            self.reveal_current = false;
            if let Some(row) = self.current.and_then(|index| document.hits.get(index)) {
                *scroll = (*row).min(maximum);
            }
        }
        if let Some(forward) = self.pending.take() {
            if document.hits.is_empty() {
                self.current = None;
            } else {
                let next = match self.current.filter(|&index| index < document.hits.len()) {
                    Some(index) if forward => (index + 1) % document.hits.len(),
                    Some(index) => (index + document.hits.len() - 1) % document.hits.len(),
                    None if forward => 0,
                    None => document.hits.len() - 1,
                };
                self.current = Some(next);
                *scroll = document.hits[next].min(maximum);
            }
        }
        self.preserve_scroll = false;
        *scroll = (*scroll).min(maximum);
        maximum
    }
    pub fn label(&self) -> String {
        let count = self.document.as_ref().map_or(0, |doc| doc.hits.len());
        let extra = if self.document.as_ref().is_some_and(|doc| doc.truncated) {
            "+ (first 10000)"
        } else {
            ""
        };
        format!(
            "{}/{count}{extra} matches · n/N jump · F clear",
            self.current
                .filter(|&index| index < count)
                .map_or(0, |index| index + 1)
        )
    }
    pub fn render(&self, scroll: usize, height: usize) -> Vec<Line<'static>> {
        self.document.as_ref().map_or_else(Vec::new, |document| {
            document.render(scroll, height, self.current)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_wrap_find_reveals_horizontal_match_and_reflow_retains_text() {
        let lines = vec![Line::from("a ".repeat(40) + "目标"), Line::from("last")];
        let mut find = Find::default();
        let mut scroll = 0;
        find.rebuild_at((0, Some(0), false, 10), lines.clone(), true, &mut scroll);
        scroll = 3;
        find.rebuild_at((0, Some(0), false, 0), lines.clone(), true, &mut scroll);
        assert_eq!(scroll, 0);
        assert_eq!(find.document.as_ref().unwrap().rows.len(), 2);
        find.query = "目标".into();
        find.rebuild_at((0, Some(0), false, 0), lines.clone(), true, &mut scroll);
        find.prepare(&mut scroll, 4);
        assert_eq!(find.current, Some(0));
        assert_eq!(find.reveal_horizontal(0, 10), 74);
        find.rebuild_at((0, Some(0), false, 10), lines, true, &mut scroll);
        find.prepare(&mut scroll, 2);
        assert_eq!(find.current, Some(0));
        assert!(scroll > 0);
    }
    #[test]
    fn active_match_survives_resize_and_cancel_and_styles_are_distinct() {
        let mut find = Find {
            query: "hit".into(),
            ..Default::default()
        };
        let lines = || vec![Line::from("hit then hit")];
        let mut key = (1, Some(0), false, 20);
        find.rebuild(key, lines(), true);
        let mut scroll = 0;
        find.prepare(&mut scroll, 1);
        find.next(true);
        find.prepare(&mut scroll, 1);
        let rendered = find.render(0, 1);
        assert_eq!(rendered[0].spans[0].style.bg, Some(Color::DarkGray));
        assert_eq!(rendered[0].spans[2].style.bg, Some(Color::Yellow));
        key.3 = 5;
        find.rebuild(key, lines(), true);
        find.prepare(&mut scroll, 1);
        assert!(find.label().starts_with("2/2"));
        assert_eq!(scroll, 2);
        find.begin(scroll, crate::app::Focus::Detail);
        find.query = "missing".into();
        find.changed();
        find.rebuild(key, lines(), true);
        find.prepare(&mut scroll, 1);
        let (old_scroll, focus) = find.cancel().unwrap();
        assert_eq!(focus, crate::app::Focus::Detail);
        scroll = old_scroll;
        find.rebuild(key, lines(), true);
        find.prepare(&mut scroll, 1);
        assert_eq!(scroll, 2);
        assert!(find.label().starts_with("2/2"));
    }
    #[test]
    fn matches_cross_soft_wraps_and_unicode_case_expansion() {
        let document = Document::new(
            vec![Line::from("hello target world"), Line::from("İSTANBUL 👩‍💻")],
            8,
            "target world",
        );
        assert_eq!(document.hits.len(), 1);
        assert!(
            document
                .rows
                .iter()
                .filter(|row| !row.pieces.is_empty())
                .count()
                >= 2
        );
        assert_eq!(ranges("İSTANBUL", "i", 10), vec![0..2]);
        assert_eq!(ranges("x👩‍💻y", "👩", 10), vec![1..12]);
        let original = "a\u{301}x\u{301}x";
        let document = Document::new(vec![Line::from(original)], 30, "\u{301}x");
        assert_eq!(document.hits.len(), 2);
        let rendered: String = document.render(0, 1, Some(1))[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(rendered, original);
    }
    #[test]
    fn jumps_wrap_and_search_is_bounded() {
        let mut find = Find {
            query: "hit".into(),
            ..Default::default()
        };
        let key = (1, Some(0), false, 12);
        find.rebuild(
            key,
            vec![Line::from("hit"), Line::from("middle"), Line::from("hit")],
            true,
        );
        let mut scroll = 0;
        find.prepare(&mut scroll, 1);
        assert_eq!(scroll, 0);
        find.next(true);
        find.prepare(&mut scroll, 1);
        assert_eq!(scroll, 2);
        find.next(true);
        find.prepare(&mut scroll, 1);
        assert_eq!(scroll, 0);
        find.next(false);
        find.prepare(&mut scroll, 1);
        assert_eq!(scroll, 2);
        assert!(find.cached(key, true));
        let document = Document::new(vec![Line::from("a ".repeat(LIMIT + 1))], 80, "a");
        assert_eq!(document.hits.len(), LIMIT);
        assert!(document.truncated);
    }
}
