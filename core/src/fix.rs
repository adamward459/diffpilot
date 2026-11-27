//! Builds the runs that act on review threads: fix comments, reply to one thread, make changes.
//!
//! The prompts are adapted from diffity's `resolve.md` and `thread.md` (MIT, see THIRD_PARTY_NOTICES). The run
//! itself, its replies and its "addressed by Claude" marks come from the engine and the MCP tools.

use crate::policy::EditPermission;
use crate::review::ReviewModel;
use crate::run::{RunId, RunKind};
use crate::runner::RunRequest;
use crate::snapshot::ChangedFile;
use crate::store::{SessionKey, Store, StoreError};
use crate::threads::{Addressed, CommentSource, ReviewThread, Side, ThreadAnchor, ThreadId, ThreadStatus};
use std::path::PathBuf;

const FIX_COMMENTS: &str = include_str!("fix_comments.md");
const REPLY_THREAD: &str = include_str!("reply_thread.md");
const MAKE_CHANGES: &str = include_str!("make_changes.md");
const TITLE_LENGTH: usize = 60;
const CAN_EDIT: &str = "Make the smallest change that does what the comment asks, with your file tools.";
const CANNOT_EDIT: &str = "You cannot edit files in this run. When the comment asks for a change, do not try to make it: \
                           `reply` with the change you would make, and do not call `resolve`.";

#[derive(Debug, Clone)]
pub struct Target {
    pub repo: PathBuf,
    pub session: SessionKey,
    pub revisions: Vec<String>,
    pub model: ReviewModel,
    pub edit_permission: EditPermission,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    NotFound,
    Resolved,
    Dismissed,
    Addressed,
    NothingSent,
    AgentSummary,
    WaitingForReviewer,
}

impl Skip {
    pub fn describe(self) -> &'static str {
        match self {
            Self::NotFound => "The thread is gone.",
            Self::Resolved => "The thread is resolved.",
            Self::Dismissed => "The thread is dismissed.",
            Self::Addressed => "Claude already addressed it. Check it, then resolve it or reopen it.",
            Self::NothingSent => "The comment is not sent yet.",
            Self::AgentSummary => "It is a review summary, not a request.",
            Self::WaitingForReviewer => "Claude asked a question and waits for your answer.",
        }
    }
}

pub fn skip_reason(thread: &ReviewThread) -> Option<Skip> {
    match thread.status {
        ThreadStatus::Resolved => return Some(Skip::Resolved),
        ThreadStatus::Dismissed => return Some(Skip::Dismissed),
        ThreadStatus::Open => {}
    }
    if thread.addressed.is_some() {
        return Some(Skip::Addressed);
    }
    let sent: Vec<_> = thread.sent_comments().collect();
    let (Some(first), Some(last)) = (sent.first(), sent.last()) else {
        return Some(Skip::NothingSent);
    };
    if thread.anchor == ThreadAnchor::Change && first.source == CommentSource::Claude {
        return Some(Skip::AgentSummary);
    }
    if sent.len() > 1 && last.source == CommentSource::Claude {
        return Some(Skip::WaitingForReviewer);
    }
    None
}

#[derive(Debug, thiserror::Error)]
#[error("There is no comment to act on.")]
pub struct NothingToFix {
    pub skipped: Vec<(ThreadId, Skip)>,
}

#[derive(Debug)]
pub struct FixPlan {
    pub request: RunRequest,
    pub skipped: Vec<(ThreadId, Skip)>,
}

/// An empty `chosen` means every open comment. `skipped` names what the run leaves out and why.
pub fn fix_comments(target: &Target, threads: &[ReviewThread], chosen: &[ThreadId]) -> Result<FixPlan, NothingToFix> {
    let mut skipped: Vec<(ThreadId, Skip)> = chosen
        .iter()
        .filter(|id| threads.iter().all(|thread| thread.id != **id))
        .map(|id| (*id, Skip::NotFound))
        .collect();
    let mut included = Vec::new();
    for thread in threads {
        if !chosen.is_empty() && !chosen.contains(&thread.id) {
            continue;
        }
        match skip_reason(thread) {
            None => included.push(thread),
            Some(Skip::NothingSent) if chosen.is_empty() => {}
            Some(reason) => skipped.push((thread.id, reason)),
        }
    }
    if included.is_empty() {
        return Err(NothingToFix { skipped });
    }
    let title = match included.len() {
        1 => "Fixing 1 comment".to_string(),
        count => format!("Fixing {count} comments"),
    };
    let mut request = request(
        target,
        RunKind::FixComments,
        &title,
        FIX_COMMENTS,
        &listing(&included),
        "",
    );
    request.threads = included.iter().map(|thread| thread.id).collect();
    Ok(FixPlan { request, skipped })
}

/// A run that answers one thread. With `can_edit` it also changes code when the comment asks for it.
pub fn reply_to(target: &Target, thread: &ReviewThread, can_edit: bool) -> Result<RunRequest, NothingToFix> {
    if thread.sent_comments().next().is_none() {
        return Err(NothingToFix {
            skipped: vec![(thread.id, Skip::NothingSent)],
        });
    }
    let mut request = request(
        target,
        RunKind::Reply,
        "Replying to a comment",
        REPLY_THREAD,
        &listing(&[thread]),
        "",
    );
    request.threads = vec![thread.id];
    request.read_only = !can_edit;
    request.prompt = request
        .prompt
        .replace("{{edits}}", if can_edit { CAN_EDIT } else { CANNOT_EDIT });
    Ok(request)
}

#[derive(Debug, thiserror::Error)]
#[error("Write what Claude should change.")]
pub struct EmptyInstruction;

pub fn make_changes(target: &Target, instruction: &str) -> Result<RunRequest, EmptyInstruction> {
    let instruction = instruction.trim();
    if instruction.is_empty() {
        return Err(EmptyInstruction);
    }
    let quoted: Vec<String> = instruction.lines().map(|line| format!("> {line}")).collect();
    Ok(request(
        target,
        RunKind::MakeChanges,
        &format!("Make changes: {}", short(instruction)),
        MAKE_CHANGES,
        "",
        &quoted.join("\n"),
    ))
}

fn request(
    target: &Target,
    kind: RunKind,
    title: &str,
    template: &str,
    threads: &str,
    instruction: &str,
) -> RunRequest {
    let prompt = template
        .replace("{{revisions}}", &target.revisions.join(" "))
        .replace("{{threads}}", threads)
        .replace("{{instruction}}", instruction);
    let mut request = RunRequest::new(&target.repo, target.session.clone(), kind, title, &prompt, target.model);
    request.revisions = target.revisions.clone();
    request.edit_permission = target.edit_permission;
    request
}

fn short(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default().trim();
    let head: String = line.chars().take(TITLE_LENGTH).collect();
    if line.chars().count() > TITLE_LENGTH {
        format!("{}…", head.trim_end())
    } else {
        head
    }
}

fn listing(threads: &[&ReviewThread]) -> String {
    threads
        .iter()
        .map(|thread| format!("- Thread `{}`: {}", thread.id.0, place(thread)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn place(thread: &ReviewThread) -> String {
    match &thread.anchor {
        ThreadAnchor::Change => "the change as a whole".into(),
        ThreadAnchor::File { path } => format!("`{path}`, the whole file"),
        ThreadAnchor::Line { path, side, line } => {
            let lines = match thread.end_line.filter(|end| end > line) {
                Some(end) => format!("{line}-{end}"),
                None => line.to_string(),
            };
            let old = if *side == Side::Old { " (old side)" } else { "" };
            format!("`{path}:{lines}`{old}")
        }
    }
}

fn anchor_path(thread: &ReviewThread) -> Option<&str> {
    match &thread.anchor {
        ThreadAnchor::File { path } | ThreadAnchor::Line { path, .. } => Some(path),
        ThreadAnchor::Change => None,
    }
}

/// Fills in the files of the threads this run addressed, when Claude did not name them. A thread gets the
/// changed file it is anchored to. When the run addressed one thread only, that thread gets every changed file.
/// Returns the threads that changed.
pub fn record_files(
    store: &Store,
    run: RunId,
    threads: &[ThreadId],
    changed: &[ChangedFile],
) -> Result<Vec<ThreadId>, StoreError> {
    let mut addressed = Vec::new();
    for id in threads {
        if let Some((_, thread)) = store.load_thread(*id)?
            && thread.addressed.as_ref().is_some_and(|mark| mark.run == run)
        {
            addressed.push(thread);
        }
    }
    let sole = addressed.len() == 1;
    let mut updated = Vec::new();
    for thread in addressed {
        let Some(mark) = thread.addressed.as_ref().filter(|mark| mark.files.is_empty()) else {
            continue;
        };
        let files: Vec<String> = match anchor_path(&thread) {
            Some(path) if changed.iter().any(|file| file.path == path) => vec![path.to_string()],
            _ if sole => changed.iter().map(|file| file.path.clone()).collect(),
            _ => Vec::new(),
        };
        if files.is_empty() {
            continue;
        }
        store.mark_addressed(thread.id, &Addressed { files, ..mark.clone() })?;
        updated.push(thread.id);
    }
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::FindingMeta;
    use crate::review::{Effort, ModelName};
    use crate::store::{NewComment, NewThread};

    fn target() -> Target {
        Target {
            repo: PathBuf::from("/r"),
            session: SessionKey::new("/r", "feature", "main"),
            revisions: vec!["main...feature".into()],
            model: ReviewModel::new(ModelName::Sonnet, Effort::Low),
            edit_permission: EditPermission::AskOnce,
        }
    }

    fn comment(source: CommentSource, body: &str, pending: bool) -> crate::threads::ThreadComment {
        crate::threads::ThreadComment {
            id: 0,
            author: if source == CommentSource::Claude {
                "Claude"
            } else {
                "You"
            }
            .into(),
            body: body.into(),
            is_streaming: false,
            source,
            pending,
        }
    }

    fn thread(id: u32, anchor: ThreadAnchor, comments: Vec<crate::threads::ThreadComment>) -> ReviewThread {
        ReviewThread {
            id: ThreadId(id),
            anchor,
            comments,
            finding: None,
            end_line: None,
            anchor_text: None,
            status: ThreadStatus::Open,
            addressed: None,
        }
    }

    fn user(id: u32, line: u32) -> ReviewThread {
        thread(
            id,
            ThreadAnchor::line("a.ts", Side::New, line),
            vec![comment(CommentSource::User, "Rename it.", false)],
        )
    }

    #[test]
    fn a_new_user_comment_is_actionable() {
        assert_eq!(skip_reason(&user(1, 3)), None);
    }

    #[test]
    fn a_finding_from_claude_is_actionable_until_claude_asks_something() {
        let mut found = thread(
            1,
            ThreadAnchor::line("a.ts", Side::New, 3),
            vec![comment(CommentSource::Claude, "Null check missing.", false)],
        );
        found.finding = Some(FindingMeta {
            severity: crate::review::Severity::MustFix,
            title: "Null".into(),
            location: None,
        });
        assert_eq!(skip_reason(&found), None);
        found
            .comments
            .push(comment(CommentSource::Claude, "Which case do you mean?", false));
        assert_eq!(skip_reason(&found), Some(Skip::WaitingForReviewer));
        found
            .comments
            .push(comment(CommentSource::User, "The empty one.", false));
        assert_eq!(skip_reason(&found), None);
    }

    #[test]
    fn closed_addressed_and_unsent_threads_are_skipped() {
        let mut one = user(1, 1);
        one.status = ThreadStatus::Resolved;
        assert_eq!(skip_reason(&one), Some(Skip::Resolved));
        one.status = ThreadStatus::Dismissed;
        assert_eq!(skip_reason(&one), Some(Skip::Dismissed));
        let mut two = user(2, 1);
        two.addressed = Some(Addressed {
            run: RunId(1),
            summary: "Fixed.".into(),
            files: vec![],
        });
        assert_eq!(skip_reason(&two), Some(Skip::Addressed));
        let draft = thread(
            3,
            ThreadAnchor::line("a.ts", Side::New, 1),
            vec![comment(CommentSource::User, "Later.", true)],
        );
        assert_eq!(skip_reason(&draft), Some(Skip::NothingSent));
    }
    #[test]
    fn a_general_comment_from_claude_is_a_summary_not_a_request() {
        let summary = thread(
            1,
            ThreadAnchor::Change,
            vec![comment(CommentSource::Claude, "No issues found.", false)],
        );
        assert_eq!(skip_reason(&summary), Some(Skip::AgentSummary));
        let request = thread(
            2,
            ThreadAnchor::Change,
            vec![comment(CommentSource::User, "Split this change.", false)],
        );
        assert_eq!(skip_reason(&request), None);
    }

    #[test]
    fn fixing_all_comments_takes_the_actionable_ones_in_thread_order() {
        let mut closed = user(2, 5);
        closed.status = ThreadStatus::Resolved;
        let draft = thread(
            4,
            ThreadAnchor::line("b.ts", Side::New, 1),
            vec![comment(CommentSource::User, "Later.", true)],
        );
        let mut ranged = user(3, 7);
        ranged.end_line = Some(9);
        let threads = [user(1, 3), closed, ranged, draft];
        let plan = fix_comments(&target(), &threads, &[]).unwrap();
        assert_eq!(plan.request.threads, [ThreadId(1), ThreadId(3)]);
        assert_eq!(plan.skipped, [(ThreadId(2), Skip::Resolved)]);
        assert_eq!(plan.request.kind, RunKind::FixComments);
        assert_eq!(plan.request.title, "Fixing 2 comments");
        assert_eq!(plan.request.edit_permission, EditPermission::AskOnce);
        assert_eq!(plan.request.revisions, ["main...feature"]);
        let prompt = &plan.request.prompt;
        assert!(prompt.contains("git diff main...feature"));
        assert!(prompt.contains("- Thread `1`: `a.ts:3`\n- Thread `3`: `a.ts:7-9`"));
        assert!(!prompt.contains("{{"), "{prompt}");
    }

    #[test]
    fn fixing_chosen_comments_names_every_one_it_leaves_out() {
        let mut addressed = user(2, 5);
        addressed.addressed = Some(Addressed {
            run: RunId(1),
            summary: String::new(),
            files: vec![],
        });
        let threads = [user(1, 3), addressed, user(3, 9)];
        let plan = fix_comments(&target(), &threads, &[ThreadId(2), ThreadId(3), ThreadId(40)]).unwrap();
        assert_eq!(plan.request.threads, [ThreadId(3)]);
        assert_eq!(plan.request.title, "Fixing 1 comment");
        assert_eq!(
            plan.skipped,
            [(ThreadId(40), Skip::NotFound), (ThreadId(2), Skip::Addressed)]
        );
    }

    #[test]
    fn nothing_to_fix_says_why() {
        let mut closed = user(1, 1);
        closed.status = ThreadStatus::Dismissed;
        let error = fix_comments(&target(), &[closed], &[ThreadId(1)]).unwrap_err();
        assert_eq!(error.skipped, [(ThreadId(1), Skip::Dismissed)]);
        assert!(fix_comments(&target(), &[], &[]).unwrap_err().skipped.is_empty());
    }

    #[test]
    fn a_reply_run_has_one_thread_and_a_prompt_for_it() {
        let one = user(7, 3);
        let request = reply_to(&target(), &one, true).unwrap();
        assert_eq!(
            (request.kind, request.threads.as_slice()),
            (RunKind::Reply, [ThreadId(7)].as_slice())
        );
        assert!(request.prompt.contains("- Thread `7`: `a.ts:3`"));
        assert!(request.prompt.contains(CAN_EDIT));
        assert!(!request.prompt.contains("{{"), "{}", request.prompt);
        assert_eq!(request.edits(), Some(EditPermission::AskOnce));
        let mut resolved = user(8, 1);
        resolved.status = ThreadStatus::Resolved;
        assert!(
            reply_to(&target(), &resolved, true).is_ok(),
            "the reviewer asked for it"
        );
        let draft = thread(
            9,
            ThreadAnchor::Change,
            vec![comment(CommentSource::User, "Later.", true)],
        );
        assert_eq!(
            reply_to(&target(), &draft, true).unwrap_err().skipped,
            [(ThreadId(9), Skip::NothingSent)]
        );
    }

    #[test]
    fn a_reply_that_cannot_edit_is_read_only_and_says_so() {
        let request = reply_to(&target(), &user(7, 3), false).unwrap();
        assert_eq!(request.edits(), None);
        assert!(request.prompt.contains(CANNOT_EDIT));
        assert!(!request.prompt.contains(CAN_EDIT));
    }

    #[test]
    fn a_change_request_quotes_the_instruction_and_names_no_thread() {
        let request = make_changes(&target(), "  Rename x to count.\nKeep the export name.  ").unwrap();
        assert_eq!(request.kind, RunKind::MakeChanges);
        assert!(request.threads.is_empty());
        assert_eq!(request.title, "Make changes: Rename x to count.");
        assert!(request.prompt.contains("> Rename x to count.\n> Keep the export name."));
        assert!(!request.prompt.contains("{{"));
        assert!(make_changes(&target(), " \n ").is_err());
        let long = make_changes(&target(), &"word ".repeat(40)).unwrap();
        assert!(long.title.ends_with('…'));
    }

    #[test]
    fn places_read_well_for_old_lines_files_and_the_whole_change() {
        let old = thread(
            1,
            ThreadAnchor::line("a.ts", Side::Old, 4),
            vec![comment(CommentSource::User, "Why?", false)],
        );
        assert_eq!(place(&old), "`a.ts:4` (old side)");
        assert_eq!(
            place(&thread(2, ThreadAnchor::File { path: "b.ts".into() }, vec![])),
            "`b.ts`, the whole file"
        );
        assert_eq!(place(&thread(3, ThreadAnchor::Change, vec![])), "the change as a whole");
    }

    fn stored(store: &Store, key: &SessionKey, path: &str, line: u32) -> ThreadId {
        let session = store.session(key).unwrap();
        store
            .insert_thread(
                session,
                &NewThread {
                    anchor: ThreadAnchor::line(path, Side::New, line),
                    end_line: None,
                    anchor_text: None,
                    finding: None,
                    first_comment: Some(NewComment {
                        author: "You".into(),
                        source: CommentSource::User,
                        body: "Fix.".into(),
                        streaming: false,
                        pending: false,
                    }),
                },
            )
            .unwrap()
            .value
            .id
    }

    fn address(store: &Store, id: ThreadId, run: i64, files: &[&str]) {
        store
            .mark_addressed(
                id,
                &Addressed {
                    run: RunId(run),
                    summary: "Fixed.".into(),
                    files: files.iter().map(|file| file.to_string()).collect(),
                },
            )
            .unwrap();
    }

    fn files_of(store: &Store, id: ThreadId) -> Vec<String> {
        store.load_thread(id).unwrap().unwrap().1.addressed.unwrap().files
    }

    fn changed(paths: &[&str]) -> Vec<ChangedFile> {
        paths
            .iter()
            .map(|path| ChangedFile {
                path: path.to_string(),
                status: crate::changes::FileStatus::Modified,
                additions: 1,
                deletions: 0,
            })
            .collect()
    }

    #[test]
    fn a_thread_gets_the_changed_file_it_is_anchored_to() {
        let store = Store::open_in_memory().unwrap();
        let key = SessionKey::new("/r", "feature", "main");
        let (a, b, c) = (
            stored(&store, &key, "a.ts", 1),
            stored(&store, &key, "b.ts", 1),
            stored(&store, &key, "c.ts", 1),
        );
        for id in [a, b, c] {
            address(&store, id, 4, &[]);
        }
        address(&store, c, 4, &["named.ts"]);
        let updated = record_files(&store, RunId(4), &[a, b, c], &changed(&["a.ts", "z.ts"])).unwrap();
        assert_eq!(updated, [a]);
        assert_eq!(files_of(&store, a), ["a.ts"]);
        assert!(
            files_of(&store, b).is_empty(),
            "no file matches and more than one thread was addressed"
        );
        assert_eq!(files_of(&store, c), ["named.ts"], "Claude's own list stays");
    }

    #[test]
    fn a_lone_thread_gets_every_changed_file() {
        let store = Store::open_in_memory().unwrap();
        let key = SessionKey::new("/r", "feature", "main");
        let (a, b) = (stored(&store, &key, "a.ts", 1), stored(&store, &key, "b.ts", 1));
        address(&store, a, 4, &[]);
        address(&store, b, 3, &[]);
        let updated = record_files(&store, RunId(4), &[a, b], &changed(&["x.ts", "y.ts"])).unwrap();
        assert_eq!(updated, [a], "another run's mark is not this run's");
        assert_eq!(files_of(&store, a), ["x.ts", "y.ts"]);
        assert!(files_of(&store, b).is_empty());
    }

    #[test]
    fn nothing_changes_when_the_run_changed_no_file() {
        let store = Store::open_in_memory().unwrap();
        let key = SessionKey::new("/r", "feature", "main");
        let a = stored(&store, &key, "a.ts", 1);
        address(&store, a, 4, &[]);
        assert!(record_files(&store, RunId(4), &[a], &[]).unwrap().is_empty());
    }
}
