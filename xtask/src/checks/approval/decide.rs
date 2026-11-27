//! What `approval` does with a protected pull request: a verdict, and the changes to make on
//! GitHub (`docs/specs/phases/P00-foundation-and-gate.md` §5.7).
//!
//! [`decide`] is a pure function of what was found: the protected paths touched, the event, the
//! labels as they are now, the fingerprint recorded when Jann added `approved`, and the current
//! one. It returns the verdict and the [`Action`]s to run, in order, and runs nothing itself.
//! Rows are tried from the top and the first that fits decides:
//!
//! 1. nothing protected is touched: pass (`check` already stops there, before it reads GitHub);
//! 2. the PR lacks `approved`: fail, "waits for Jann";
//! 3. `approved` was just added: record the fingerprint in a comment, pass;
//! 4. the recorded fingerprint is the current one: pass;
//! 5. a fingerprint is recorded and differs from the current one (5a): take `approved` off, say so
//!    in a comment, fail. None is recorded (5b): fail, and change nothing but the `needs-jann`
//!    label.
//!
//! Every row but the first also makes sure `needs-jann` exists and is on the PR. CI never adds
//! `approved` and never removes `needs-jann`.
//!
//! Only the run of the event `labeled approved` records (row 3). Row 5b takes no action because
//! GitHub starts a run for every event, in any order: when Jann changes two labels at once, the
//! run of another event may read `approved` before the run of `labeled approved` has recorded
//! it, and removing the label then would undo the approval that run is about to record. The check
//! still never passes without a recorded fingerprint equal to the current one.
//!
//! A recorded fingerprint is read back with [`recorded`] from comments of the bot that were never
//! edited, because everyone else, Claude included, writes comments under Jann's account. Nothing
//! that a pull request wrote is put into a verdict or a comment: they hold our own text and the
//! fingerprints and the head, which are hex.

use super::fingerprint::Fingerprint;
use crate::ci::PullRequestEvent;
use crate::github::Comment;
use crate::verdict::{Check, Finding, Verdict};

/// The start of the line that records a fingerprint in a comment.
pub const MARKER: &str = "<!-- theos-gate:approval";

/// The login of the account whose comments `recorded` trusts: the Actions workflow token.
pub const BOT: &str = "github-actions[bot]";

/// The sentence that follows the marker line, for the person who reads the comment.
const RECORDED_SENTENCE: &str = "Approval recorded for this content of the PR.";

/// The failure of a protected pull request without the label `approved`.
const WAITS: &str = "waits for Jann: add the label `approved` after review";

/// The failure of a pull request that has `approved` but no recorded approval. `docs/gate.md` §5
/// words only the changed case.
const NOT_RECORDED: &str = "no approval is recorded for this content yet: the run of the label `approved` records it; if that run has finished and this check is still red, remove `approved` and add it again";

/// Added by CI to a pull request that touches a protected path; never removed by CI. The colour
/// and the description of the label are in `gate/repo-settings.toml`.
pub const NEEDS_JANN: &str = "needs-jann";

/// Added by Jann after review; the only label that CI removes. The colour and the description of
/// the label are in `gate/repo-settings.toml`.
pub const APPROVED: &str = "approved";

/// What happened to the pull request, as far as `approval` cares.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Event {
    /// A label was added.
    Labeled(String),
    /// Anything else: a push, an edit, a re-run.
    Other,
}

impl Event {
    /// The event of a `pull_request` payload: `labeled` with its label, else `Other`.
    #[must_use]
    pub fn from_pull_request(pr: &PullRequestEvent) -> Self {
        match (pr.action.as_str(), &pr.label) {
            ("labeled", Some(label)) => Self::Labeled(label.clone()),
            _ => Self::Other,
        }
    }
}

/// One change `approval` makes on GitHub.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Action {
    /// Make sure the repository has the label with this name, looking as
    /// `gate/repo-settings.toml` says.
    EnsureLabel(String),
    /// Put the label on the pull request.
    AddLabel(String),
    /// Take the label off the pull request.
    RemoveLabel(String),
    /// Write this comment.
    Comment(String),
}

/// The comment that records `fingerprint` for the pull request at `head`: the marker line, then
/// a sentence. `fingerprint` and `head` are hex.
#[must_use]
pub fn marker(fingerprint: &Fingerprint, head: &str) -> String {
    format!(
        "{MARKER} fingerprint={} head={head} -->\n\n{RECORDED_SENTENCE}",
        fingerprint.0
    )
}

/// The fingerprint recorded for the pull request: the one in the newest comment that has the
/// marker, was written by [`BOT`] and was never edited (`created_at == updated_at`).
///
/// Newest is by `created_at`, and by position in `comments` when two share a time. A newest
/// comment whose marker holds no fingerprint gives `None`; an older comment is not used instead.
#[must_use]
pub fn recorded(comments: &[Comment]) -> Option<Fingerprint> {
    let (_, newest) = comments
        .iter()
        .enumerate()
        .filter(|(_, comment)| {
            comment.author == BOT
                && comment.created_at == comment.updated_at
                && comment.body.contains(MARKER)
        })
        .max_by(|(first_index, first), (second_index, second)| {
            first
                .created_at
                .cmp(&second.created_at)
                .then(first_index.cmp(second_index))
        })?;
    fingerprint_of(&newest.body)
}

/// The fingerprint in the marker line of `body`: `fingerprint=<hex>` between the marker and the
/// `-->` that closes it.
fn fingerprint_of(body: &str) -> Option<Fingerprint> {
    let (_, after_marker) = body.split_once(MARKER)?;
    let line = after_marker.split("-->").next()?;
    line.split_whitespace()
        .find_map(|word| word.strip_prefix("fingerprint="))
        .and_then(Fingerprint::parse)
}

/// Decides the verdict and the actions for a pull request. See the module documentation for the
/// rows.
///
/// `touched` are the protected paths the pull request changes, `labels` its labels now, `recorded`
/// the fingerprint from [`recorded`], `current` the fingerprint now and `head` the commit it was
/// computed for.
#[must_use]
pub fn decide(
    touched: &[String],
    event: &Event,
    labels: &[String],
    recorded: Option<&Fingerprint>,
    current: &Fingerprint,
    head: &str,
) -> (Verdict, Vec<Action>) {
    // Row 1.
    if touched.is_empty() {
        return (Verdict::pass(Check::Approval), Vec::new());
    }
    let has = |name: &str| labels.iter().any(|label| label == name);
    let mut actions = Vec::new();
    if !has(NEEDS_JANN) {
        actions.push(Action::EnsureLabel(NEEDS_JANN.to_owned()));
        actions.push(Action::AddLabel(NEEDS_JANN.to_owned()));
    }
    // Row 2: the labels are the ones read just now, so a label already undone is not here.
    if !has(APPROVED) {
        return (fail(WAITS), actions);
    }
    // Row 3.
    if matches!(event, Event::Labeled(label) if label == APPROVED) {
        actions.push(Action::Comment(marker(current, head)));
        return (Verdict::pass(Check::Approval), actions);
    }
    // Row 4.
    if recorded == Some(current) {
        return (Verdict::pass(Check::Approval), actions);
    }
    // Row 5b: nothing recorded. Not a pass, but `approved` stays and nothing is written: the run
    // of the `labeled approved` event may still be to come and records it.
    let Some(old) = recorded else {
        return (fail(NOT_RECORDED), actions);
    };
    // Row 5a.
    let reason = format!(
        "approval removed: the PR changed after approval ({} → {})",
        old.0, current.0
    );
    actions.push(Action::RemoveLabel(APPROVED.to_owned()));
    actions.push(Action::Comment(reason.clone()));
    (fail(&reason), actions)
}

fn fail(message: &str) -> Verdict {
    Verdict::with(Check::Approval, vec![Finding::new(message)])
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "7d646e6b3c1a90f2e48d5b7a6c0e1f39a2d84b57";

    fn fp(digit: char) -> Fingerprint {
        Fingerprint(digit.to_string().repeat(64))
    }

    fn touched() -> Vec<String> {
        vec!["docs/gate.md".to_owned()]
    }

    fn labels(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    fn labeled(name: &str) -> Event {
        Event::Labeled(name.to_owned())
    }

    fn needs_jann_actions() -> Vec<Action> {
        vec![
            Action::EnsureLabel("needs-jann".to_owned()),
            Action::AddLabel("needs-jann".to_owned()),
        ]
    }

    fn messages(verdict: &Verdict) -> Vec<&str> {
        verdict
            .findings
            .iter()
            .map(|finding| finding.message.as_str())
            .collect()
    }

    fn comment(author: &str, body: &str, created: &str, updated: &str) -> Comment {
        Comment {
            author: author.to_owned(),
            body: body.to_owned(),
            created_at: created.to_owned(),
            updated_at: updated.to_owned(),
        }
    }

    fn bot_marker(digit: char, time: &str) -> Comment {
        comment(BOT, &marker(&fp(digit), HEAD), time, time)
    }

    // AC: TU-00.4
    #[test]
    fn unprotected_pr_passes() {
        for event in [Event::Other, labeled("approved")] {
            let (verdict, actions) = decide(&[], &event, &[], None, &fp('a'), HEAD);
            assert_eq!(verdict, Verdict::pass(Check::Approval));
            assert_eq!(actions, Vec::<Action>::new());
        }
    }

    // AC: TU-00.4
    #[test]
    fn protected_without_label_fails_and_adds_needs_jann() {
        let (verdict, actions) = decide(&touched(), &Event::Other, &[], None, &fp('a'), HEAD);
        assert!(!verdict.passed());
        assert_eq!(verdict.check, Check::Approval);
        assert_eq!(
            messages(&verdict),
            ["waits for Jann: add the label `approved` after review"]
        );
        assert_eq!(actions, needs_jann_actions());

        // With `needs-jann` on the PR already there is nothing to add; the verdict is the same.
        let (verdict, actions) = decide(
            &touched(),
            &Event::Other,
            &labels(&["needs-jann", "bug"]),
            None,
            &fp('a'),
            HEAD,
        );
        assert_eq!(
            messages(&verdict),
            ["waits for Jann: add the label `approved` after review"]
        );
        assert_eq!(actions, Vec::<Action>::new());
    }

    // AC: TU-00.4
    #[test]
    fn adding_approved_records_fingerprint_and_passes() {
        let (verdict, actions) = decide(
            &touched(),
            &labeled("approved"),
            &labels(&["needs-jann", "approved"]),
            None,
            &fp('a'),
            HEAD,
        );
        assert_eq!(verdict, Verdict::pass(Check::Approval));
        assert_eq!(actions, [Action::Comment(marker(&fp('a'), HEAD))]);

        // A recorded fingerprint of an earlier approval does not matter: this one is recorded.
        let (verdict, actions) = decide(
            &touched(),
            &labeled("approved"),
            &labels(&["needs-jann", "approved"]),
            Some(&fp('b')),
            &fp('a'),
            HEAD,
        );
        assert!(verdict.passed());
        assert_eq!(actions, [Action::Comment(marker(&fp('a'), HEAD))]);

        // `needs-jann` comes first when the PR does not have it.
        let (_, actions) = decide(
            &touched(),
            &labeled("approved"),
            &labels(&["approved"]),
            None,
            &fp('a'),
            HEAD,
        );
        let mut expected = needs_jann_actions();
        expected.push(Action::Comment(marker(&fp('a'), HEAD)));
        assert_eq!(actions, expected);
    }

    // AC: TU-00.4
    #[test]
    fn approved_with_same_fingerprint_passes() {
        for event in [Event::Other, labeled("macos")] {
            let (verdict, actions) = decide(
                &touched(),
                &event,
                &labels(&["needs-jann", "approved"]),
                Some(&fp('a')),
                &fp('a'),
                HEAD,
            );
            assert_eq!(verdict, Verdict::pass(Check::Approval));
            assert_eq!(actions, Vec::<Action>::new());
        }
    }

    // AC: TU-00.4
    #[test]
    fn changed_fingerprint_removes_approval_and_fails() {
        let (verdict, actions) = decide(
            &touched(),
            &Event::Other,
            &labels(&["needs-jann", "approved"]),
            Some(&fp('b')),
            &fp('a'),
            HEAD,
        );
        let reason = format!(
            "approval removed: the PR changed after approval ({} → {})",
            "b".repeat(64),
            "a".repeat(64)
        );
        assert_eq!(messages(&verdict), [reason.as_str()]);
        assert_eq!(
            actions,
            [
                Action::RemoveLabel("approved".to_owned()),
                Action::Comment(reason)
            ]
        );
    }

    // AC: TU-00.4
    #[test]
    fn approved_without_recorded_fingerprint_fails() {
        let (verdict, actions) = decide(
            &touched(),
            &Event::Other,
            &labels(&["approved"]),
            None,
            &fp('a'),
            HEAD,
        );
        // Still a failure, but `approved` stays and nothing is written: the run of the label
        // `approved` may be about to record the approval.
        assert_eq!(messages(&verdict), [NOT_RECORDED]);
        assert_eq!(actions, needs_jann_actions());
    }

    // AC: TU-00.4
    #[test]
    fn a_run_of_another_event_does_not_undo_the_approval_the_labeled_run_records() {
        // Jann removes `needs-jann` and adds `approved` in the same second. The run of the
        // `unlabeled` event goes first and already sees `approved`; nothing is recorded yet.
        let (verdict, actions) = decide(
            &touched(),
            &Event::Other,
            &labels(&["approved"]),
            None,
            &fp('a'),
            HEAD,
        );
        assert!(!verdict.passed());
        assert_eq!(actions, needs_jann_actions());

        // Then the run of the `labeled approved` event records and passes.
        let (verdict, actions) = decide(
            &touched(),
            &labeled("approved"),
            &labels(&["approved", "needs-jann"]),
            None,
            &fp('a'),
            HEAD,
        );
        assert_eq!(verdict, Verdict::pass(Check::Approval));
        assert_eq!(actions, [Action::Comment(marker(&fp('a'), HEAD))]);
    }

    // AC: TU-00.4
    #[test]
    fn labeled_event_with_label_already_removed_fails() {
        let (verdict, actions) = decide(
            &touched(),
            &labeled("approved"),
            &labels(&["needs-jann"]),
            Some(&fp('a')),
            &fp('a'),
            HEAD,
        );
        assert_eq!(
            messages(&verdict),
            ["waits for Jann: add the label `approved` after review"]
        );
        assert_eq!(actions, Vec::<Action>::new());
    }

    #[test]
    fn the_verdict_is_the_one_of_the_approval_check_and_has_no_file() {
        let (verdict, _) = decide(&touched(), &Event::Other, &[], None, &fp('a'), HEAD);
        assert_eq!(verdict.check, Check::Approval);
        assert_eq!(verdict.findings, [Finding::new(WAITS)]);
        assert_eq!(verdict.notes, Vec::<String>::new());
    }

    #[test]
    fn no_text_of_the_pr_reaches_a_verdict_or_a_comment() {
        // Paths are the pull request's text. Neither the verdict nor an action may hold one.
        let touched = vec!["docs/::error::forged\n::stop-commands::x.md".to_owned()];
        for (labels, recorded, event) in [
            (labels(&[]), None, Event::Other),
            (labels(&["approved"]), None, Event::Other),
            (labels(&["approved"]), Some(fp('b')), Event::Other),
            (labels(&["approved"]), None, labeled("approved")),
        ] {
            let (verdict, actions) =
                decide(&touched, &event, &labels, recorded.as_ref(), &fp('a'), HEAD);
            let mut text = messages(&verdict).join("\n");
            for action in &actions {
                if let Action::Comment(body) = action {
                    text.push_str(body);
                }
            }
            assert!(!text.contains("forged"), "{text}");
            assert!(!text.contains("docs/"), "{text}");
        }
    }

    #[test]
    fn the_gate_labels_are_the_ones_the_workflows_and_the_settings_name() {
        assert_eq!(NEEDS_JANN, "needs-jann");
        assert_eq!(APPROVED, "approved");
    }

    #[test]
    fn the_event_is_labeled_only_for_a_labeled_action_with_a_label() {
        let pr = |action: &str, label: Option<&str>| PullRequestEvent {
            action: action.to_owned(),
            number: 7,
            title: String::new(),
            body: None,
            base_sha: String::new(),
            head_sha: String::new(),
            head_ref: String::new(),
            label: label.map(str::to_owned),
        };
        assert_eq!(
            Event::from_pull_request(&pr("labeled", Some("approved"))),
            labeled("approved")
        );
        assert_eq!(
            Event::from_pull_request(&pr("labeled", Some("macos"))),
            labeled("macos")
        );
        assert_eq!(Event::from_pull_request(&pr("labeled", None)), Event::Other);
        assert_eq!(
            Event::from_pull_request(&pr("unlabeled", Some("approved"))),
            Event::Other
        );
        assert_eq!(
            Event::from_pull_request(&pr("synchronize", None)),
            Event::Other
        );
    }

    #[test]
    fn the_marker_names_the_fingerprint_and_the_head() {
        let text = marker(&fp('a'), HEAD);
        assert_eq!(
            text,
            format!(
                "<!-- theos-gate:approval fingerprint={} head={HEAD} -->\n\nApproval recorded for \
                 this content of the PR.",
                "a".repeat(64)
            )
        );
        assert!(text.starts_with(MARKER));
    }

    #[test]
    fn a_comment_the_bot_wrote_is_read_back() {
        let comments = [bot_marker('a', "2026-10-31T10:00:00Z")];
        assert_eq!(recorded(&comments), Some(fp('a')));
        assert_eq!(recorded(&[]), None);
    }

    // AC: TU-00.4
    #[test]
    fn recorded_ignores_comments_not_by_bot() {
        let forged = |author: &str| {
            comment(
                author,
                &marker(&fp('f'), HEAD),
                "2026-10-31T11:00:00Z",
                "2026-10-31T11:00:00Z",
            )
        };
        let comments = [
            bot_marker('a', "2026-10-31T10:00:00Z"),
            forged("jann"),
            forged("github-actions"),
            forged("github-actions[bot] "),
            forged("GitHub-Actions[bot]"),
            forged(""),
        ];
        assert_eq!(recorded(&comments), Some(fp('a')));
        assert_eq!(recorded(&comments[1..]), None);
    }

    // AC: TU-00.4
    #[test]
    fn recorded_ignores_edited_comments() {
        let mut edited = bot_marker('f', "2026-10-31T11:00:00Z");
        edited.updated_at = "2026-10-31T11:00:01Z".to_owned();
        let comments = [bot_marker('a', "2026-10-31T10:00:00Z"), edited];
        assert_eq!(recorded(&comments), Some(fp('a')));
        assert_eq!(recorded(&comments[1..]), None);
    }

    // AC: TU-00.4
    #[test]
    fn recorded_takes_newest() {
        let old = bot_marker('a', "2026-10-31T10:00:00Z");
        let new = bot_marker('b', "2026-10-31T11:00:00Z");
        assert_eq!(recorded(&[old.clone(), new.clone()]), Some(fp('b')));
        // The order of the list does not decide, the time does.
        assert_eq!(recorded(&[new.clone(), old.clone()]), Some(fp('b')));
        // Two at the same time: the one listed last.
        let twin = bot_marker('c', "2026-10-31T11:00:00Z");
        assert_eq!(recorded(&[new, twin, old]), Some(fp('c')));
    }

    #[test]
    fn recorded_skips_comments_without_the_marker_such_as_the_removal_notice() {
        let (_, actions) = decide(
            &touched(),
            &Event::Other,
            &labels(&["approved"]),
            Some(&fp('b')),
            &fp('a'),
            HEAD,
        );
        let Some(Action::Comment(notice)) = actions.last() else {
            panic!("expected a comment, got {actions:?}");
        };
        let comments = [
            bot_marker('b', "2026-10-31T10:00:00Z"),
            comment(BOT, notice, "2026-10-31T11:00:00Z", "2026-10-31T11:00:00Z"),
            comment(
                BOT,
                "Thanks!",
                "2026-10-31T12:00:00Z",
                "2026-10-31T12:00:00Z",
            ),
        ];
        assert_eq!(recorded(&comments), Some(fp('b')));
    }

    #[test]
    fn a_newest_marker_without_a_fingerprint_is_no_record() {
        let at = "2026-10-31T11:00:00Z";
        let broken = |body: &str| comment(BOT, body, at, at);
        for body in [
            format!("{MARKER} head={HEAD} -->"),
            format!("{MARKER} fingerprint= head={HEAD} -->"),
            format!("{MARKER} fingerprint=XYZ head={HEAD} -->"),
            format!("{MARKER} fingerprint={} head={HEAD} -->", "A".repeat(64)),
            format!("{MARKER} fingerprint={}", "a".repeat(63)),
            // The fingerprint has to be inside the marker comment.
            format!("{MARKER} head={HEAD} -->\nfingerprint={}", "a".repeat(64)),
        ] {
            let comments = [bot_marker('a', "2026-10-31T10:00:00Z"), broken(&body)];
            assert_eq!(recorded(&comments), None, "{body}");
        }
    }
}
