//! The tools a guide run uses to write the guide: overview, chapters, notes, diagrams, and a last call that saves it.
//!
//! Claude calls them one at a time, so a mistake comes back as an error on that call and the rest of the guide
//! stays. The guide is a draft in the store until `finish_guide` saves it. diffity's `set_guide` takes the whole
//! guide in one call and has the agent send it again after any error. That wastes a long output on a typo.

use super::bridge::{Binding, Notice};
use super::tools::{ToolDef, ToolError, parse};
use crate::diff::{self, FileDiff};
use crate::guide::{Guide, build, stale};
use crate::review::ReviewModel;
use crate::store::Store;
use serde_json::{Value, json};

pub const TOOLS: [&str; 5] = ["set_overview", "add_chapter", "add_note", "add_diagram", "finish_guide"];

pub fn definitions() -> Vec<ToolDef> {
    let range = json!({
        "type": "object",
        "properties": {
            "startLine": { "type": "integer", "minimum": 1 },
            "endLine": { "type": "integer", "minimum": 1, "description": "Defaults to startLine." },
            "side": { "type": "string", "enum": ["new", "old"], "description": "\"new\" (default) for added or unchanged code, \"old\" for removed code." }
        },
        "required": ["startLine"]
    });
    let tool = |name, description, input_schema, read_only| ToolDef {
        name,
        description,
        input_schema,
        read_only,
    };
    vec![
        tool(
            "set_overview",
            "Write the overview of the guide: what this change does and why. Call it once, before the chapters. Call it again to replace it.",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "What the change does, in a short line." },
                    "summary": { "type": "string", "description": "Markdown: what the change does and why, in a sentence or two." },
                    "before": { "type": "string", "description": "How things worked before, in a sentence." },
                    "after": { "type": "string", "description": "How they work after, in a sentence." }
                },
                "required": ["title", "summary"]
            }),
            false,
        ),
        tool(
            "add_chapter",
            "Add the next chapter of the guide. Chapters are read in the order you add them. Returns the chapter number, which add_note and add_diagram take.",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "summary": { "type": "string", "description": "Markdown: why this part exists, in a sentence or two." },
                    "phase": { "type": "string", "description": "The stage of the change, such as \"Core\" or \"UI\". Chapters of one phase sit together." },
                    "attention": { "type": "string", "enum": ["high", "normal", "low"], "description": "high: read every line. low: skim (tests, config, generated files)." },
                    "points": { "type": "array", "items": { "type": "string" }, "description": "Short facts that make the files easier to read." },
                    "focus": { "type": "array", "items": { "type": "string" }, "description": "What to check while reading these files." },
                    "files": {
                        "type": "array",
                        "description": "Files of the diff in reading order. Each is a path, or an object with the line `ranges` the chapter covers when the file is split between chapters.",
                        "items": {
                            "oneOf": [
                                { "type": "string" },
                                {
                                    "type": "object",
                                    "properties": { "path": { "type": "string" }, "ranges": { "type": "array", "items": range } },
                                    "required": ["path"]
                                }
                            ]
                        }
                    },
                    "impact": {
                        "type": "array",
                        "description": "Code outside the diff that depends on this change and may now behave differently.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "folder": { "type": "string", "description": "The linked folder the file is in, exactly as the prompt lists it. Leave out for the repo." },
                                "site": { "type": "string", "description": "For example `loadSettings()` or `GET /api/session`." },
                                "path": { "type": "string", "description": "Relative to the repo, or to `folder` when it is set." },
                                "line": { "type": "integer", "minimum": 1 },
                                "text": { "type": "string", "description": "One sentence: what changes for it." }
                            },
                            "required": ["site", "path", "text"]
                        }
                    }
                },
                "required": ["title", "summary", "files"]
            }),
            false,
        ),
        tool(
            "add_note",
            "Add the next stop of the chapter's review path. The notes of a chapter are its review path, so add them in the order the reviewer should read them. Add one only where it saves the reviewer time. A diff stop's file must be one of the chapter's files and its lines must be inside a hunk.",
            json!({
                "type": "object",
                "properties": {
                    "chapter": { "type": "integer", "minimum": 1, "description": "Chapter number from add_chapter." },
                    "file": { "type": "string", "description": "A file of the chapter, or a path inside `folder`." },
                    "line": { "type": "integer", "minimum": 1, "description": "Leave out for a stop about the whole file." },
                    "endLine": { "type": "integer", "minimum": 1 },
                    "side": { "type": "string", "enum": ["new", "old"] },
                    "text": { "type": "string", "description": "Markdown, one or two sentences: why this change matters." },
                    "critical": { "type": "boolean", "description": "The reviewer should take extra care here: security, data loss, hard to undo, easy to get wrong." },
                    "kind": { "type": "string", "enum": ["read", "check", "skim"], "description": "read: understand it. check (default): verify one thing. skim: a quick look." },
                    "check": { "type": "string", "description": "The one thing to verify, as an action." },
                    "node": { "type": "string", "description": "The id of the node in the chapter's diagram this stop belongs to." },
                    "folder": { "type": "string", "description": "A linked folder, exactly as the prompt lists it, when the stop is in code outside the diff. `file` is then relative to it and `line` is checked against the file." }
                },
                "required": ["chapter", "file", "text"]
            }),
            false,
        ),
        tool(
            "add_diagram",
            "Add a Mermaid diagram to a chapter, or to the overview when `chapter` is left out. Use `flowchart LR` / `flowchart TD` (mark nodes :::added, :::changed or :::removed) or `sequenceDiagram`. A diagram that does not parse is refused with its line number. Stops of the chapter can name its node ids.",
            json!({
                "type": "object",
                "properties": {
                    "chapter": { "type": "integer", "minimum": 1 },
                    "title": { "type": "string" },
                    "source": { "type": "string", "description": "The Mermaid text." }
                },
                "required": ["source"]
            }),
            false,
        ),
        tool(
            "finish_guide",
            "Save the guide. Call it last. Files that no chapter names go to a closing chapter. A stop whose `node` is not in its chapter's diagrams is refused.",
            json!({ "type": "object", "properties": {} }),
            false,
        ),
    ]
}

fn problem(message: String) -> ToolError {
    ToolError(message)
}

fn run_model(store: &Store, binding: &Binding) -> ReviewModel {
    store
        .load_run(binding.run)
        .ok()
        .flatten()
        .map_or(ReviewModel::STANDARD, |run| run.model)
}

fn files_of(binding: &Binding) -> Result<Vec<FileDiff>, ToolError> {
    Ok(diff::load_revisions(&binding.repo, &binding.revisions)?)
}

fn not_open(store: &Store, binding: &Binding) -> Result<(), ToolError> {
    if store.guide_of_run(binding.run)?.is_some() {
        return Err(problem("The guide is saved. Start a new run to write another.".into()));
    }
    Ok(())
}

/// Reads the run's draft (or starts one on the diff as it is now), changes it, and saves it.
fn edit<T>(
    store: &Store,
    binding: &Binding,
    change: impl FnOnce(&mut Guide, &[FileDiff]) -> Result<T, String>,
) -> Result<T, ToolError> {
    not_open(store, binding)?;
    let files = files_of(binding)?;
    let (mut guide, model) = match store.load_draft(binding.run)? {
        Some(draft) => (draft.guide, draft.model.unwrap_or(ReviewModel::STANDARD)),
        None => {
            let mut guide = build::begin(&files, stale::head(&binding.repo));
            guide.context = binding.linked.clone();
            (guide, run_model(store, binding))
        }
    };
    let out = change(&mut guide, &files).map_err(problem)?;
    store.save_draft(binding.session, binding.run, model, &guide)?;
    (binding.notify)(Notice::GuideChanged);
    Ok(out)
}

pub fn call(store: &Store, binding: &Binding, tool: &str, args: Value) -> Result<Value, ToolError> {
    match tool {
        "set_overview" => {
            let args: build::OverviewArgs = parse(args)?;
            edit(store, binding, |guide, _| build::set_overview(guide, &args))?;
            Ok(json!({ "ok": true }))
        }
        "add_chapter" => {
            let args: build::ChapterArgs = parse(args)?;
            let number = edit(store, binding, |guide, files| build::add_chapter(guide, files, &args))?;
            Ok(json!({ "chapter": number }))
        }
        "add_note" => {
            let args: build::NoteArgs = parse(args)?;
            edit(store, binding, |guide, files| build::add_note(guide, files, &args))?;
            Ok(json!({ "ok": true }))
        }
        "add_diagram" => {
            let args: build::DiagramArgs = parse(args)?;
            let kind = edit(store, binding, |guide, _| build::add_diagram(guide, &args))?;
            Ok(json!({ "ok": true, "kind": kind }))
        }
        "finish_guide" => finish(store, binding),
        other => Err(ToolError(format!("Unknown tool `{other}`."))),
    }
}

fn finish(store: &Store, binding: &Binding) -> Result<Value, ToolError> {
    not_open(store, binding)?;
    let Some(draft) = store.load_draft(binding.run)? else {
        return Err(problem(
            "There is nothing to save. Call set_overview and add_chapter first.".into(),
        ));
    };
    let mut guide = draft.guide;
    let done = build::finish(&mut guide, &files_of(binding)?).map_err(problem)?;
    store.complete_draft(binding.run, &guide)?;
    (binding.notify)(Notice::GuideChanged);
    Ok(json!({ "saved": true, "chapters": done.chapters, "filesAddedToEverythingElse": done.leftover }))
}
