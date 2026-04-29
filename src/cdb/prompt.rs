//! Prompt detection and output cleaning for the cdb REPL.
//!
//! The cdb prompt looks like `0:000> `, `1:1:012> `, or `0:000:x86> `. When
//! a command produces no visible output cdb emits the next prompt directly
//! after the previous one (no newline), so the regex must also tolerate
//! `>\s` as a leading anchor.

use once_cell::sync::Lazy;
use regex::Regex;

static PROMPT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?m)(^|\r\n|\n|>\s)\d+:[0-9a-fA-F]+(:[0-9a-fA-F]+)?>\s*\z").unwrap());

#[derive(Debug, Clone, Copy)]
pub struct PromptMatch {
    pub start: usize,
    pub end: usize,
}

/// Locate the position of a trailing cdb prompt.
pub fn find_prompt(tail: &str) -> Option<PromptMatch> {
    let m = PROMPT_RE.find(tail)?;
    Some(PromptMatch {
        start: m.start(),
        end: m.end(),
    })
}

/// Find the byte offset of the *last* prompt that ends at the end of the
/// supplied string.
fn rfind_prompt(s: &str) -> Option<usize> {
    PROMPT_RE
        .find_iter(s)
        .filter(|m| m.end() == s.len())
        .map(|m| m.start())
        .last()
}

/// Strip the trailing prompt and the leading echo of the issued command
/// from raw cdb output.
pub fn clean_command_output(raw: &str, command: &str) -> String {
    let trimmed_end = raw.rfind('\n').unwrap_or(raw.len());
    let mut content = if let Some(pos) = rfind_prompt(raw) {
        raw[..pos].to_string()
    } else {
        raw[..trimmed_end.min(raw.len())].to_string()
    };
    let echo_candidates = [
        format!("{command}\r\n"),
        format!("{command}\n"),
        command.to_string(),
    ];
    let content_trimmed = content.trim_start_matches(['\r', '\n']);
    for c in &echo_candidates {
        if let Some(rest) = content_trimmed.strip_prefix(c.as_str()) {
            content = rest.to_string();
            break;
        }
    }
    content.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_basic_prompt() {
        let tail = "eax=1234\nebx=5678\n0:000> ";
        assert!(find_prompt(tail).is_some());
    }

    #[test]
    fn detects_multi_digit_prompt() {
        let tail = "output\n12:1f:42> ";
        assert!(find_prompt(tail).is_some());
    }

    #[test]
    fn no_prompt_when_missing() {
        let tail = "just some output\nno prompt";
        assert!(find_prompt(tail).is_none());
    }

    #[test]
    fn clean_command_output_strips_prompt_and_echo() {
        let raw = "r\neax=00000000 ebx=00000000\n0:000> ";
        let out = clean_command_output(raw, "r");
        assert!(out.contains("eax"));
        assert!(!out.contains("0:000>"));
    }

    /// Regression test for the "bp produces no output" bug: when a cdb
    /// command emits no visible output, cdb sends the next prompt directly
    /// after the previous one without a newline, producing back-to-back
    /// prompts like `0:023> 0:023> `. The prompt regex must detect this.
    #[test]
    fn detects_back_to_back_prompts() {
        let tail = "ntdll!DbgBreakPoint\n0:023> 0:023> ";
        assert!(
            find_prompt(tail).is_some(),
            "must detect prompt in back-to-back scenario"
        );
    }

    #[test]
    fn detects_prompt_after_crlf() {
        let tail = "some output\r\n0:000> ";
        assert!(find_prompt(tail).is_some());
    }

    #[test]
    fn detects_standalone_prompt() {
        let tail = "0:000> ";
        assert!(find_prompt(tail).is_some());
    }

    #[test]
    fn rfind_prompt_on_bare_prompt() {
        let raw = "0:023> ";
        assert_eq!(rfind_prompt(raw), Some(0));
    }

    #[test]
    fn rfind_prompt_on_back_to_back() {
        let raw = "0:023> 0:023> ";
        let pos = rfind_prompt(raw).unwrap();
        assert_eq!(&raw[pos..pos + 1], ">");
    }

    /// The exact buffer tail observed in the stuck-session bug.
    #[test]
    fn regression_smsecurityexchangeinfo_scenario() {
        let tail = concat!(
            "(18cc.2a5c): Break instruction exception - code 80000003 (first chance)\n",
            "ntdll!DbgBreakPoint:\n",
            "00007ff8`e5d94af0 cc              int     3\n",
            "0:023> 00007ff8`41422c78 someDll!func1 ",
            "(unsigned char __cdecl func1(struct tagDATA *,",
            "void *,unsigned int))\n",
            "0:023> 0:023> ",
        );
        assert!(find_prompt(tail).is_some());
    }
}
