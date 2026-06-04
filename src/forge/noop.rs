//! Null-object adapters: implement [`Forge`]/[`Tracker`] with no side effects,
//! returning canned `(dry-run)` refs. Used to render a `--dry-run` preview (and
//! as a stand-in in tests) without branching at the call site.

use super::{
    CiStatus, Forge, ForgeError, IssueRef, IssueState, PrDraft, PrRef, PrState, Tracker, Transition,
};

/// A no-op forge: every call succeeds and mutates nothing.
pub struct NoopForge;

impl Forge for NoopForge {
    fn open_pr(&self, draft: &PrDraft) -> Result<PrRef, ForgeError> {
        Ok(PrRef {
            id: "0".to_string(),
            url: "(dry-run)".to_string(),
            branch: draft.branch.clone(),
        })
    }
    fn pr_state(&self, _pr: &str) -> Result<PrState, ForgeError> {
        Ok(PrState {
            approvals: 0,
            ci: CiStatus::None,
            merged: false,
            draft: true,
        })
    }
    fn merge_pr(&self, _pr: &str) -> Result<(), ForgeError> {
        Ok(())
    }
    fn close_pr(&self, _pr: &str) -> Result<(), ForgeError> {
        Ok(())
    }
    fn comment_pr(&self, _pr: &str, _body: &str) -> Result<(), ForgeError> {
        Ok(())
    }
    fn set_pr_body(&self, _pr: &str, _body: &str) -> Result<(), ForgeError> {
        Ok(())
    }
    fn describe(&self) -> String {
        "noop:forge".to_string()
    }
}

/// A no-op tracker: every call succeeds and mutates nothing.
pub struct NoopTracker;

impl Tracker for NoopTracker {
    fn create_issue(&self, title: &str, _body: &str) -> Result<IssueRef, ForgeError> {
        Ok(IssueRef {
            id: "0".to_string(),
            url: "(dry-run)".to_string(),
            title: title.to_string(),
        })
    }
    fn transition(&self, _issue: &str, _to: Transition) -> Result<(), ForgeError> {
        Ok(())
    }
    fn close_issue(&self, _issue: &str) -> Result<(), ForgeError> {
        Ok(())
    }
    fn comment_issue(&self, _issue: &str, _body: &str) -> Result<(), ForgeError> {
        Ok(())
    }
    fn issue_state(&self, _issue: &str) -> Result<IssueState, ForgeError> {
        Ok(IssueState {
            open: true,
            url: "(dry-run)".to_string(),
        })
    }
    fn describe(&self) -> String {
        "noop:tracker".to_string()
    }
}
