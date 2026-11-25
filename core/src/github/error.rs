use crate::git::GitError;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GithubError {
    #[error("not signed in to {0}")]
    NoToken(String),
    #[error("GitHub rejected the token")]
    Unauthorized,
    #[error("GitHub refused the request: {0}")]
    Forbidden(String),
    #[error("{0} was not found")]
    NotFound(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("unexpected GitHub response: {0}")]
    Response(String),
    #[error("GitHub: {0}")]
    Api(String),
    #[error("the `{0}` remote is not a GitHub repository")]
    NotGithub(String),
    #[error("the repository has no `origin` or `upstream` remote")]
    NoRemote,
    #[error("keychain: {0}")]
    Keychain(String),
    #[error("{0}")]
    Invalid(String),
    #[error("there are uncommitted changes to tracked files; commit or stash them first")]
    Dirty,
    #[error("the branch is already checked out in {0}")]
    InWorktree(String),
    #[error("could not fetch {0}")]
    FetchFailed(String),
    #[error(transparent)]
    Git(#[from] GitError),
}
