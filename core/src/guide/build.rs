//! Builds a guide one call at a time, the way the MCP tools receive it, and checks each call against the diff.
//!
//! Every function returns an error text written for Claude, which fixes the call and sends it again. The word
//! limits and the rules for phases, files and flow steps follow diffity's `set_guide` checks (MIT, see
//! THIRD_PARTY_NOTICES), spread over several calls instead of one.

use super::mermaid::{self, DiagramKind, Graph};
use super::stale;
use super::{Attention, Chapter, ChapterFile, Diagram, FlowStep, Guide, Impact, Note, Overview, Span};
use crate::anchor;
use crate::diff::FileDiff;
use crate::threads::Side;
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const OVERVIEW_TITLE_WORDS: usize = 10;
pub const SUMMARY_WORDS: usize = 50;
pub const BEFORE_AFTER_WORDS: usize = 30;
pub const TITLE_WORDS: usize = 6;
pub const CHAPTER_SUMMARY_WORDS: usize = 40;
pub const BULLET_WORDS: usize = 20;
pub const MAX_BULLETS: usize = 3;
pub const NOTE_WORDS: usize = 40;
pub const PHASE_WORDS: usize = 4;
pub const MAX_FLOW_STEPS: usize = 10;
pub const FLOW_WORDS: usize = 25;
pub const MAX_IMPACT: usize = 8;
pub const IMPACT_WORDS: usize = 25;
pub const MAX_DIAGRAM_BYTES: usize = 8 * 1024;
pub const MAX_DIAGRAMS: usize = 3;
pub const LEFTOVER_CHAPTER: &str = "Everything else";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewArgs {
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub before: Option<String>,
    #[serde(default)]
    pub after: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeArg {
    pub start_line: u32,
    #[serde(default)]
    pub end_line: Option<u32>,
    #[serde(default)]
    pub side: Option<Side>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum FileArg {
    Path(String),
    Ranges {
        path: String,
        #[serde(default)]
        ranges: Vec<RangeArg>,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImpactArg {
    #[serde(default)]
    pub folder: Option<String>,
    pub site: String,
    pub path: String,
    #[serde(default)]
    pub line: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterArgs {
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub attention: Option<Attention>,
    #[serde(default)]
    pub points: Vec<String>,
    #[serde(default)]
    pub focus: Vec<String>,
    pub files: Vec<FileArg>,
    #[serde(default)]
    pub impact: Vec<ImpactArg>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteArgs {
    pub chapter: usize,
    pub file: String,
    #[serde(default)]
    pub line: Option<u32>,
    #[serde(default)]
    pub end_line: Option<u32>,
    #[serde(default)]
    pub side: Option<Side>,
    pub text: String,
    #[serde(default)]
    pub critical: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowArg {
    pub node: String,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagramArgs {
    #[serde(default)]
    pub chapter: Option<usize>,
    #[serde(default)]
    pub title: Option<String>,
    pub source: String,
    #[serde(default)]
    pub flow: Vec<FlowArg>,
}

#[derive(Default)]
struct Problems(Vec<String>);

impl Problems {
    fn words(&mut self, what: &str, text: &str, max: usize) {
        let words = text.split_whitespace().count();
        if words > max {
            self.0.push(format!("{what} is {words} words (max {max})"));
        }
    }

    fn bullets(&mut self, what: &str, items: &[String]) {
        if items.len() > MAX_BULLETS {
            self.0
                .push(format!("{what} has {} items (max {MAX_BULLETS})", items.len()));
        }
        for (at, item) in items.iter().enumerate() {
            self.words(&format!("{what} item {}", at + 1), item, BULLET_WORDS);
        }
    }

    fn done(self) -> Result<(), String> {
        if self.0.is_empty() {
            return Ok(());
        }
        Err(format!(
            "Too long for a reviewer to skim. Cut these, keeping the why, and call the tool again: {}.",
            self.0.join("; ")
        ))
    }
}

fn clean(text: &str) -> String {
    text.trim().to_string()
}

fn non_empty(text: Option<&str>) -> Option<String> {
    text.map(clean).filter(|text| !text.is_empty())
}

fn items(list: &[String]) -> Vec<String> {
    list.iter()
        .map(|item| clean(item))
        .filter(|item| !item.is_empty())
        .collect()
}

pub fn resolve<'a>(files: &'a [FileDiff], raw: &str) -> Option<&'a FileDiff> {
    let path = raw.trim().trim_start_matches("./");
    files
        .iter()
        .find(|file| file.path == path)
        .or_else(|| files.iter().find(|file| file.old_path.as_deref() == Some(path)))
}

fn not_in_diff(files: &[FileDiff], paths: &[String]) -> String {
    let listed: Vec<&str> = files.iter().map(|file| file.path.as_str()).take(80).collect();
    format!(
        "These files are not in the diff: {}. Use paths exactly as get_diff lists them: {}.",
        paths.join(", "),
        listed.join(", ")
    )
}

fn span(file: &FileDiff, side: Side, line: u32, end_line: Option<u32>) -> Result<Span, String> {
    let end = end_line.filter(|end| *end >= line).unwrap_or(line);
    let text = anchor::lines_text(file, side, line, end).ok_or_else(|| {
        format!(
            "Lines {line}-{end} of `{}` are not in the diff on the {} side. Pick lines inside a hunk, as the @@ headers show.",
            file.path,
            if side == Side::New { "new" } else { "old" }
        )
    })?;
    Ok(Span {
        side,
        line,
        end_line: (end > line).then_some(end),
        anchor_text: Some(text),
    })
}

pub fn begin(files: &[FileDiff], head: Option<String>) -> Guide {
    Guide {
        fingerprint: stale::fingerprint(files),
        file_fingerprints: stale::file_fingerprints(files),
        head,
        ..Guide::default()
    }
}

pub fn set_overview(guide: &mut Guide, args: &OverviewArgs) -> Result<(), String> {
    let (title, summary) = (clean(&args.title), clean(&args.summary));
    if title.is_empty() || summary.is_empty() {
        return Err("The overview needs a title and a summary.".into());
    }
    let (before, after) = (non_empty(args.before.as_deref()), non_empty(args.after.as_deref()));
    let mut problems = Problems::default();
    problems.words("title", &title, OVERVIEW_TITLE_WORDS);
    problems.words("summary", &summary, SUMMARY_WORDS);
    if let Some(text) = &before {
        problems.words("before", text, BEFORE_AFTER_WORDS);
    }
    if let Some(text) = &after {
        problems.words("after", text, BEFORE_AFTER_WORDS);
    }
    problems.done()?;
    guide.overview = Overview {
        title,
        summary,
        before,
        after,
    };
    Ok(())
}

fn chapter_files(guide: &Guide, files: &[FileDiff], args: &[FileArg]) -> Result<Vec<ChapterFile>, String> {
    let mut found: Vec<ChapterFile> = Vec::new();
    let mut unknown = Vec::new();
    for arg in args {
        let (raw, ranges) = match arg {
            FileArg::Path(path) => (path, &[][..]),
            FileArg::Ranges { path, ranges } => (path, ranges.as_slice()),
        };
        let Some(file) = resolve(files, raw) else {
            unknown.push(raw.trim().to_string());
            continue;
        };
        let mut spans = Vec::new();
        for range in ranges {
            spans.push(span(
                file,
                range.side.unwrap_or(Side::New),
                range.start_line,
                range.end_line,
            )?);
        }
        let whole = spans.is_empty();
        let taken = guide.chapters.iter().enumerate().find(|(_, chapter)| {
            chapter
                .files
                .iter()
                .any(|known| known.path == file.path && (whole || known.spans.is_empty()))
        });
        if let Some((at, _)) = taken {
            return Err(format!(
                "`{}` is already in chapter {}. A file belongs to one chapter, unless each chapter names the line ranges it covers with `ranges`.",
                file.path,
                at + 1
            ));
        }
        match found.iter_mut().find(|known| known.path == file.path) {
            Some(known) if !known.spans.is_empty() && !whole => known.spans.extend(spans),
            Some(_) => {}
            None => found.push(ChapterFile {
                path: file.path.clone(),
                spans,
            }),
        }
    }
    if !unknown.is_empty() {
        return Err(not_in_diff(files, &unknown));
    }
    if found.is_empty() {
        return Err("A chapter must list at least one file of the diff.".into());
    }
    Ok(found)
}

/// Adds a chapter after the ones before it. Returns its number, which starts at 1.
pub fn add_chapter(guide: &mut Guide, files: &[FileDiff], args: &ChapterArgs) -> Result<usize, String> {
    let (title, summary) = (clean(&args.title), clean(&args.summary));
    if title.is_empty() {
        return Err("Every chapter needs a title.".into());
    }
    let (points, focus) = (items(&args.points), items(&args.focus));
    let phase = non_empty(args.phase.as_deref());
    let mut problems = Problems::default();
    problems.words(&format!("chapter \"{title}\" title"), &title, TITLE_WORDS);
    problems.words(&format!("chapter \"{title}\" summary"), &summary, CHAPTER_SUMMARY_WORDS);
    problems.bullets(&format!("chapter \"{title}\" points"), &points);
    problems.bullets(&format!("chapter \"{title}\" focus"), &focus);
    if let Some(name) = &phase {
        problems.words(&format!("chapter \"{title}\" phase"), name, PHASE_WORDS);
    }
    let impact = impact_of(&args.impact, &title, files, &guide.context, &mut problems)?;
    problems.done()?;
    if let Some(name) = &phase {
        let last = guide.chapters.last().and_then(|chapter| chapter.phase.as_ref());
        if last != Some(name)
            && guide
                .chapters
                .iter()
                .any(|chapter| chapter.phase.as_ref() == Some(name))
        {
            return Err(format!(
                "The chapters of phase \"{name}\" must sit next to each other. Add chapter \"{title}\" right after them, or give it another phase."
            ));
        }
    }
    let chapter_files = chapter_files(guide, files, &args.files)?;
    guide.chapters.push(Chapter {
        title,
        summary,
        phase,
        attention: args.attention.unwrap_or_default(),
        points,
        focus,
        files: chapter_files,
        notes: Vec::new(),
        diagrams: Vec::new(),
        impact,
    });
    Ok(guide.chapters.len())
}

/// Code outside the diff. An item inside the diff is dropped, because the reviewer reads those files anyway.
fn impact_of(
    args: &[ImpactArg],
    title: &str,
    files: &[FileDiff],
    context: &[PathBuf],
    problems: &mut Problems,
) -> Result<Vec<Impact>, String> {
    let mut kept = Vec::new();
    for item in args {
        if item.site.trim().is_empty() || item.path.trim().is_empty() {
            continue;
        }
        let (folder, path) = place(item, context)?;
        if folder.is_none() && resolve(files, &path).is_some() {
            continue;
        }
        kept.push((item, folder, path));
    }
    if kept.len() > MAX_IMPACT {
        problems.0.push(format!(
            "chapter \"{title}\" impact has {} items (max {MAX_IMPACT}); keep the ones most likely to break",
            kept.len()
        ));
    }
    Ok(kept
        .into_iter()
        .map(|(item, folder, path)| {
            let (site, text) = (clean(&item.site), clean(&item.text));
            problems.words(&format!("chapter \"{title}\" impact `{site}`"), &text, IMPACT_WORDS);
            Impact {
                folder,
                site,
                path,
                line: item.line.filter(|line| *line > 0),
                text,
            }
        })
        .collect())
}

/// The linked folder an impact item is in, and its path inside that folder or the repo.
fn place(item: &ImpactArg, context: &[PathBuf]) -> Result<(Option<PathBuf>, String), String> {
    let path = Path::new(item.path.trim().trim_start_matches("./"));
    let named = item.folder.as_deref().map(str::trim).filter(|name| !name.is_empty());
    let folder = match named {
        Some(name) => Some(
            context
                .iter()
                .find(|folder| folder.as_path() == Path::new(name))
                .ok_or_else(|| unknown_folder(name, context))?,
        ),
        None => context.iter().find(|folder| path.starts_with(folder)),
    };
    let Some(folder) = folder else {
        return Ok((None, path.to_string_lossy().into_owned()));
    };
    let inside = path.strip_prefix(folder).unwrap_or(path);
    if inside.as_os_str().is_empty() || !folder.join(inside).exists() {
        return Err(format!(
            "`{}` is not a file in the linked folder `{}`. Give its path inside that folder.",
            inside.display(),
            folder.display()
        ));
    }
    Ok((Some(folder.clone()), inside.to_string_lossy().into_owned()))
}

fn unknown_folder(name: &str, context: &[PathBuf]) -> String {
    if context.is_empty() {
        return format!("`{name}` is not a linked folder, and none are linked. Leave out `folder`.");
    }
    let listed: Vec<String> = context.iter().map(|folder| folder.display().to_string()).collect();
    format!("`{name}` is not a linked folder. Use one of: {}.", listed.join(", "))
}

pub fn add_note(guide: &mut Guide, files: &[FileDiff], args: &NoteArgs) -> Result<(), String> {
    let count = guide.chapters.len();
    let chapter = args
        .chapter
        .checked_sub(1)
        .and_then(|at| guide.chapters.get_mut(at))
        .ok_or_else(|| {
            format!(
                "There is no chapter {}. Add the chapter first. There are {count} so far.",
                args.chapter
            )
        })?;
    let text = clean(&args.text);
    if text.is_empty() {
        return Err("The note is empty.".into());
    }
    let mut problems = Problems::default();
    problems.words("the note", &text, NOTE_WORDS);
    problems.done()?;
    let file = resolve(files, &args.file).ok_or_else(|| not_in_diff(files, &[args.file.trim().to_string()]))?;
    if !chapter.covers(&file.path) {
        let listed: Vec<&str> = chapter.files.iter().map(|file| file.path.as_str()).collect();
        return Err(format!(
            "`{}` is not one of chapter {}'s files: {}. Put the note on one of them.",
            file.path,
            args.chapter,
            listed.join(", ")
        ));
    }
    let span = match args.line.filter(|line| *line > 0) {
        Some(line) => Some(span(file, args.side.unwrap_or(Side::New), line, args.end_line)?),
        None => None,
    };
    chapter.notes.push(Note {
        path: file.path.clone(),
        span,
        text,
        critical: args.critical,
    });
    Ok(())
}

fn flow_steps(graph: &Graph, args: &[FlowArg]) -> Result<Vec<FlowStep>, String> {
    let steps: Vec<&FlowArg> = args.iter().filter(|step| !step.node.trim().is_empty()).collect();
    let mut problems = Problems::default();
    if steps.len() > MAX_FLOW_STEPS {
        problems.0.push(format!(
            "flow has {} steps (max {MAX_FLOW_STEPS}); keep the main path",
            steps.len()
        ));
    }
    let known: Option<Vec<&str>> = match graph {
        Graph::Flowchart(chart) => Some(chart.nodes.iter().map(|node| node.id.as_str()).collect()),
        Graph::Sequence(sequence) => Some(sequence.participants.iter().map(|part| part.id.as_str()).collect()),
        Graph::Source => None,
    };
    let mut flow = Vec::new();
    for step in steps {
        let node = clean(&step.node);
        if known.as_ref().is_some_and(|known| !known.contains(&node.as_str())) {
            return Err(format!(
                "Flow step `{node}` is not in the diagram. Use the ids as written in it, such as `A` for `A[Load]`."
            ));
        }
        let text = clean(&step.text);
        problems.words(&format!("flow step `{node}` text"), &text, FLOW_WORDS);
        flow.push(FlowStep { node, text });
    }
    problems.done()?;
    Ok(flow)
}

/// Adds a diagram to a chapter, or to the overview when `args.chapter` is unset.
pub fn add_diagram(guide: &mut Guide, args: &DiagramArgs) -> Result<DiagramKind, String> {
    let source = args.source.trim();
    if source.is_empty() {
        return Err("The diagram is empty.".into());
    }
    if source.len() > MAX_DIAGRAM_BYTES {
        return Err(format!(
            "The diagram is {} bytes (max {MAX_DIAGRAM_BYTES}). Show the main path only.",
            source.len()
        ));
    }
    let graph =
        mermaid::parse(source).map_err(|error| format!("{error} Fix the Mermaid text and call the tool again."))?;
    let kind = mermaid::kind_of(source).unwrap_or(DiagramKind::Other(String::new()));
    let flow = flow_steps(&graph, &args.flow)?;
    let diagram = Diagram {
        title: non_empty(args.title.as_deref()),
        kind: kind.clone(),
        source: source.to_string(),
        flow,
    };
    let count = guide.chapters.len();
    let target = match args.chapter {
        None => &mut guide.diagrams,
        Some(number) => {
            &mut number
                .checked_sub(1)
                .and_then(|at| guide.chapters.get_mut(at))
                .ok_or_else(|| format!("There is no chapter {number}. There are {count} so far."))?
                .diagrams
        }
    };
    if target.len() >= MAX_DIAGRAMS {
        return Err(format!(
            "There are already {MAX_DIAGRAMS} diagrams here. Show what the reviewer needs most."
        ));
    }
    target.push(diagram);
    Ok(kind)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    pub chapters: usize,
    /// Files no chapter named. They went to a last, low-attention chapter.
    pub leftover: Vec<String>,
}

/// Checks the guide is whole. Files no chapter covers go to a closing chapter, so the reviewer still sees them.
pub fn finish(guide: &mut Guide, files: &[FileDiff]) -> Result<Finished, String> {
    if guide.overview.summary.trim().is_empty() {
        return Err("Call set_overview first: the guide needs a title and a summary.".into());
    }
    if guide.chapters.is_empty() {
        return Err("Add at least one chapter with add_chapter.".into());
    }
    let leftover: Vec<String> = files
        .iter()
        .filter(|file| !guide.covers(&file.path))
        .map(|file| file.path.clone())
        .collect();
    if !leftover.is_empty() {
        guide.chapters.push(Chapter {
            title: LEFTOVER_CHAPTER.into(),
            summary: "Files the guide does not walk through.".into(),
            phase: None,
            attention: Attention::Low,
            points: Vec::new(),
            focus: Vec::new(),
            files: leftover
                .iter()
                .map(|path| ChapterFile {
                    path: path.clone(),
                    spans: Vec::new(),
                })
                .collect(),
            notes: Vec::new(),
            diagrams: Vec::new(),
            impact: Vec::new(),
        });
    }
    Ok(Finished {
        chapters: guide.chapters.len(),
        leftover,
    })
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::{load, repo};
    use super::*;
    use serde_json::json;

    fn chapter(title: &str, files: serde_json::Value) -> ChapterArgs {
        serde_json::from_value(json!({ "title": title, "summary": "Why it exists.", "files": files })).unwrap()
    }

    fn note(chapter: usize, file: &str, line: Option<u32>) -> NoteArgs {
        serde_json::from_value(json!({ "chapter": chapter, "file": file, "line": line, "text": "Mind this." })).unwrap()
    }

    fn diagram(source: &str) -> DiagramArgs {
        serde_json::from_value(json!({ "source": source })).unwrap()
    }

    struct Setup {
        _repo: tempfile::TempDir,
        files: Vec<FileDiff>,
        guide: Guide,
    }

    fn setup() -> Setup {
        let repo = repo();
        let files = load(repo.path());
        let guide = begin(&files, None);
        Setup {
            _repo: repo,
            files,
            guide,
        }
    }

    #[test]
    fn a_new_guide_carries_the_fingerprints_of_the_diff_it_describes() {
        let setup = setup();
        assert_eq!(setup.guide.fingerprint, stale::fingerprint(&setup.files));
        let paths: Vec<_> = setup.guide.file_fingerprints.keys().map(String::as_str).collect();
        assert_eq!(paths, ["notes.md", "src/send.ts", "src/timer.ts"]);
        assert!(setup.guide.is_empty());
    }

    #[test]
    fn the_overview_needs_a_title_and_a_summary_and_keeps_to_the_word_limits() {
        let mut setup = setup();
        let args = |title: &str, summary: &str| OverviewArgs {
            title: title.into(),
            summary: summary.into(),
            before: Some(" Sent at once. ".into()),
            after: Some("  ".into()),
        };
        assert!(set_overview(&mut setup.guide, &args("", "x")).is_err());
        let long = "word ".repeat(SUMMARY_WORDS + 1);
        let error = set_overview(&mut setup.guide, &args(&"t ".repeat(11), &long)).unwrap_err();
        assert!(
            error.contains(&format!("title is 11 words (max {OVERVIEW_TITLE_WORDS})")),
            "{error}"
        );
        assert!(
            error.contains(&format!("summary is {} words", SUMMARY_WORDS + 1)),
            "{error}"
        );
        assert!(
            setup.guide.overview.summary.is_empty(),
            "a refused call changes nothing"
        );

        set_overview(&mut setup.guide, &args("Undo send", "A message waits five seconds.")).unwrap();
        assert_eq!(
            setup.guide.overview,
            Overview {
                title: "Undo send".into(),
                summary: "A message waits five seconds.".into(),
                before: Some("Sent at once.".into()),
                after: None,
            }
        );
    }

    #[test]
    fn chapters_are_numbered_from_one_and_keep_the_order_of_their_files() {
        let mut setup = setup();
        let first = chapter("The timer", json!(["src/timer.ts", "./src/send.ts"]));
        assert_eq!(add_chapter(&mut setup.guide, &setup.files, &first), Ok(1));
        let second = chapter("Notes", json!(["notes.md"]));
        assert_eq!(add_chapter(&mut setup.guide, &setup.files, &second), Ok(2));
        let paths: Vec<_> = setup.guide.chapters[0]
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect();
        assert_eq!(paths, ["src/timer.ts", "src/send.ts"]);
        assert_eq!(setup.guide.chapters[0].attention, Attention::Normal);
    }

    #[test]
    fn a_chapter_with_a_file_outside_the_diff_names_the_files_that_are_in_it() {
        let mut setup = setup();
        let error = add_chapter(
            &mut setup.guide,
            &setup.files,
            &chapter("Bad", json!(["src/nope.ts", "notes.md"])),
        )
        .unwrap_err();
        assert!(
            error.contains("src/nope.ts") && error.contains("src/send.ts"),
            "{error}"
        );
        assert!(setup.guide.chapters.is_empty());
        let empty = add_chapter(&mut setup.guide, &setup.files, &chapter("Empty", json!([]))).unwrap_err();
        assert!(empty.contains("at least one file"), "{empty}");
    }

    #[test]
    fn a_file_belongs_to_one_chapter_unless_each_names_its_own_lines() {
        let mut setup = setup();
        let whole = chapter("Whole", json!(["src/send.ts"]));
        add_chapter(&mut setup.guide, &setup.files, &whole).unwrap();
        let again = add_chapter(
            &mut setup.guide,
            &setup.files,
            &chapter("Again", json!(["src/send.ts"])),
        )
        .unwrap_err();
        assert!(again.contains("already in chapter 1"), "{again}");
        let part = json!([{ "path": "src/send.ts", "ranges": [{ "startLine": 1, "endLine": 1 }] }]);
        let against_whole = add_chapter(&mut setup.guide, &setup.files, &chapter("Part", part.clone())).unwrap_err();
        assert!(against_whole.contains("already in chapter 1"), "{against_whole}");

        let mut split = setup.guide.clone();
        split.chapters.clear();
        let head = json!([{ "path": "src/send.ts", "ranges": [{ "startLine": 1, "endLine": 2 }] }]);
        let tail = json!([{ "path": "src/send.ts", "ranges": [{ "startLine": 5, "endLine": 6 }] }]);
        assert_eq!(add_chapter(&mut split, &setup.files, &chapter("Head", head)), Ok(1));
        assert_eq!(add_chapter(&mut split, &setup.files, &chapter("Tail", tail)), Ok(2));
        let span = &split.chapters[1].files[0].spans[0];
        assert_eq!((span.side, span.line, span.end_line), (Side::New, 5, Some(6)));
        assert_eq!(span.anchor_text.as_deref(), Some("  startTimer(id);\n  audit(id);"));
        let whole_after = add_chapter(&mut split, &setup.files, &chapter("Whole", json!(["src/send.ts"]))).unwrap_err();
        assert!(whole_after.contains("already in chapter"), "{whole_after}");
    }

    #[test]
    fn a_range_outside_the_hunks_is_refused() {
        let mut setup = setup();
        let far = json!([{ "path": "src/send.ts", "ranges": [{ "startLine": 40 }] }]);
        let error = add_chapter(&mut setup.guide, &setup.files, &chapter("Far", far)).unwrap_err();
        assert!(error.contains("not in the diff on the new side"), "{error}");
    }

    #[test]
    fn a_rename_is_found_by_its_old_path() {
        let setup = setup();
        let mut renamed = super::super::fixtures::file(&setup.files, "src/send.ts").clone();
        renamed.old_path = Some("src/old_send.ts".into());
        let mut guide = Guide::default();
        let found = chapter("Renamed", json!(["src/old_send.ts"]));
        add_chapter(&mut guide, &[renamed], &found).unwrap();
        assert_eq!(guide.chapters[0].files[0].path, "src/send.ts");
    }

    #[test]
    fn phases_must_sit_together_and_text_over_the_limits_is_listed_at_once() {
        let mut setup = setup();
        let with = |title: &str, phase: &str, file: &str| {
            let mut args = chapter(title, json!([file]));
            args.phase = Some(phase.into());
            args
        };
        add_chapter(&mut setup.guide, &setup.files, &with("One", "Core", "src/timer.ts")).unwrap();
        add_chapter(&mut setup.guide, &setup.files, &with("Two", "Docs", "notes.md")).unwrap();
        let error = add_chapter(&mut setup.guide, &setup.files, &with("Three", "Core", "src/send.ts")).unwrap_err();
        assert!(error.contains("phase \"Core\" must sit next to each other"), "{error}");

        let mut long = chapter("Long", json!(["src/send.ts"]));
        long.summary = "word ".repeat(CHAPTER_SUMMARY_WORDS + 1);
        long.points = vec!["a".into(); MAX_BULLETS + 1];
        long.focus = vec!["word ".repeat(BULLET_WORDS + 1)];
        let error = add_chapter(&mut setup.guide, &setup.files, &long).unwrap_err();
        assert!(
            error.contains("summary is 41 words")
                && error.contains("points has 4 items")
                && error.contains("focus item 1"),
            "{error}"
        );
    }

    #[test]
    fn impact_inside_the_diff_is_dropped_and_the_rest_is_kept() {
        let mut setup = setup();
        let mut args = chapter("Timer", json!(["src/timer.ts"]));
        args.impact = serde_json::from_value(json!([
            { "site": "send()", "path": "src/send.ts", "text": "In the diff already." },
            { "site": "outbox.flush()", "path": "./src/outbox.ts", "line": 9, "text": "Now waits for the timer." },
            { "site": "", "path": "src/x.ts", "text": "No site." }
        ]))
        .unwrap();
        add_chapter(&mut setup.guide, &setup.files, &args).unwrap();
        assert_eq!(
            setup.guide.chapters[0].impact,
            [Impact {
                folder: None,
                site: "outbox.flush()".into(),
                path: "src/outbox.ts".into(),
                line: Some(9),
                text: "Now waits for the timer.".into(),
            }]
        );
    }

    #[test]
    fn impact_can_point_into_a_linked_folder() {
        let linked = tempfile::tempdir().unwrap();
        let folder = linked.path().to_path_buf();
        std::fs::create_dir_all(folder.join("src")).unwrap();
        std::fs::write(folder.join("src/client.ts"), "x").unwrap();
        let mut setup = setup();
        setup.guide.context = vec![folder.clone()];
        let named = folder.display().to_string();
        let mut args = chapter("Timer", json!(["src/timer.ts"]));
        args.impact = serde_json::from_value(json!([
            { "folder": named, "site": "send()", "path": "src/client.ts", "line": 3, "text": "Sends seconds." },
            { "site": "flush()", "path": folder.join("src/client.ts"), "text": "Found by its full path." }
        ]))
        .unwrap();
        add_chapter(&mut setup.guide, &setup.files, &args).unwrap();
        let impact = &setup.guide.chapters[0].impact;
        assert_eq!(impact.len(), 2);
        for item in impact {
            assert_eq!(
                (item.folder.as_ref(), item.path.as_str()),
                (Some(&folder), "src/client.ts")
            );
        }

        let mut missing = chapter("Missing", json!(["src/send.ts"]));
        missing.impact = serde_json::from_value(json!([
            { "folder": named, "site": "x()", "path": "src/gone.ts", "text": "Gone." }
        ]))
        .unwrap();
        let error = add_chapter(&mut setup.guide, &setup.files, &missing).unwrap_err();
        assert!(
            error.contains("`src/gone.ts` is not a file in the linked folder"),
            "{error}"
        );

        let mut unknown = chapter("Unknown", json!(["src/send.ts"]));
        unknown.impact = serde_json::from_value(json!([
            { "folder": "/elsewhere", "site": "x()", "path": "a.ts", "text": "Where?" }
        ]))
        .unwrap();
        let error = add_chapter(&mut setup.guide, &setup.files, &unknown).unwrap_err();
        assert!(error.contains(&format!("Use one of: {named}")), "{error}");
    }

    #[test]
    fn a_note_is_pinned_to_a_line_of_one_of_the_chapters_files() {
        let mut setup = setup();
        add_chapter(&mut setup.guide, &setup.files, &chapter("Send", json!(["src/send.ts"]))).unwrap();
        let no_chapter = add_note(&mut setup.guide, &setup.files, &note(2, "src/send.ts", Some(5))).unwrap_err();
        assert!(no_chapter.contains("There is no chapter 2"), "{no_chapter}");
        let other_file = add_note(&mut setup.guide, &setup.files, &note(1, "notes.md", None)).unwrap_err();
        assert!(
            other_file.contains("not one of chapter 1's files: src/send.ts"),
            "{other_file}"
        );
        let outside = add_note(&mut setup.guide, &setup.files, &note(1, "src/send.ts", Some(90))).unwrap_err();
        assert!(outside.contains("not in the diff"), "{outside}");

        add_note(&mut setup.guide, &setup.files, &note(1, "src/send.ts", Some(5))).unwrap();
        add_note(&mut setup.guide, &setup.files, &note(1, "src/send.ts", None)).unwrap();
        let mut removed = note(1, "src/send.ts", Some(4));
        removed.side = Some(Side::Old);
        removed.critical = true;
        add_note(&mut setup.guide, &setup.files, &removed).unwrap();
        let notes = &setup.guide.chapters[0].notes;
        let span = notes[0].span.as_ref().unwrap();
        assert_eq!(
            (span.side, span.line, span.anchor_text.as_deref()),
            (Side::New, 5, Some("  startTimer(id);"))
        );
        assert_eq!(notes[1].span, None);
        let old = notes[2].span.as_ref().unwrap();
        assert_eq!(
            (old.side, old.anchor_text.as_deref(), notes[2].critical),
            (Side::Old, Some("  deliver(id);"), true)
        );

        let mut long = note(1, "src/send.ts", None);
        long.text = "word ".repeat(NOTE_WORDS + 1);
        assert!(
            add_note(&mut setup.guide, &setup.files, &long)
                .unwrap_err()
                .contains("the note is 41 words")
        );
    }

    #[test]
    fn a_diagram_must_parse_and_its_flow_must_name_its_nodes() {
        let mut setup = setup();
        add_chapter(&mut setup.guide, &setup.files, &chapter("Send", json!(["src/send.ts"]))).unwrap();

        let broken = add_diagram(&mut setup.guide, &diagram("flowchart LR\n  A --> B\n  B -->")).unwrap_err();
        assert!(
            broken.contains("Mermaid line 3") && broken.contains("Fix the Mermaid text"),
            "{broken}"
        );

        let mut flowing = diagram("flowchart LR\n  Send[Send]:::changed --> Timer[[Timer]]:::added");
        flowing.flow = vec![
            FlowArg {
                node: "Send".into(),
                text: "The user sends.".into(),
            },
            FlowArg {
                node: "Timer".into(),
                text: "A timer starts.".into(),
            },
        ];
        assert_eq!(add_diagram(&mut setup.guide, &flowing), Ok(DiagramKind::Flowchart));
        assert_eq!(setup.guide.diagrams[0].flow.len(), 2);
        assert!(matches!(setup.guide.diagrams[0].graph(), Ok(Graph::Flowchart(_))));

        flowing.flow[1].node = "Nope".into();
        let error = add_diagram(&mut setup.guide, &flowing).unwrap_err();
        assert!(error.contains("`Nope` is not in the diagram"), "{error}");
        assert_eq!(setup.guide.diagrams.len(), 1);

        let mut sequence = diagram("sequenceDiagram\n  UI->>Service: send");
        sequence.chapter = Some(1);
        sequence.flow = vec![FlowArg {
            node: "Service".into(),
            text: "Queues it.".into(),
        }];
        assert_eq!(add_diagram(&mut setup.guide, &sequence), Ok(DiagramKind::Sequence));
        assert_eq!(setup.guide.chapters[0].diagrams.len(), 1);

        let mut state = diagram("stateDiagram-v2\n  [*] --> Idle");
        state.chapter = Some(1);
        assert_eq!(
            add_diagram(&mut setup.guide, &state),
            Ok(DiagramKind::Other("stateDiagram-v2".into()))
        );
        state.chapter = Some(9);
        assert!(
            add_diagram(&mut setup.guide, &state)
                .unwrap_err()
                .contains("no chapter 9")
        );
    }

    #[test]
    fn a_scope_holds_a_few_diagrams_of_a_limited_size() {
        let mut setup = setup();
        for _ in 0..MAX_DIAGRAMS {
            add_diagram(&mut setup.guide, &diagram("flowchart LR\n  A --> B")).unwrap();
        }
        let full = add_diagram(&mut setup.guide, &diagram("flowchart LR\n  A --> B")).unwrap_err();
        assert!(full.contains("already 3 diagrams"), "{full}");
        let big = format!("flowchart LR\n{}", "  A --> B\n".repeat(MAX_DIAGRAM_BYTES));
        assert!(
            add_diagram(&mut Guide::default(), &diagram(&big))
                .unwrap_err()
                .contains("max")
        );
        assert!(add_diagram(&mut Guide::default(), &diagram("  ")).is_err());
    }

    #[test]
    fn finishing_needs_an_overview_and_a_chapter_and_collects_the_files_left_out() {
        let mut setup = setup();
        assert!(
            finish(&mut setup.guide, &setup.files)
                .unwrap_err()
                .contains("set_overview")
        );
        set_overview(
            &mut setup.guide,
            &OverviewArgs {
                title: "Undo send".into(),
                summary: "Waits.".into(),
                before: None,
                after: None,
            },
        )
        .unwrap();
        assert!(
            finish(&mut setup.guide, &setup.files)
                .unwrap_err()
                .contains("add_chapter")
        );
        add_chapter(
            &mut setup.guide,
            &setup.files,
            &chapter("Timer", json!(["src/timer.ts"])),
        )
        .unwrap();

        let done = finish(&mut setup.guide, &setup.files).unwrap();
        assert_eq!(done.chapters, 2);
        assert_eq!(done.leftover, ["src/send.ts", "notes.md"]);
        let last = setup.guide.chapters.last().unwrap();
        assert_eq!(
            (last.title.as_str(), last.attention),
            (LEFTOVER_CHAPTER, Attention::Low)
        );
        assert!(setup.files.iter().all(|file| setup.guide.covers(&file.path)));

        let again = finish(&mut setup.guide, &setup.files).unwrap();
        assert_eq!(
            (again.chapters, again.leftover.len()),
            (2, 0),
            "finishing twice adds nothing"
        );
    }
}
