// Adapted from diffity (MIT License, Copyright (c) 2026 Kamran Ahmed).
// The license text is in core/THIRD_PARTY_NOTICES.

use super::client::Github;
use super::error::GithubError;
use super::graphql::{Actor, Connection, PullRequestField, RepositoryData, TotalCount};
use super::remote::RepoSlug;
use super::types::PullRequest;
use crate::diff::{self, LineKind};
use crate::review::Severity;
use crate::threads::{Side, ThreadAnchor};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeSet, HashMap, HashSet};

const THREAD_FIELDS: &str =
    "id isResolved isOutdated path line startLine originalLine originalStartLine diffSide startDiffSide";
const COMMENT_FIELDS: &str = "databaseId body createdAt author{login __typename} pullRequestReview{id}";

fn threads_query() -> String {
    "query($owner:String!,$name:String!,$number:Int!,$after:String){repository(owner:$owner,name:$name){\
     pullRequest(number:$number){reviewThreads(first:100,after:$after){pageInfo{hasNextPage endCursor} nodes{\
     THREAD_FIELDS comments(first:100){pageInfo{hasNextPage endCursor} nodes{COMMENT_FIELDS}}}}}}}"
        .replace("THREAD_FIELDS", THREAD_FIELDS)
        .replace("COMMENT_FIELDS", COMMENT_FIELDS)
}

fn thread_comments_query() -> String {
    "query($id:ID!,$after:String){node(id:$id){... on PullRequestReviewThread{\
     comments(first:100,after:$after){pageInfo{hasNextPage endCursor} nodes{COMMENT_FIELDS}}}}}"
        .replace("COMMENT_FIELDS", COMMENT_FIELDS)
}

const ADD_REVIEW_MUTATION: &str =
    "mutation($input:AddPullRequestReviewInput!){addPullRequestReview(input:$input){pullRequestReview{id url}}}";
const ADD_THREAD_MUTATION: &str = "mutation($input:AddPullRequestReviewThreadInput!){addPullRequestReviewThread(input:$input){thread{id comments(first:1){nodes{databaseId}}}}}";
const SUBMIT_REVIEW_MUTATION: &str =
    "mutation($input:SubmitPullRequestReviewInput!){submitPullRequestReview(input:$input){pullRequestReview{id url}}}";
const DELETE_REVIEW_MUTATION: &str =
    "mutation($input:DeletePullRequestReviewInput!){deletePullRequestReview(input:$input){pullRequestReview{id}}}";
const REPLY_MUTATION: &str = "mutation($input:AddPullRequestReviewThreadReplyInput!){addPullRequestReviewThreadReply(input:$input){comment{databaseId}}}";

/// Pending reviews are only visible to their author, so any node here is the viewer's.
const PENDING_REVIEWS_QUERY: &str = "query($owner:String!,$name:String!,$number:Int!){repository(owner:$owner,name:$name){\
    pullRequest(number:$number){reviews(first:5,states:PENDING){nodes{id url viewerDidAuthor comments{totalCount}}}}}}";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewEvent {
    Comment,
    Approve,
    RequestChanges,
}

impl ReviewEvent {
    fn name(self) -> &'static str {
        match self {
            Self::Comment => "COMMENT",
            Self::Approve => "APPROVE",
            Self::RequestChanges => "REQUEST_CHANGES",
        }
    }
}

/// What to do with a review you started on github.com and have not submitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingAction {
    AddToExisting,
    Discard,
}

/// A comment of the user's to post. `id` is the caller's own id and comes back in the result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewComment {
    pub id: String,
    pub anchor: ThreadAnchor,
    pub body: String,
    pub severity: Option<Severity>,
}

fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::MustFix => "must-fix",
        Severity::Suggestion => "suggestion",
        Severity::Nit => "nit",
        Severity::Question => "question",
    }
}

impl ReviewComment {
    fn text(&self) -> String {
        match self.severity {
            Some(severity) => format!("**[{}]** {}", severity_label(severity), self.body.trim()),
            None => self.body.trim().to_string(),
        }
    }
}

fn github_side(side: Side) -> &'static str {
    match side {
        Side::Old => "LEFT",
        Side::New => "RIGHT",
    }
}

fn side_from_github(side: Option<&str>) -> Side {
    match side {
        Some("LEFT") => Side::Old,
        _ => Side::New,
    }
}

/// The lines of each file that GitHub's diff of the pull request shows, per side.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrDiff {
    files: HashMap<String, FileLines>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct FileLines {
    old: BTreeSet<u32>,
    new: BTreeSet<u32>,
}

impl PrDiff {
    /// `patches` are `(path, patch)` pairs; a file without a patch (binary, too large) has no commentable line.
    pub fn from_patches<'a>(patches: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut files = HashMap::new();
        for (path, patch) in patches {
            let mut lines = FileLines::default();
            let text = format!("diff --git a/{path} b/{path}\n{patch}\n");
            for file in diff::parse(&text) {
                for hunk in file.hunks {
                    for line in hunk.lines {
                        if line.kind != LineKind::Added {
                            lines.old.extend(line.old);
                        }
                        if line.kind != LineKind::Deleted {
                            lines.new.extend(line.new);
                        }
                    }
                }
            }
            files.insert(path.to_string(), lines);
        }
        Self { files }
    }

    pub fn has_file(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    pub fn shows_line(&self, path: &str, side: Side, line: u32) -> bool {
        self.files.get(path).is_some_and(|lines| match side {
            Side::Old => lines.old.contains(&line),
            Side::New => lines.new.contains(&line),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyReason {
    WholeChange,
    WholeFile,
    FileNotInPullRequest,
    LineNotInDiff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    EmptyComment,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DraftThread {
    pub path: String,
    pub body: String,
    pub line: u32,
    pub side: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewPlan {
    pub drafts: Vec<(String, DraftThread)>,
    pub body_sections: Vec<String>,
    pub in_body: Vec<(String, BodyReason)>,
    pub skipped: Vec<(String, SkipReason)>,
}

fn location(anchor: &ThreadAnchor) -> String {
    match anchor {
        ThreadAnchor::Line { path, line, .. } => format!("`{path}:{line}`"),
        ThreadAnchor::File { path } => format!("`{path}`"),
        ThreadAnchor::Change => String::new(),
    }
}

pub fn build_plan(comments: &[ReviewComment], diff: &PrDiff) -> ReviewPlan {
    let mut plan = ReviewPlan::default();
    for comment in comments {
        let text = comment.text();
        if comment.body.trim().is_empty() {
            plan.skipped.push((comment.id.clone(), SkipReason::EmptyComment));
            continue;
        }
        let in_body = |reason: BodyReason, plan: &mut ReviewPlan| {
            let place = location(&comment.anchor);
            let section = if place.is_empty() {
                text.clone()
            } else {
                format!("{place}: {text}")
            };
            plan.body_sections.push(section);
            plan.in_body.push((comment.id.clone(), reason));
        };
        match &comment.anchor {
            ThreadAnchor::Change => in_body(BodyReason::WholeChange, &mut plan),
            ThreadAnchor::File { path } if diff.has_file(path) => in_body(BodyReason::WholeFile, &mut plan),
            ThreadAnchor::File { .. } => in_body(BodyReason::FileNotInPullRequest, &mut plan),
            ThreadAnchor::Line { path, .. } if !diff.has_file(path) => {
                in_body(BodyReason::FileNotInPullRequest, &mut plan);
            }
            ThreadAnchor::Line { path, side, line } if !diff.shows_line(path, *side, *line) => {
                in_body(BodyReason::LineNotInDiff, &mut plan);
            }
            ThreadAnchor::Line { path, side, line } => plan.drafts.push((
                comment.id.clone(),
                DraftThread {
                    path: path.clone(),
                    body: text,
                    line: *line,
                    side: github_side(*side),
                },
            )),
        }
    }
    plan
}

pub fn compose_review_body(user_body: Option<&str>, sections: &[String]) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(body) = user_body.map(str::trim).filter(|body| !body.is_empty()) {
        parts.push(body.to_string());
    }
    parts.extend(
        sections
            .iter()
            .map(|section| section.trim().to_string())
            .filter(|section| !section.is_empty()),
    );
    parts.join("\n\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostedThread {
    pub local_id: String,
    pub github_thread_id: String,
    pub github_comment_id: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PostResult {
    pub review_url: Option<String>,
    pub posted: Vec<PostedThread>,
    pub in_body: Vec<(String, BodyReason)>,
    pub skipped: Vec<(String, SkipReason)>,
    /// Inline comments GitHub accepted but that could not be matched to a thread, so they cannot be linked.
    pub unlinked: Vec<String>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteComment {
    pub github_id: Option<i64>,
    pub author: String,
    pub is_bot: bool,
    pub body: String,
    pub created_at: String,
    pub review_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteThread {
    pub github_id: String,
    pub anchor: ThreadAnchor,
    pub start_line: Option<u32>,
    pub resolved: bool,
    pub outdated: bool,
    pub comments: Vec<RemoteComment>,
}

impl RemoteThread {
    /// Comments GitHub has that the caller does not know yet.
    pub fn new_comments(&self, known: &HashSet<i64>) -> Vec<&RemoteComment> {
        self.comments
            .iter()
            .filter(|comment| comment.github_id.is_some_and(|id| !known.contains(&id)))
            .collect()
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct RawThread {
    id: String,
    is_resolved: bool,
    is_outdated: bool,
    path: String,
    line: Option<u32>,
    start_line: Option<u32>,
    original_line: Option<u32>,
    original_start_line: Option<u32>,
    diff_side: Option<String>,
    comments: Connection<RawComment>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct RawComment {
    database_id: Option<i64>,
    body: String,
    created_at: String,
    author: Option<Actor>,
    pull_request_review: Option<IdRef>,
}

#[derive(Deserialize, Debug)]
struct IdRef {
    id: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ThreadsField {
    review_threads: Connection<RawThread>,
}

#[derive(Deserialize, Debug)]
struct NodeData {
    node: Option<CommentsField>,
}

#[derive(Deserialize, Debug)]
struct CommentsField {
    comments: Option<Connection<RawComment>>,
}

impl RawComment {
    fn into_comment(self) -> RemoteComment {
        let is_bot = self.author.as_ref().and_then(|actor| actor.typename.as_deref()) == Some("Bot");
        RemoteComment {
            github_id: self.database_id,
            author: self.author.map_or_else(|| "ghost".into(), |actor| actor.login),
            is_bot,
            body: self.body,
            created_at: self.created_at,
            review_id: self.pull_request_review.map(|review| review.id),
        }
    }
}

fn anchor_of(thread: &RawThread) -> (ThreadAnchor, Option<u32>) {
    let Some(end) = thread.line.or(thread.original_line) else {
        return (ThreadAnchor::file(&thread.path), None);
    };
    let start = thread
        .start_line
        .or(thread.original_start_line)
        .filter(|start| *start > 0 && *start < end);
    let side = side_from_github(thread.diff_side.as_deref());
    (ThreadAnchor::line(&thread.path, side, end), start)
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ReviewPayload {
    pull_request_review: Option<ReviewNode>,
}

#[derive(Deserialize, Debug)]
struct ReviewNode {
    id: String,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct AddReviewData {
    add_pull_request_review: ReviewPayload,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct SubmitReviewData {
    submit_pull_request_review: ReviewPayload,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct AddThreadData {
    add_pull_request_review_thread: AddThreadPayload,
}

#[derive(Deserialize, Debug)]
struct AddThreadPayload {
    thread: Option<AddedThread>,
}

#[derive(Deserialize, Debug)]
struct AddedThread {
    id: String,
    comments: Connection<AddedComment>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct AddedComment {
    database_id: Option<i64>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct PendingReviewsField {
    reviews: Connection<PendingReviewNode>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct PendingReviewNode {
    id: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    viewer_did_author: bool,
    comments: TotalCount,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ReplyData {
    add_pull_request_review_thread_reply: ReplyPayload,
}

#[derive(Deserialize, Debug)]
struct ReplyPayload {
    comment: Option<AddedComment>,
}

#[derive(Deserialize)]
struct PrFile {
    filename: String,
    #[serde(default)]
    patch: Option<String>,
}

const MAX_FILE_PAGES: u32 = 30;

impl Github {
    pub fn pr_diff(&self, slug: &RepoSlug, number: u64) -> Result<PrDiff, GithubError> {
        let mut files: Vec<PrFile> = Vec::new();
        for page in 1..=MAX_FILE_PAGES {
            let path = format!(
                "/repos/{}/{}/pulls/{number}/files?per_page=100&page={page}",
                slug.owner, slug.name
            );
            let batch: Vec<PrFile> = serde_json::from_str(&self.rest_get(&path)?)
                .map_err(|error| GithubError::Response(error.to_string()))?;
            let full = batch.len() == 100;
            files.extend(batch);
            if !full {
                break;
            }
        }
        Ok(PrDiff::from_patches(files.iter().map(|file| {
            (file.filename.as_str(), file.patch.as_deref().unwrap_or(""))
        })))
    }

    pub fn review_threads(&self, slug: &RepoSlug, number: u64) -> Result<Vec<RemoteThread>, GithubError> {
        let query = threads_query();
        let mut out = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let data: RepositoryData<PullRequestField<ThreadsField>> = self.graphql(
                &query,
                json!({ "owner": slug.owner, "name": slug.name, "number": number, "after": after }),
            )?;
            let connection = data
                .repository
                .and_then(|repository| repository.pull_request)
                .ok_or_else(|| GithubError::NotFound(format!("PR #{number} in {}", slug.full_name())))?
                .review_threads;
            let next = connection.next_cursor();
            for thread in connection.into_items() {
                out.push(self.finish_thread(thread)?);
            }
            match next {
                Some(cursor) => after = Some(cursor),
                None => return Ok(out),
            }
        }
    }

    fn finish_thread(&self, thread: RawThread) -> Result<RemoteThread, GithubError> {
        let (anchor, start_line) = anchor_of(&thread);
        let mut cursor = thread.comments.next_cursor();
        let mut comments: Vec<RemoteComment> = thread
            .comments
            .into_items()
            .into_iter()
            .map(RawComment::into_comment)
            .collect();
        while let Some(after) = cursor.take() {
            let data: NodeData = self.graphql(&thread_comments_query(), json!({ "id": thread.id, "after": after }))?;
            let more = data
                .node
                .and_then(|node| node.comments)
                .ok_or_else(|| GithubError::NotFound("review thread".into()))?;
            cursor = more.next_cursor();
            comments.extend(more.into_items().into_iter().map(RawComment::into_comment));
        }
        Ok(RemoteThread {
            github_id: thread.id,
            anchor,
            start_line,
            resolved: thread.is_resolved,
            outdated: thread.is_outdated,
            comments,
        })
    }

    /// The id of the reply, or `None` in dry-run mode.
    pub fn reply_to_thread(&self, github_thread_id: &str, body: &str) -> Result<Option<i64>, GithubError> {
        let sent: Option<ReplyData> = self.graphql_mutation(
            "addPullRequestReviewThreadReply",
            REPLY_MUTATION,
            json!({ "input": { "pullRequestReviewThreadId": github_thread_id, "body": body } }),
        )?;
        Ok(sent.and_then(|data| {
            data.add_pull_request_review_thread_reply
                .comment
                .and_then(|comment| comment.database_id)
        }))
    }

    fn pending_review(
        &self,
        slug: &RepoSlug,
        number: u64,
    ) -> Result<Option<(String, Option<String>, u32)>, GithubError> {
        let data: RepositoryData<PullRequestField<PendingReviewsField>> = self.graphql(
            PENDING_REVIEWS_QUERY,
            json!({ "owner": slug.owner, "name": slug.name, "number": number }),
        )?;
        let reviews = data
            .repository
            .and_then(|repository| repository.pull_request)
            .map(|pr| pr.reviews.into_items())
            .unwrap_or_default();
        Ok(reviews
            .into_iter()
            .find(|review| review.viewer_did_author)
            .map(|review| (review.id, review.url, review.comments.total_count)))
    }

    pub fn plan_review(
        &self,
        slug: &RepoSlug,
        number: u64,
        comments: &[ReviewComment],
    ) -> Result<ReviewPlan, GithubError> {
        Ok(build_plan(comments, &self.pr_diff(slug, number)?))
    }

    /// Posts one review. `commit_oid` is the pull request head the comments were written against.
    #[allow(clippy::too_many_arguments)]
    pub fn post_review(
        &self,
        slug: &RepoSlug,
        pr: &PullRequest,
        commit_oid: &str,
        event: ReviewEvent,
        body: Option<&str>,
        comments: &[ReviewComment],
        pending: Option<PendingAction>,
    ) -> Result<PostResult, GithubError> {
        let current = self.pull_request(slug, pr.number)?;
        if !current.head_sha.eq_ignore_ascii_case(commit_oid) {
            return Err(GithubError::HeadMoved {
                expected: commit_oid.to_string(),
                actual: current.head_sha,
            });
        }
        let plan = self.plan_review(slug, pr.number, comments)?;
        let review_body = compose_review_body(body, &plan.body_sections);
        let mut result = PostResult {
            in_body: plan.in_body.clone(),
            skipped: plan.skipped.clone(),
            ..PostResult::default()
        };
        if plan.drafts.is_empty() && review_body.is_empty() && event == ReviewEvent::Comment {
            return Ok(result);
        }

        let existing = self.pending_review(slug, pr.number)?;
        let drafts: Vec<&DraftThread> = plan.drafts.iter().map(|(_, draft)| draft).collect();
        let mut mapped: Vec<(String, String, Option<i64>)> = Vec::new();
        let review_id: Option<String> = match (existing, pending) {
            (Some((_, _, count)), None) => return Err(GithubError::PendingReviewExists(count)),
            (Some((id, url, _)), Some(PendingAction::AddToExisting)) => {
                for (local_id, draft) in &plan.drafts {
                    let mut input = serde_json::to_value(draft).unwrap_or_default();
                    input["pullRequestReviewId"] = json!(id);
                    let added: Option<AddThreadData> = self.graphql_mutation(
                        "addPullRequestReviewThread",
                        ADD_THREAD_MUTATION,
                        json!({ "input": input }),
                    )?;
                    if let Some(thread) = added.and_then(|data| data.add_pull_request_review_thread.thread) {
                        let first = thread
                            .comments
                            .into_items()
                            .into_iter()
                            .next()
                            .and_then(|comment| comment.database_id);
                        mapped.push((local_id.clone(), thread.id, first));
                    }
                }
                let submitted: Option<SubmitReviewData> = self.graphql_mutation(
                    "submitPullRequestReview",
                    SUBMIT_REVIEW_MUTATION,
                    json!({ "input": { "pullRequestReviewId": id, "event": event.name(), "body": review_body } }),
                )?;
                let node = submitted.and_then(|data| data.submit_pull_request_review.pull_request_review);
                result.review_url = node.as_ref().and_then(|node| node.url.clone()).or(url);
                node.map(|node| node.id)
            }
            (existing, _) => {
                if let Some((id, _, _)) = existing {
                    let _: Option<serde_json::Value> = self.graphql_mutation(
                        "deletePullRequestReview",
                        DELETE_REVIEW_MUTATION,
                        json!({ "input": { "pullRequestReviewId": id } }),
                    )?;
                }
                let input = json!({
                    "pullRequestId": pr.node_id,
                    "commitOID": commit_oid,
                    "event": event.name(),
                    "body": review_body,
                    "threads": drafts,
                });
                let created: Option<AddReviewData> =
                    self.graphql_mutation("addPullRequestReview", ADD_REVIEW_MUTATION, json!({ "input": input }))?;
                let node = created.and_then(|data| data.add_pull_request_review.pull_request_review);
                result.review_url = node.as_ref().and_then(|node| node.url.clone());
                node.map(|node| node.id)
            }
        };

        let Some(review_id) = review_id else {
            result.dry_run = true;
            return Ok(result);
        };
        if mapped.is_empty() && !plan.drafts.is_empty() {
            let remote = self.review_threads(slug, pr.number)?;
            mapped = match_created_threads(&plan.drafts, &remote, &review_id);
        }
        for (local_id, github_thread_id, github_comment_id) in &mapped {
            result.posted.push(PostedThread {
                local_id: local_id.clone(),
                github_thread_id: github_thread_id.clone(),
                github_comment_id: *github_comment_id,
            });
        }
        let mapped_ids: HashSet<&str> = mapped.iter().map(|(id, _, _)| id.as_str()).collect();
        result.unlinked = plan
            .drafts
            .iter()
            .map(|(id, _)| id.clone())
            .filter(|id| !mapped_ids.contains(id.as_str()))
            .collect();
        Ok(result)
    }
}

/// `addPullRequestReview` does not return its threads, so they are matched by file and text.
fn match_created_threads(
    drafts: &[(String, DraftThread)],
    remote: &[RemoteThread],
    review_id: &str,
) -> Vec<(String, String, Option<i64>)> {
    let mut candidates: Vec<&RemoteThread> = remote
        .iter()
        .filter(|thread| {
            thread
                .comments
                .first()
                .and_then(|comment| comment.review_id.as_deref())
                .is_some_and(|id| id == review_id)
        })
        .collect();
    let mut mapped = Vec::new();
    for (local_id, draft) in drafts {
        let found = candidates.iter().position(|thread| {
            thread.anchor.path() == draft.path
                && thread
                    .comments
                    .first()
                    .is_some_and(|comment| comment.body.trim() == draft.body.trim())
        });
        if let Some(position) = found {
            let thread = candidates.remove(position);
            mapped.push((
                local_id.clone(),
                thread.github_id.clone(),
                thread.comments.first().and_then(|comment| comment.github_id),
            ));
        }
    }
    mapped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::client::DryRunLog;
    use crate::github::http::fake::FakeHttp;
    use serde_json::Value;
    use std::sync::Arc;

    const PR: &str = include_str!("../../tests/fixtures/github/get_pr.json");
    const FILES: &str = include_str!("../../tests/fixtures/github/pr_files.json");
    const THREADS_1: &str = include_str!("../../tests/fixtures/github/review_threads_page1.json");
    const THREADS_2: &str = include_str!("../../tests/fixtures/github/review_threads_page2.json");
    const MORE_COMMENTS: &str = include_str!("../../tests/fixtures/github/thread_comments.json");
    const CREATED_THREADS: &str = include_str!("../../tests/fixtures/github/review_threads_created.json");
    const SHA: &str = "3f2a91c0d4e5b6a7988776655443322110ffeedd";
    const NO_PENDING: &str = r#"{"data":{"repository":{"pullRequest":{"reviews":{"nodes":[]}}}}}"#;
    const PENDING: &str = r#"{"data":{"repository":{"pullRequest":{"reviews":{"nodes":[
        {"id":"PRR_other","url":null,"viewerDidAuthor":false,"comments":{"totalCount":9}},
        {"id":"PRR_pending","url":"https://github.com/o/r/pull/482#pullrequestreview-7","viewerDidAuthor":true,"comments":{"totalCount":2}}]}}}}}"#;
    const ADDED_REVIEW: &str = r#"{"data":{"addPullRequestReview":{"pullRequestReview":{"id":"PRR_new","url":"https://github.com/o/r/pull/482#pullrequestreview-9"}}}}"#;

    fn slug() -> RepoSlug {
        RepoSlug {
            host: "github.com".into(),
            owner: "o".into(),
            name: "r".into(),
        }
    }

    fn client(http: &Arc<FakeHttp>) -> Github {
        Github::new(http.clone(), "github.com", "tok")
    }

    fn pr() -> PullRequest {
        crate::github::types::sample_pr(482, "feature/undo")
    }

    fn comment(id: &str, anchor: ThreadAnchor, body: &str, severity: Option<Severity>) -> ReviewComment {
        ReviewComment {
            id: id.into(),
            anchor,
            body: body.into(),
            severity,
        }
    }

    fn comments() -> Vec<ReviewComment> {
        vec![
            comment(
                "c1",
                ThreadAnchor::line("src/undo.ts", Side::New, 11),
                "missing await",
                Some(Severity::MustFix),
            ),
            comment(
                "c2",
                ThreadAnchor::line("src/undo.ts", Side::Old, 11),
                "why remove this?",
                None,
            ),
            comment("c3", ThreadAnchor::line("src/undo.ts", Side::New, 99), "far away", None),
            comment("c4", ThreadAnchor::file("src/other.ts"), "unrelated file", None),
            comment("c5", ThreadAnchor::Change, "overall fine", None),
            comment("c6", ThreadAnchor::line("src/undo.ts", Side::New, 12), "   ", None),
            comment("c7", ThreadAnchor::file("src/b.ts"), "new file note", None),
        ]
    }

    fn diff() -> PrDiff {
        client(&Arc::new(FakeHttp::new().reply("/pulls/482/files", 200, FILES)))
            .pr_diff(&slug(), 482)
            .unwrap()
    }

    fn body_of(request: &crate::github::http::Request) -> Value {
        serde_json::from_str(request.body.as_ref().unwrap()).unwrap()
    }

    #[test]
    fn maps_diff_lines_per_side() {
        let diff = diff();
        for line in [10, 11, 12, 13] {
            assert!(diff.shows_line("src/undo.ts", Side::Old, line), "old {line}");
        }
        for line in [10, 11, 12, 13, 14] {
            assert!(diff.shows_line("src/undo.ts", Side::New, line), "new {line}");
        }
        assert!(!diff.shows_line("src/undo.ts", Side::Old, 14));
        assert!(!diff.shows_line("src/undo.ts", Side::New, 9));
        assert!(diff.shows_line("src/b.ts", Side::New, 2));
        assert!(!diff.shows_line("src/b.ts", Side::Old, 1));
        assert!(diff.has_file("logo.png") && !diff.shows_line("logo.png", Side::New, 1));
        assert!(!diff.has_file("src/other.ts"));
    }

    #[test]
    fn plans_inline_threads_and_body_sections() {
        let plan = build_plan(&comments(), &diff());
        let drafts: Vec<(&str, u32, &str, &str)> = plan
            .drafts
            .iter()
            .map(|(id, draft)| (id.as_str(), draft.line, draft.side, draft.body.as_str()))
            .collect();
        assert_eq!(
            drafts,
            [
                ("c1", 11, "RIGHT", "**[must-fix]** missing await"),
                ("c2", 11, "LEFT", "why remove this?")
            ]
        );
        assert_eq!(
            plan.in_body,
            [
                ("c3".to_string(), BodyReason::LineNotInDiff),
                ("c4".to_string(), BodyReason::FileNotInPullRequest),
                ("c5".to_string(), BodyReason::WholeChange),
                ("c7".to_string(), BodyReason::WholeFile),
            ]
        );
        assert_eq!(
            plan.body_sections,
            [
                "`src/undo.ts:99`: far away",
                "`src/other.ts`: unrelated file",
                "overall fine",
                "`src/b.ts`: new file note"
            ]
        );
        assert_eq!(plan.skipped, [("c6".to_string(), SkipReason::EmptyComment)]);
    }

    #[test]
    fn drafts_serialize_the_way_the_api_expects() {
        let plan = build_plan(&comments()[..1], &diff());
        assert_eq!(
            serde_json::to_value(&plan.drafts[0].1).unwrap(),
            json!({"path":"src/undo.ts","body":"**[must-fix]** missing await","line":11,"side":"RIGHT"})
        );
    }

    #[test]
    fn composes_the_review_body() {
        assert_eq!(compose_review_body(None, &[]), "");
        assert_eq!(
            compose_review_body(Some(" hello "), &["a".into(), "".into(), "b".into()]),
            "hello\n\na\n\nb"
        );
    }

    fn fresh_http(extra: &[(&str, &str)]) -> Arc<FakeHttp> {
        let mut http = FakeHttp::new()
            .reply("pullRequest(number:$number){id", 200, PR)
            .reply("/pulls/482/files", 200, FILES)
            .reply("states:PENDING", 200, NO_PENDING);
        for (needle, body) in extra {
            http = http.reply(needle, 200, body);
        }
        Arc::new(http)
    }

    #[test]
    fn posts_a_review_with_inline_threads_and_links_them() {
        let http = fresh_http(&[
            ("addPullRequestReview(input", ADDED_REVIEW),
            ("reviewThreads(first:100", CREATED_THREADS),
        ]);
        let result = client(&http)
            .post_review(
                &slug(),
                &pr(),
                SHA,
                ReviewEvent::RequestChanges,
                Some("Please fix"),
                &comments(),
                None,
            )
            .unwrap();
        http.assert_done();

        let sent = http.requests();
        let input = &body_of(&sent[3])["variables"]["input"];
        assert_eq!(input["pullRequestId"], "PR_482");
        assert_eq!(input["commitOID"], SHA);
        assert_eq!(input["event"], "REQUEST_CHANGES");
        assert_eq!(
            input["body"],
            "Please fix\n\n`src/undo.ts:99`: far away\n\n`src/other.ts`: unrelated file\n\noverall fine\n\n`src/b.ts`: new file note"
        );
        assert_eq!(input["threads"].as_array().unwrap().len(), 2);
        assert_eq!(input["threads"][1]["side"], "LEFT");

        assert_eq!(
            result.review_url.as_deref(),
            Some("https://github.com/o/r/pull/482#pullrequestreview-9")
        );
        assert_eq!(
            result.posted,
            [
                PostedThread {
                    local_id: "c1".into(),
                    github_thread_id: "PRRT_c1".into(),
                    github_comment_id: Some(9001)
                },
                PostedThread {
                    local_id: "c2".into(),
                    github_thread_id: "PRRT_c2".into(),
                    github_comment_id: Some(9002)
                },
            ]
        );
        assert!(result.unlinked.is_empty() && !result.dry_run);
        assert_eq!(result.in_body.len(), 4);
        assert_eq!(result.skipped.len(), 1);
    }

    #[test]
    fn an_approval_without_comments_posts_no_threads() {
        let http = fresh_http(&[("addPullRequestReview(input", ADDED_REVIEW)]);
        let result = client(&http)
            .post_review(&slug(), &pr(), SHA, ReviewEvent::Approve, None, &[], None)
            .unwrap();
        assert!(result.posted.is_empty());
        let input = &body_of(&http.requests()[3])["variables"]["input"];
        assert_eq!(
            (input["event"].as_str(), input["threads"].as_array().map(Vec::len)),
            (Some("APPROVE"), Some(0))
        );
        http.assert_done();
    }

    #[test]
    fn nothing_is_sent_when_there_is_nothing_to_post() {
        let http = Arc::new(FakeHttp::new().reply("pullRequest(number:$number){id", 200, PR).reply(
            "/pulls/482/files",
            200,
            FILES,
        ));
        let empty = [comment("c", ThreadAnchor::Change, "  ", None)];
        let result = client(&http)
            .post_review(&slug(), &pr(), SHA, ReviewEvent::Comment, None, &empty, None)
            .unwrap();
        assert_eq!(result.review_url, None);
        assert_eq!(result.skipped.len(), 1);
        assert_eq!(http.requests().len(), 2);
    }

    #[test]
    fn refuses_to_post_when_the_head_moved() {
        let http = Arc::new(FakeHttp::new().reply("pullRequest(number:$number){id", 200, PR));
        let error = client(&http)
            .post_review(
                &slug(),
                &pr(),
                "deadbeef",
                ReviewEvent::Comment,
                None,
                &comments(),
                None,
            )
            .unwrap_err();
        assert_eq!(
            error,
            GithubError::HeadMoved {
                expected: "deadbeef".into(),
                actual: SHA.into()
            }
        );
        assert_eq!(http.requests().len(), 1);
    }

    #[test]
    fn a_pending_review_needs_a_decision() {
        let http = Arc::new(
            FakeHttp::new()
                .reply("pullRequest(number:$number){id", 200, PR)
                .reply("/pulls/482/files", 200, FILES)
                .reply("states:PENDING", 200, PENDING),
        );
        let error = client(&http)
            .post_review(&slug(), &pr(), SHA, ReviewEvent::Comment, None, &comments(), None)
            .unwrap_err();
        assert_eq!(error, GithubError::PendingReviewExists(2));
    }

    #[test]
    fn adds_to_a_pending_review_and_submits_it() {
        let thread = |id: &str, comment_id: u64| {
            format!(
                r#"{{"data":{{"addPullRequestReviewThread":{{"thread":{{"id":"{id}","comments":{{"nodes":[{{"databaseId":{comment_id}}}]}}}}}}}}}}"#
            )
        };
        let submitted = r#"{"data":{"submitPullRequestReview":{"pullRequestReview":{"id":"PRR_pending","url":"https://github.com/o/r/pull/482#pullrequestreview-7"}}}}"#;
        let http = Arc::new(
            FakeHttp::new()
                .reply("pullRequest(number:$number){id", 200, PR)
                .reply("/pulls/482/files", 200, FILES)
                .reply("states:PENDING", 200, PENDING)
                .reply("addPullRequestReviewThread(input", 200, &thread("PRRT_a", 1))
                .reply("addPullRequestReviewThread(input", 200, &thread("PRRT_b", 2))
                .reply("submitPullRequestReview(input", 200, submitted),
        );
        let result = client(&http)
            .post_review(
                &slug(),
                &pr(),
                SHA,
                ReviewEvent::Comment,
                None,
                &comments(),
                Some(PendingAction::AddToExisting),
            )
            .unwrap();
        http.assert_done();
        let sent = http.requests();
        assert_eq!(
            body_of(&sent[3])["variables"]["input"]["pullRequestReviewId"],
            "PRR_pending"
        );
        assert_eq!(body_of(&sent[3])["variables"]["input"]["line"], 11);
        assert_eq!(body_of(&sent[5])["variables"]["input"]["event"], "COMMENT");
        assert_eq!(result.posted.len(), 2);
        assert_eq!(result.posted[1].github_comment_id, Some(2));
        assert_eq!(
            result.review_url.as_deref(),
            Some("https://github.com/o/r/pull/482#pullrequestreview-7")
        );
    }

    #[test]
    fn discards_a_pending_review_before_posting_a_new_one() {
        let http = Arc::new(
            FakeHttp::new()
                .reply("pullRequest(number:$number){id", 200, PR)
                .reply("/pulls/482/files", 200, FILES)
                .reply("states:PENDING", 200, PENDING)
                .reply(
                    "deletePullRequestReview(input",
                    200,
                    r#"{"data":{"deletePullRequestReview":{"pullRequestReview":{"id":"PRR_pending"}}}}"#,
                )
                .reply("addPullRequestReview(input", 200, ADDED_REVIEW)
                .reply("reviewThreads(first:100", 200, CREATED_THREADS),
        );
        let result = client(&http)
            .post_review(
                &slug(),
                &pr(),
                SHA,
                ReviewEvent::Comment,
                None,
                &comments(),
                Some(PendingAction::Discard),
            )
            .unwrap();
        http.assert_done();
        assert_eq!(
            body_of(&http.requests()[3])["variables"]["input"]["pullRequestReviewId"],
            "PRR_pending"
        );
        assert_eq!(result.posted.len(), 2);
    }

    #[test]
    fn comments_that_cannot_be_matched_to_a_created_thread_are_reported() {
        let none = r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[]}}}}}"#;
        let http = fresh_http(&[
            ("addPullRequestReview(input", ADDED_REVIEW),
            ("reviewThreads(first:100", none),
        ]);
        let result = client(&http)
            .post_review(&slug(), &pr(), SHA, ReviewEvent::Comment, None, &comments(), None)
            .unwrap();
        assert!(result.posted.is_empty());
        assert_eq!(result.unlinked, ["c1", "c2"]);
    }

    #[test]
    fn dry_run_records_the_review_and_sends_no_mutation() {
        let http = fresh_http(&[]);
        let log = Arc::new(DryRunLog::default());
        let result = client(&http)
            .with_dry_run(log.clone())
            .post_review(&slug(), &pr(), SHA, ReviewEvent::Comment, Some("hi"), &comments(), None)
            .unwrap();
        assert!(result.dry_run && result.posted.is_empty() && result.review_url.is_none());
        assert_eq!(http.requests().len(), 3);
        let mutations = log.mutations();
        assert_eq!(mutations.len(), 1);
        assert_eq!(mutations[0].name, "addPullRequestReview");
        assert_eq!(mutations[0].payload["input"]["threads"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn reads_review_threads_with_paged_comments() {
        let http = Arc::new(
            FakeHttp::new()
                .reply("reviewThreads(first:100", 200, THREADS_1)
                .reply("node(id:$id)", 200, MORE_COMMENTS)
                .reply("reviewThreads(first:100", 200, THREADS_2),
        );
        let threads = client(&http).review_threads(&slug(), 482).unwrap();
        http.assert_done();
        assert_eq!(threads.len(), 4);

        let live = &threads[0];
        assert_eq!(live.anchor, ThreadAnchor::line("src/undo.ts", Side::New, 11));
        assert_eq!((live.resolved, live.outdated, live.start_line), (false, false, Some(9)));
        let authors: Vec<&str> = live.comments.iter().map(|comment| comment.author.as_str()).collect();
        assert_eq!(authors, ["mira", "ghost", "alice"]);
        assert!(!live.comments[1].is_bot);
        assert_eq!(live.comments[2].body, "Fixed in the next commit");

        let outdated = &threads[1];
        assert_eq!(outdated.anchor, ThreadAnchor::line("src/undo.ts", Side::Old, 12));
        assert!(outdated.outdated && outdated.resolved);
        assert_eq!(outdated.start_line, Some(10));

        assert_eq!(threads[2].anchor, ThreadAnchor::file("src/b.ts"));
        let bot = &threads[3];
        assert!(bot.comments[0].is_bot);
        assert_eq!(bot.comments[0].review_id.as_deref(), Some("PRR_bot"));
        assert!(http.requests()[1].body.as_ref().unwrap().contains("PRRT_live"));
    }

    #[test]
    fn finds_the_comments_a_local_thread_does_not_have_yet() {
        let http = Arc::new(
            FakeHttp::new()
                .reply("reviewThreads(first:100", 200, THREADS_1)
                .reply("node(id:$id)", 200, MORE_COMMENTS)
                .reply("reviewThreads(first:100", 200, THREADS_2),
        );
        let threads = client(&http).review_threads(&slug(), 482).unwrap();
        let known: HashSet<i64> = [101].into();
        let new: Vec<i64> = threads[0]
            .new_comments(&known)
            .iter()
            .filter_map(|comment| comment.github_id)
            .collect();
        assert_eq!(new, [102, 103]);
        let all: HashSet<i64> = [101, 102, 103].into();
        assert!(threads[0].new_comments(&all).is_empty());
    }

    #[test]
    fn replies_to_a_thread() {
        let http = Arc::new(FakeHttp::new().reply(
            "addPullRequestReviewThreadReply(input",
            200,
            r#"{"data":{"addPullRequestReviewThreadReply":{"comment":{"databaseId":777}}}}"#,
        ));
        assert_eq!(client(&http).reply_to_thread("PRRT_live", "thanks").unwrap(), Some(777));
        let input = &body_of(&http.requests()[0])["variables"]["input"];
        assert_eq!(*input, json!({"pullRequestReviewThreadId":"PRRT_live","body":"thanks"}));

        let quiet = Arc::new(FakeHttp::new());
        let log = Arc::new(DryRunLog::default());
        assert_eq!(
            client(&quiet)
                .with_dry_run(log.clone())
                .reply_to_thread("T", "x")
                .unwrap(),
            None
        );
        assert!(quiet.requests().is_empty() && log.mutations().len() == 1);
    }

    #[test]
    fn queries_have_balanced_braces() {
        for query in [threads_query(), thread_comments_query()] {
            assert_eq!(query.matches('{').count(), query.matches('}').count(), "{query}");
        }
    }
}
