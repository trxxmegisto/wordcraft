//! Chapters: the document's headings, and the text from a heading to the next one of the same or
//! a higher level. The assistant can check one chapter at a time.

use std::sync::Arc;

use wordcraft_engine::doc::{Block, Document};

/// Heading titles longer than this are shortened in the list (characters).
pub const MAX_TITLE_CHARS: usize = 80;

/// Table cells inside table cells are read this deep; deeper text is left out.
const MAX_TABLE_DEPTH: usize = 16;

/// A heading in the main text: where it is, its level and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chapter {
    /// The heading's index among the body's top-level blocks.
    pub block: usize,
    /// Heading level, 1–9 (Heading 1 is a chapter, Heading 2 a section in it, …).
    pub level: u8,
    /// The heading's text, without tracked deletions, shortened to [`MAX_TITLE_CHARS`].
    pub title: String,
}

/// The headings among the body's top-level paragraphs, in document order: paragraphs whose style
/// or own properties give them an outline level (Heading 1–9), with text. Headings in tables and
/// text boxes don't start chapters.
pub fn chapters(doc: &Document) -> Vec<Chapter> {
    let mut out = Vec::new();
    for (block, b) in doc.body.iter().enumerate() {
        let Block::Para(p) = &**b else { continue };
        let Some(level) = doc.styles.resolve_para(&p.props).outline_level.filter(|l| *l < 9) else { continue };
        let text = p.final_text();
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let mut title: String = text.chars().take(MAX_TITLE_CHARS).collect();
        if text.chars().count() > MAX_TITLE_CHARS {
            title.push('…');
        }
        out.push(Chapter { block, level: level.saturating_add(1), title });
    }
    out
}

/// The chapter a body block belongs to: the last heading at or before it.
pub fn chapter_at(chapters: &[Chapter], block: usize) -> Option<&Chapter> {
    chapters.iter().take_while(|c| c.block <= block).last()
}

/// The text of the chapter whose heading is body block `block`: the heading and everything up to
/// the next heading of the same or a higher level (or the end), without tracked deletions;
/// paragraphs end in `\n`, table cells are separated by `\t`. None if `block` isn't a heading.
pub fn chapter_text(doc: &Document, block: usize) -> Option<String> {
    let all = chapters(doc);
    let at = all.iter().position(|c| c.block == block)?;
    let level = all.get(at)?.level;
    let end = all.iter().skip(at + 1).find(|c| c.level <= level).map_or(doc.body.len(), |c| c.block);
    let mut out = String::new();
    text_of(doc.body.get(block..end)?, &mut out, 0);
    Some(out)
}

fn text_of(blocks: &[Arc<Block>], out: &mut String, depth: usize) {
    if depth > MAX_TABLE_DEPTH {
        return;
    }
    for b in blocks {
        match &**b {
            Block::Para(p) => {
                out.push_str(&p.final_text());
                out.push('\n');
            }
            Block::Table(t) => {
                for row in &t.rows {
                    let cells: Vec<String> = row
                        .cells
                        .iter()
                        .map(|c| {
                            let mut s = String::new();
                            text_of(&c.blocks, &mut s, depth + 1);
                            s.trim_end_matches('\n').replace('\n', " ")
                        })
                        .collect();
                    out.push_str(&cells.join("\t"));
                    out.push('\n');
                }
            }
        }
    }
}
