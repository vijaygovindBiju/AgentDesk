//! Pure classification of a raw event's `kind` into the four attention
//! categories plus a severity and a one-line summary. Rules are data so a
//! future adapter can extend them without touching this logic.
//! See docs/EVENT_MODEL.md "Categories" and "Classification".

use agentdesk_model::{Category, Severity};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule {
    pub kind: &'static str,
    pub category: Category,
    pub severity: Severity,
    /// Level-1 summary shown on the phone. Cancellations always say
    /// "Cancelled" so the Completed section cannot read as success.
    pub summary: &'static str,
}

const fn rule(kind: &'static str, category: Category, severity: Severity, summary: &'static str) -> Rule {
    Rule { kind, category, severity, summary }
}

use Category::*;
use Severity::*;

/// The classification table. Order is irrelevant; kinds must be unique.
pub const RULES: &[Rule] = &[
    // Requests: agent is blocked on a human decision.
    rule("approval_required", Request, Critical, "Approval Required"),
    rule("input_required", Request, Critical, "Input Required"),
    rule("credential_required", Request, Critical, "Credential Required"),
    // Errors: something failed or stopped unexpectedly.
    rule("build_failed", Error, Critical, "Build Failed"),
    rule("command_failed", Error, Critical, "Command Failed"),
    rule("adapter_error", Error, Critical, "Adapter Error"),
    rule("test_failed", Error, Important, "Tests Failed"),
    rule("cancelled_by_agent", Error, Important, "Cancelled"),
    rule("aborted", Error, Important, "Aborted"),
    // Completed: terminal state — success or an expected stop.
    rule("build_completed", Completed, Important, "Build Completed"),
    rule("task_completed", Completed, Important, "Task Completed"),
    rule("tests_passed", Completed, Notable, "Tests Passed"),
    rule("install_completed", Completed, Notable, "Install Completed"),
    rule("cancelled_by_user", Completed, Notable, "Cancelled"),
    rule("cancelled", Completed, Notable, "Cancelled"),
    // Working: progress; normally no attention.
    rule("started", Working, Notable, "Started"),
    rule("waiting", Working, Notable, "Waiting"),
    rule("progress", Working, Routine, "Working"),
    rule("installing", Working, Routine, "Installing"),
];

/// How a kind was matched; `Fallback` is counted as `unclassified_events`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Matched {
    Exact,
    /// `*_failed` or `*_completed` suffix rule.
    Suffix,
    /// Unknown kind; classified as `working/0`.
    Fallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub category: Category,
    pub severity: Severity,
    pub summary: String,
    pub matched: Matched,
}

/// Classify by `kind`. Never fails, never panics.
pub fn classify(kind: &str) -> Classification {
    if let Some(r) = RULES.iter().find(|r| r.kind == kind) {
        return Classification { category: r.category, severity: r.severity, summary: r.summary.to_string(), matched: Matched::Exact };
    }
    if kind.ends_with("_failed") {
        return Classification { category: Error, severity: Critical, summary: humanize(kind), matched: Matched::Suffix };
    }
    if kind.ends_with("_completed") {
        return Classification { category: Completed, severity: Important, summary: humanize(kind), matched: Matched::Suffix };
    }
    Classification { category: Working, severity: Routine, summary: humanize(kind), matched: Matched::Fallback }
}

/// `build_failed` → `Build Failed`.
fn humanize(kind: &str) -> String {
    kind.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_rule_row_classifies_exactly() {
        for r in RULES {
            let c = classify(r.kind);
            assert_eq!((c.category, c.severity, c.summary.as_str(), c.matched), (r.category, r.severity, r.summary, Matched::Exact), "kind {}", r.kind);
        }
    }

    #[test]
    fn rule_kinds_are_unique() {
        let mut seen = HashSet::new();
        for r in RULES {
            assert!(seen.insert(r.kind), "duplicate rule for {}", r.kind);
        }
    }

    #[test]
    fn cancellation_is_split_by_cause() {
        assert_eq!((classify("cancelled_by_user").category, classify("cancelled_by_user").severity), (Completed, Notable));
        assert_eq!((classify("cancelled_by_agent").category, classify("cancelled_by_agent").severity), (Error, Important));
        assert_eq!((classify("aborted").category, classify("aborted").severity), (Error, Important));
        assert_eq!((classify("cancelled").category, classify("cancelled").severity), (Completed, Notable));
        for k in ["cancelled_by_user", "cancelled_by_agent", "cancelled"] {
            assert_eq!(classify(k).summary, "Cancelled", "{k} must never read as success");
        }
    }

    #[test]
    fn suffix_fallbacks() {
        let f = classify("foo_failed");
        assert_eq!((f.category, f.severity, f.matched), (Error, Critical, Matched::Suffix));
        assert_eq!(f.summary, "Foo Failed");
        let c = classify("deploy_completed");
        assert_eq!((c.category, c.severity, c.matched), (Completed, Important, Matched::Suffix));
    }

    #[test]
    fn unknown_kind_is_working_routine_and_never_panics() {
        for k in ["teleport", "", "___", "FAILED", "completed_x"] {
            let c = classify(k);
            assert_eq!((c.category, c.severity, c.matched), (Working, Routine, Matched::Fallback), "{k:?}");
        }
        assert_eq!(classify("").summary, "");
    }

    #[test]
    fn requests_are_always_critical() {
        for r in RULES.iter().filter(|r| r.category == Request) {
            assert_eq!(r.severity, Critical, "{}", r.kind);
        }
    }
}
