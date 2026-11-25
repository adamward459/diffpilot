//! Read-only check of the GitHub client: `cargo run -p diffpilot-core --example github_smoke -- owner/name`.

use diffpilot_core::github::Github;
use diffpilot_core::github::auth::Credentials;
use diffpilot_core::github::http::UreqHttp;
use diffpilot_core::github::remote::{GITHUB_HOST, RepoSlug};
use std::sync::Arc;

fn main() {
    let target = std::env::args().nth(1).expect("usage: github_smoke owner/name");
    let (owner, name) = target.split_once('/').expect("owner/name");
    let slug = RepoSlug {
        host: GITHUB_HOST.into(),
        owner: owner.into(),
        name: name.into(),
    };
    let http = Arc::new(UreqHttp::new());
    let credentials = Credentials::system();
    let status = Github::auth_status(http.clone(), &credentials, GITHUB_HOST);
    println!("signed in as {:?} via {:?}", status.login, status.source);
    let github = Github::connect(http, &credentials, GITHUB_HOST).expect("a token");
    for pr in github.list_pull_requests(&slug).expect("list") {
        println!(
            "#{} {:?} checks={:?} review={:?} {} -> {}  {}",
            pr.number, pr.state, pr.checks, pr.review_decision, pr.head_ref, pr.base_ref, pr.title
        );
    }
}
