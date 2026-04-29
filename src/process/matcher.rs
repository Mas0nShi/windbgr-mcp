//! Module-name matcher used by `process_find_by_module`.
//!
//! Matching is intentionally lenient and case-insensitive so that callers
//! can pass either a bare DLL name (`ntdll.dll`) or any substring of the
//! full module path. The strategy is centralised here so the rule set is
//! single-sourced and unit-tested instead of being scattered across the
//! call site.

/// Normalised representation of a single user-supplied pattern.
#[derive(Debug, Clone)]
pub struct ModulePattern {
    /// The original (trimmed) text — preserved for echoing back to the
    /// caller in match results.
    pub original: String,
    /// Lower-cased form used for matching.
    pub needle: String,
}

impl ModulePattern {
    /// Parse a single pattern. Returns `None` for empty/whitespace input.
    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(Self {
            original: trimmed.to_string(),
            needle: trimmed.to_lowercase(),
        })
    }

    /// True when `module_name` (case-insensitive) matches this pattern.
    pub fn matches_name(&self, module_name_lc: &str) -> bool {
        module_name_lc == self.needle || module_name_lc.contains(&self.needle)
    }

    /// True when the full module path (lower-cased) matches this pattern.
    pub fn matches_path(&self, module_path_lc: &str) -> bool {
        module_path_lc.ends_with(&self.needle) || module_path_lc.contains(&self.needle)
    }
}

/// Collection of patterns prepared for the per-module hot loop.
#[derive(Debug, Clone, Default)]
pub struct ModuleMatcher {
    patterns: Vec<ModulePattern>,
}

impl ModuleMatcher {
    pub fn from_raw<I, S>(raw: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let patterns = raw
            .into_iter()
            .filter_map(|s| ModulePattern::parse(s.as_ref()))
            .collect();
        Self { patterns }
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// Return the first pattern that matches the supplied module name/path
    /// (both expected to already be lower-cased by the caller for the hot
    /// loop). Use this in the discovery hot path to keep allocations down.
    pub fn first_match<'a>(
        &'a self,
        module_name_lc: &str,
        module_path_lc: &str,
    ) -> Option<&'a ModulePattern> {
        self.patterns
            .iter()
            .find(|p| p.matches_name(module_name_lc) || p.matches_path(module_path_lc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_yields_empty_matcher() {
        let m = ModuleMatcher::from_raw(["", "  "]);
        assert!(m.is_empty());
    }

    #[test]
    fn matches_exact_name_case_insensitive() {
        let m = ModuleMatcher::from_raw(["NTDLL.DLL"]);
        assert!(m
            .first_match("ntdll.dll", "c:/windows/system32/ntdll.dll")
            .is_some());
    }

    #[test]
    fn matches_path_suffix() {
        let m = ModuleMatcher::from_raw(["system32/ntdll.dll"]);
        assert!(m
            .first_match("ntdll.dll", "c:/windows/system32/ntdll.dll")
            .is_some());
    }

    #[test]
    fn returns_first_matching_pattern() {
        let m = ModuleMatcher::from_raw(["ntdll", "kernelbase"]);
        let hit = m
            .first_match("ntdll.dll", "c:/windows/system32/ntdll.dll")
            .unwrap();
        assert_eq!(hit.original, "ntdll");
    }

    #[test]
    fn no_match_returns_none() {
        let m = ModuleMatcher::from_raw(["foobar"]);
        assert!(m.first_match("ntdll.dll", "c:/x/ntdll.dll").is_none());
    }
}
