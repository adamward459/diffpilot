use gpui::{App, Global, PathPromptOptions};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

pub type Picked = Pin<Box<dyn Future<Output = Option<Vec<PathBuf>>>>>;

/// Answers for the folder picker, in order. The OS picker cannot be driven in tests. `None` is a cancel.
#[derive(Default)]
pub struct CannedPicks(pub RefCell<VecDeque<Option<Vec<PathBuf>>>>);

impl Global for CannedPicks {}

pub fn pick_folders(multiple: bool, cx: &App) -> Picked {
    if let Some(canned) = cx.try_global::<CannedPicks>() {
        let answer = canned.0.borrow_mut().pop_front().flatten();
        return Box::pin(std::future::ready(answer));
    }
    let picked = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple,
        prompt: None,
    });
    Box::pin(async move { picked.await.ok().and_then(Result::ok).flatten() })
}
