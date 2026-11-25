// Adapted from diffity (MIT License, Copyright (c) 2026 Kamran Ahmed).
// The license text is in core/THIRD_PARTY_NOTICES.

pub const GITHUB_HOST: &str = "github.com";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoSlug {
    pub host: String,
    pub owner: String,
    pub name: String,
}

impl RepoSlug {
    pub fn matches(&self, other: &RepoSlug) -> bool {
        self.host.eq_ignore_ascii_case(&other.host)
            && self.owner.eq_ignore_ascii_case(&other.owner)
            && self.name.eq_ignore_ascii_case(&other.name)
    }

    pub fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    pub fn web_url(&self) -> String {
        format!("https://{}/{}/{}", self.host, self.owner, self.name)
    }

    pub fn pull_url(&self, number: u64) -> String {
        format!("{}/pull/{number}", self.web_url())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub graphql: String,
    pub rest: String,
}

impl Endpoint {
    pub fn for_host(host: &str) -> Self {
        if host.eq_ignore_ascii_case(GITHUB_HOST) {
            return Self {
                graphql: "https://api.github.com/graphql".into(),
                rest: "https://api.github.com".into(),
            };
        }
        Self {
            graphql: format!("https://{host}/api/graphql"),
            rest: format!("https://{host}/api/v3"),
        }
    }
}

/// `enterprise_hosts` are the GitHub Enterprise Server hosts the user signed in to.
pub fn parse_remote_url(url: &str, enterprise_hosts: &[String]) -> Option<RepoSlug> {
    let url = url.trim();
    let (host, path) = if let Some((_, rest)) = url.split_once("://") {
        let (authority, path) = rest.split_once('/')?;
        let authority = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
        let host = authority.split(':').next().unwrap_or(authority);
        (host, path)
    } else {
        let rest = url.rsplit_once('@').map_or(url, |(_, rest)| rest);
        rest.split_once(':')?
    };
    let host = known_host(host, enterprise_hosts)?;
    slug_from_path(host, path)
}

fn known_host(host: &str, enterprise_hosts: &[String]) -> Option<String> {
    let lower = host.to_ascii_lowercase();
    if matches!(lower.as_str(), "github.com" | "www.github.com" | "ssh.github.com") {
        return Some(GITHUB_HOST.to_string());
    }
    enterprise_hosts
        .iter()
        .any(|known| known.eq_ignore_ascii_case(&lower))
        .then_some(lower)
}

fn slug_from_path(host: String, path: &str) -> Option<RepoSlug> {
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut parts = path.split('/');
    let owner = parts.next()?;
    let name = parts.next()?;
    if owner.is_empty() || name.is_empty() || parts.next().is_some() {
        return None;
    }
    Some(RepoSlug {
        host,
        owner: owner.to_string(),
        name: name.to_string(),
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrRef {
    pub slug: RepoSlug,
    pub number: u64,
}

pub fn parse_pr_url(input: &str, enterprise_hosts: &[String]) -> Option<PrRef> {
    let input = input.trim();
    let rest = input
        .strip_prefix("https://")
        .or_else(|| input.strip_prefix("http://"))
        .unwrap_or(input);
    let (host, rest) = rest.split_once('/')?;
    let host = known_host(host, enterprise_hosts)?;
    let mut parts = rest.split('/');
    let owner = parts.next()?;
    let name = parts.next()?;
    if parts.next()? != "pull" {
        return None;
    }
    let digits: String = parts.next()?.chars().take_while(char::is_ascii_digit).collect();
    let number = digits.parse().ok()?;
    if owner.is_empty() || name.is_empty() {
        return None;
    }
    Some(PrRef {
        slug: RepoSlug {
            host,
            owner: owner.to_string(),
            name: name.strip_suffix(".git").unwrap_or(name).to_string(),
        },
        number,
    })
}

/// The remote pull requests are opened against: the one `gh repo set-default` picked, else `upstream`
/// (a fork setup, where `origin` is your fork), else `origin`.
pub fn choose_pr_remote(remotes: &[String], gh_resolved: &str) -> Option<String> {
    let gh_default = gh_resolved.lines().find_map(|line| {
        let (key, value) = line.split_once(char::is_whitespace)?;
        let name = key.strip_prefix("remote.")?.strip_suffix(".gh-resolved")?;
        (value.trim() == "base").then_some(name)
    });
    let has = |name: &str| remotes.iter().any(|remote| remote == name);
    if let Some(name) = gh_default.filter(|name| has(name)) {
        return Some(name.to_string());
    }
    ["upstream", "origin"]
        .into_iter()
        .find(|name| has(name))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slug(host: &str, owner: &str, name: &str) -> Option<RepoSlug> {
        Some(RepoSlug {
            host: host.into(),
            owner: owner.into(),
            name: name.into(),
        })
    }

    fn ghe() -> Vec<String> {
        vec!["git.corp.example".to_string()]
    }

    #[test]
    fn parses_github_com_remote_forms() {
        let gh = |url: &str| parse_remote_url(url, &[]);
        let expected = slug("github.com", "a", "b");
        for url in [
            "https://github.com/a/b.git",
            "https://github.com/a/b",
            "https://github.com/a/b/",
            "https://user:tok@github.com/a/b.git",
            "git@github.com:a/b.git",
            "ssh://git@github.com/a/b.git",
            "ssh://git@ssh.github.com:443/a/b.git",
            "git://github.com/a/b",
            "https://www.GitHub.com/a/b",
        ] {
            assert_eq!(gh(url), expected, "{url}");
        }
        assert_eq!(gh("git@github.com:a/my.repo.git"), slug("github.com", "a", "my.repo"));
    }

    #[test]
    fn parses_enterprise_hosts_only_when_they_are_known() {
        assert_eq!(
            parse_remote_url("git@git.corp.example:team/app.git", &ghe()),
            slug("git.corp.example", "team", "app")
        );
        assert_eq!(
            parse_remote_url("https://git.corp.example/team/app", &ghe()),
            slug("git.corp.example", "team", "app")
        );
        assert_eq!(
            parse_remote_url("ssh://git@git.corp.example:2222/team/app.git", &ghe()),
            slug("git.corp.example", "team", "app")
        );
        assert_eq!(parse_remote_url("git@git.corp.example:team/app.git", &[]), None);
    }

    #[test]
    fn rejects_other_hosts_and_bad_paths() {
        for url in [
            "git@gitlab.com:a/b.git",
            "https://example.com/a/b",
            "https://github.com/a",
            "https://github.com/a/b/c",
            "/local/path/repo",
            "",
        ] {
            assert_eq!(parse_remote_url(url, &ghe()), None, "{url}");
        }
    }

    #[test]
    fn parses_pull_request_urls() {
        let pr = parse_pr_url("https://github.com/o/r/pull/42", &[]).unwrap();
        assert_eq!((pr.number, pr.slug.full_name()), (42, "o/r".to_string()));
        assert_eq!(parse_pr_url("github.com/o/r/pull/7/files", &[]).unwrap().number, 7);
        assert_eq!(
            parse_pr_url("https://github.com/o/r/pull/9#discussion_r1", &[])
                .unwrap()
                .number,
            9
        );
        assert_eq!(
            parse_pr_url("https://git.corp.example/t/a/pull/3", &ghe())
                .unwrap()
                .slug
                .host,
            "git.corp.example"
        );
        assert!(parse_pr_url("https://github.com/o/r/issues/9", &[]).is_none());
        assert!(parse_pr_url("https://git.corp.example/t/a/pull/3", &[]).is_none());
        assert!(parse_pr_url("42", &[]).is_none());
    }

    #[test]
    fn picks_the_endpoint_of_each_host() {
        assert_eq!(
            Endpoint::for_host("github.com").graphql,
            "https://api.github.com/graphql"
        );
        let corp = Endpoint::for_host("git.corp.example");
        assert_eq!(corp.graphql, "https://git.corp.example/api/graphql");
        assert_eq!(corp.rest, "https://git.corp.example/api/v3");
    }

    #[test]
    fn prefers_the_gh_default_then_upstream_then_origin() {
        let remotes: Vec<String> = ["origin", "upstream", "mine"].map(String::from).to_vec();
        assert_eq!(
            choose_pr_remote(&remotes, "remote.mine.gh-resolved base\nremote.origin.gh-resolved head"),
            Some("mine".into())
        );
        assert_eq!(choose_pr_remote(&remotes, ""), Some("upstream".into()));
        assert_eq!(choose_pr_remote(&remotes[..1], ""), Some("origin".into()));
        assert_eq!(choose_pr_remote(&["fork".to_string()], ""), None);
    }

    #[test]
    fn builds_web_urls() {
        let repo = slug("github.com", "o", "r").unwrap();
        assert_eq!(repo.pull_url(5), "https://github.com/o/r/pull/5");
    }
}
