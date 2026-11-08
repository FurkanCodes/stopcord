//! The parts of a screen a rule can look at.
//!
//! Region names and meanings match herdr's manifest format, so its rule files
//! work unchanged. "Prompt marker" regions are shaped around Codex's `›` prompt
//! and its `•`, `■`, `✗`, `✓` response markers.

use crate::Screen;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    /// The terminal title.
    Title,
    /// The last OSC 9;4 progress report.
    Progress,
    /// The whole visible screen.
    WholeRecent,
    /// Everything after the last line that starts with `›`.
    AfterLastPromptMarker,
    /// Everything before the current `›` prompt (one no response follows).
    BeforeCurrentPromptMarker,
    /// The whole screen, or nothing when a current `›` prompt is showing.
    WholeRecentWithoutCurrentPromptMarker,
    /// The last response-marker line before the current prompt.
    CurrentPromptBlockMarker,
    /// From that response-marker line to the end.
    AfterCurrentPromptBlockMarker,
    /// The lines inside the bottom box drawn with `─` rules.
    PromptBoxBody,
    /// Everything above that box.
    AbovePromptBox,
    /// The last non-empty line above that box.
    LastNonEmptyAbovePromptBox,
    /// Everything after the last `─` rule, or the whole screen when there is none.
    AfterLastHorizontalRule,
    /// The last N lines.
    BottomLines(usize),
    /// From the Nth-last non-empty line to the end.
    BottomNonEmptyLines(usize),
    /// From the top through the Nth non-empty line.
    TopNonEmptyLines(usize),
}

impl Region {
    pub fn parse(spec: &str) -> Result<Self, String> {
        let spec = spec.trim();
        let fixed = match spec {
            "osc_title" => Some(Region::Title),
            "osc_progress" => Some(Region::Progress),
            "whole_recent" => Some(Region::WholeRecent),
            "after_last_prompt_marker" => Some(Region::AfterLastPromptMarker),
            "before_current_prompt_marker" => Some(Region::BeforeCurrentPromptMarker),
            "whole_recent_without_current_prompt_marker" => {
                Some(Region::WholeRecentWithoutCurrentPromptMarker)
            }
            "current_prompt_block_marker" => Some(Region::CurrentPromptBlockMarker),
            "after_current_prompt_block_marker" => Some(Region::AfterCurrentPromptBlockMarker),
            "prompt_box_body" => Some(Region::PromptBoxBody),
            "above_prompt_box" => Some(Region::AbovePromptBox),
            "last_non_empty_above_prompt_box" => Some(Region::LastNonEmptyAbovePromptBox),
            "after_last_horizontal_rule" => Some(Region::AfterLastHorizontalRule),
            _ => None,
        };
        if let Some(region) = fixed {
            return Ok(region);
        }
        let counted = [
            ("bottom_lines", Region::BottomLines as fn(usize) -> Region),
            ("bottom_non_empty_lines", Region::BottomNonEmptyLines),
            ("top_non_empty_lines", Region::TopNonEmptyLines),
        ];
        for (name, make) in counted {
            if let Some(count) = spec
                .strip_prefix(name)
                .and_then(|rest| rest.strip_prefix('('))
                .and_then(|rest| rest.strip_suffix(')'))
            {
                let n: usize = count
                    .parse()
                    .map_err(|_| format!("region {spec:?}: {count:?} is not a whole number"))?;
                if n == 0 || n > u16::MAX as usize {
                    return Err(format!("region {spec:?}: the count must be 1 to 65535"));
                }
                return Ok(make(n));
            }
        }
        Err(format!("unknown region {spec:?}"))
    }

    /// The part of the screen this region covers.
    pub fn slice<'a>(&self, screen: &Screen<'a>) -> &'a str {
        let text = screen.text;
        match *self {
            Region::Title => screen.title,
            Region::Progress => screen.progress,
            Region::WholeRecent => text,
            Region::AfterLastPromptMarker => after_last_prompt_marker(text),
            Region::BeforeCurrentPromptMarker => before_current_prompt_marker(text),
            Region::WholeRecentWithoutCurrentPromptMarker => {
                if current_prompt_index(&lines(text)).is_some() {
                    ""
                } else {
                    text
                }
            }
            Region::CurrentPromptBlockMarker => current_prompt_block_marker(text).unwrap_or(""),
            Region::AfterCurrentPromptBlockMarker => {
                after_current_prompt_block_marker(text).unwrap_or("")
            }
            Region::PromptBoxBody => prompt_box_body(text).unwrap_or(""),
            Region::AbovePromptBox => above_prompt_box(text),
            Region::LastNonEmptyAbovePromptBox => last_non_empty_line(above_prompt_box(text)),
            Region::AfterLastHorizontalRule => after_last_horizontal_rule(text),
            Region::BottomLines(n) => {
                let all = lines(text);
                from_line(text, &all, all.len().saturating_sub(n))
            }
            Region::BottomNonEmptyLines(n) => bottom_non_empty_lines(text, n),
            Region::TopNonEmptyLines(n) => top_non_empty_lines(text, n),
        }
    }
}

fn lines(text: &str) -> Vec<&str> {
    text.lines().collect()
}

/// Byte offset where line `index` starts.
fn line_start(text: &str, all: &[&str], index: usize) -> usize {
    all[..index.min(all.len())]
        .iter()
        .map(|line| line.len() + 1)
        .sum::<usize>()
        .min(text.len())
}

fn from_line<'a>(text: &'a str, all: &[&str], index: usize) -> &'a str {
    &text[line_start(text, all, index)..]
}

fn up_to_line<'a>(text: &'a str, all: &[&str], index: usize) -> &'a str {
    &text[..line_start(text, all, index)]
}

fn bottom_non_empty_lines(text: &str, n: usize) -> &str {
    let all = lines(text);
    let start = all
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, line)| !line.trim().is_empty())
        .take(n)
        .last()
        .map(|(index, _)| index);
    match start {
        Some(index) => from_line(text, &all, index),
        None => "",
    }
}

fn top_non_empty_lines(text: &str, n: usize) -> &str {
    let all = lines(text);
    let end = all
        .iter()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .take(n)
        .last()
        .map(|(index, _)| index);
    match end {
        Some(index) => up_to_line(text, &all, index + 1),
        None => "",
    }
}

fn is_prompt_line(line: &str) -> bool {
    line == "›" || line.starts_with("› ")
}

fn is_block_marker_line(line: &str) -> bool {
    line.starts_with(['•', '■', '✗', '✓'])
}

/// The last `›` prompt line, if no response marker follows it.
fn current_prompt_index(all: &[&str]) -> Option<usize> {
    let index = all.iter().rposition(|line| is_prompt_line(line))?;
    if all[index + 1..]
        .iter()
        .any(|line| is_block_marker_line(line))
    {
        return None;
    }
    Some(index)
}

fn after_last_prompt_marker(text: &str) -> &str {
    let all = lines(text);
    match all.iter().rposition(|line| is_prompt_line(line)) {
        Some(index) => from_line(text, &all, index + 1),
        None => text,
    }
}

fn before_current_prompt_marker(text: &str) -> &str {
    let all = lines(text);
    match current_prompt_index(&all) {
        Some(index) => up_to_line(text, &all, index),
        None => text,
    }
}

fn current_prompt_block_marker(text: &str) -> Option<&str> {
    let all = lines(text);
    let prompt = current_prompt_index(&all)?;
    all[..prompt]
        .iter()
        .rev()
        .find(|line| is_block_marker_line(line))
        .copied()
}

fn after_current_prompt_block_marker(text: &str) -> Option<&str> {
    let all = lines(text);
    let prompt = current_prompt_index(&all)?;
    let block = all[..prompt]
        .iter()
        .rposition(|line| is_block_marker_line(line))?;
    Some(from_line(text, &all, block))
}

/// A line drawn with `─`: only rule characters, or at least three followed by a label.
fn is_horizontal_rule(line: &str) -> bool {
    let trimmed = line.trim();
    let run = trimmed.chars().take_while(|&ch| ch == '─').count();
    if run == 0 {
        return false;
    }
    let rest = trimmed.chars().skip(run).collect::<String>();
    rest.trim_start().is_empty() || run >= 3
}

/// The prompt box's top border: the second-last rule line.
fn prompt_box_top(all: &[&str]) -> Option<usize> {
    all.iter()
        .enumerate()
        .rev()
        .filter(|(_, line)| is_horizontal_rule(line))
        .nth(1)
        .map(|(index, _)| index)
}

fn prompt_box_body(text: &str) -> Option<&str> {
    let all = lines(text);
    let top = prompt_box_top(&all)?;
    let end = all[top + 1..]
        .iter()
        .position(|line| is_horizontal_rule(line))
        .map(|offset| top + 1 + offset)
        .unwrap_or(all.len());
    Some(&text[line_start(text, &all, top + 1)..line_start(text, &all, end)])
}

fn above_prompt_box(text: &str) -> &str {
    let all = lines(text);
    match prompt_box_top(&all) {
        Some(top) => up_to_line(text, &all, top),
        None => text,
    }
}

fn after_last_horizontal_rule(text: &str) -> &str {
    let all = lines(text);
    match all.iter().rposition(|line| is_horizontal_rule(line)) {
        Some(index) => from_line(text, &all, index + 1),
        None => text,
    }
}

fn last_non_empty_line(text: &str) -> &str {
    text.lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(text: &str) -> Screen<'_> {
        Screen {
            text,
            title: "",
            progress: "",
        }
    }

    fn cut(spec: &str, text: &str) -> String {
        Region::parse(spec)
            .unwrap()
            .slice(&screen(text))
            .to_string()
    }

    #[test]
    fn parses_every_region_the_rules_use() {
        for spec in [
            "osc_title",
            "osc_progress",
            "whole_recent",
            "after_last_prompt_marker",
            "before_current_prompt_marker",
            "whole_recent_without_current_prompt_marker",
            "prompt_box_body",
            "last_non_empty_above_prompt_box",
            "after_last_horizontal_rule",
            "bottom_non_empty_lines(12)",
            "top_non_empty_lines(20)",
            "bottom_lines(3)",
        ] {
            assert!(Region::parse(spec).is_ok(), "{spec}");
        }
        assert!(Region::parse("bottom_non_empty_lines(0)").is_err());
        assert!(Region::parse("bottom_lines(x)").is_err());
        assert!(Region::parse("middle").is_err());
    }

    #[test]
    fn bottom_and_top_skip_empty_lines() {
        let text = "a\n\nb\nc\n\nd";
        assert_eq!(cut("bottom_non_empty_lines(2)", text), "c\n\nd");
        assert_eq!(cut("top_non_empty_lines(2)", text), "a\n\nb\n");
        assert_eq!(cut("bottom_lines(2)", text), "\nd");
    }

    #[test]
    fn prompt_box_regions() {
        let text =
            "Working on it\nlast thought\n────────\n> type here\n────────\n  ? for shortcuts";
        assert_eq!(cut("prompt_box_body", text), "> type here\n");
        assert_eq!(cut("last_non_empty_above_prompt_box", text), "last thought");
        assert_eq!(cut("after_last_horizontal_rule", text), "  ? for shortcuts");
    }

    #[test]
    fn codex_prompt_markers() {
        let text = "• Ran tests\n  ok\n› fix the bug\n  esc to interrupt";
        assert_eq!(cut("after_last_prompt_marker", text), "  esc to interrupt");
        assert_eq!(
            cut("before_current_prompt_marker", text),
            "• Ran tests\n  ok\n"
        );
        assert_eq!(cut("whole_recent_without_current_prompt_marker", text), "");
        assert_eq!(cut("current_prompt_block_marker", text), "• Ran tests");
        // A response after the prompt makes that prompt stale.
        let stale = "› fix the bug\n• Done";
        assert_eq!(
            cut("whole_recent_without_current_prompt_marker", stale),
            stale
        );
    }

    #[test]
    fn labelled_rule_needs_three_dashes() {
        assert!(is_horizontal_rule("──── Working ────"));
        assert!(!is_horizontal_rule("─ a"));
        assert!(is_horizontal_rule("─"));
    }
}
