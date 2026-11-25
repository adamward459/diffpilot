// Adapted from diffity (MIT License, Copyright (c) 2026 Kamran Ahmed).
// The license text is in core/THIRD_PARTY_LICENSES.

use super::error::GithubError;
use crate::paths::find_executable;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

const KEYCHAIN_SERVICE: &str = "com.diffpilot.app";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenSource {
    Keychain,
    Gh,
}

pub trait TokenStore: Send + Sync {
    fn read(&self, host: &str) -> Result<Option<String>, GithubError>;
    fn write(&self, host: &str, token: &str) -> Result<(), GithubError>;
    fn delete(&self, host: &str) -> Result<(), GithubError>;
}

pub trait GhCli: Send + Sync {
    fn token(&self, host: &str) -> Option<String>;
}

pub struct KeychainStore {
    service: String,
}

impl KeychainStore {
    pub fn new() -> Self {
        Self::with_service(KEYCHAIN_SERVICE)
    }

    pub fn with_service(service: &str) -> Self {
        Self {
            service: service.to_string(),
        }
    }
}

impl Default for KeychainStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "macos")]
mod keychain {
    use super::{GithubError, KeychainStore, TokenStore};
    use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};

    const ITEM_NOT_FOUND: i32 = -25300;

    fn failure(error: security_framework::base::Error) -> GithubError {
        GithubError::Keychain(error.to_string())
    }

    impl TokenStore for KeychainStore {
        fn read(&self, host: &str) -> Result<Option<String>, GithubError> {
            match get_generic_password(&self.service, host) {
                Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
                Err(error) if error.code() == ITEM_NOT_FOUND => Ok(None),
                Err(error) => Err(failure(error)),
            }
        }

        fn write(&self, host: &str, token: &str) -> Result<(), GithubError> {
            set_generic_password(&self.service, host, token.as_bytes()).map_err(failure)
        }

        fn delete(&self, host: &str) -> Result<(), GithubError> {
            match delete_generic_password(&self.service, host) {
                Ok(()) => Ok(()),
                Err(error) if error.code() == ITEM_NOT_FOUND => Ok(()),
                Err(error) => Err(failure(error)),
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl TokenStore for KeychainStore {
    fn read(&self, _host: &str) -> Result<Option<String>, GithubError> {
        Err(GithubError::Keychain("the keychain is only available on macOS".into()))
    }

    fn write(&self, _host: &str, _token: &str) -> Result<(), GithubError> {
        self.read("").map(|_| ())
    }

    fn delete(&self, _host: &str) -> Result<(), GithubError> {
        self.read("").map(|_| ())
    }
}

#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<String, String>>);

impl TokenStore for MemoryStore {
    fn read(&self, host: &str) -> Result<Option<String>, GithubError> {
        Ok(self.0.lock().unwrap().get(host).cloned())
    }

    fn write(&self, host: &str, token: &str) -> Result<(), GithubError> {
        self.0.lock().unwrap().insert(host.to_string(), token.to_string());
        Ok(())
    }

    fn delete(&self, host: &str) -> Result<(), GithubError> {
        self.0.lock().unwrap().remove(host);
        Ok(())
    }
}

pub struct GhCommand;

const GH_CANDIDATES: [&str; 2] = ["/opt/homebrew/bin/gh", "/usr/local/bin/gh"];

impl GhCli for GhCommand {
    fn token(&self, host: &str) -> Option<String> {
        let candidates: Vec<PathBuf> = GH_CANDIDATES.iter().map(PathBuf::from).collect();
        let binary = find_executable(&candidates, "gh")?;
        let output = Command::new(binary)
            .args(["auth", "token", "--hostname", host])
            .env("GH_PROMPT_DISABLED", "1")
            .stdin(Stdio::null())
            .output()
            .ok()?;
        let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
        (output.status.success() && !token.is_empty()).then_some(token)
    }
}

#[derive(Clone)]
pub struct Credentials {
    store: Arc<dyn TokenStore>,
    gh: Arc<dyn GhCli>,
}

impl Credentials {
    pub fn new(store: Arc<dyn TokenStore>, gh: Arc<dyn GhCli>) -> Self {
        Self { store, gh }
    }

    pub fn system() -> Self {
        Self::new(Arc::new(KeychainStore::new()), Arc::new(GhCommand))
    }

    /// The token saved in the keychain, else the one the `gh` CLI holds.
    pub fn token(&self, host: &str) -> Result<Option<(String, TokenSource)>, GithubError> {
        if let Some(token) = self.store.read(host)? {
            return Ok(Some((token, TokenSource::Keychain)));
        }
        Ok(self.gh.token(host).map(|token| (token, TokenSource::Gh)))
    }

    pub fn save(&self, host: &str, token: &str) -> Result<(), GithubError> {
        let token = token.trim();
        if token.is_empty() {
            return Err(GithubError::Invalid("the token is empty".into()));
        }
        self.store.write(host, token)
    }

    pub fn import_from_gh(&self, host: &str) -> Result<String, GithubError> {
        let token = self
            .gh
            .token(host)
            .ok_or_else(|| GithubError::Invalid(format!("`gh` has no token for {host}; run `gh auth login`")))?;
        self.store.write(host, &token)?;
        Ok(token)
    }

    pub fn sign_out(&self, host: &str) -> Result<(), GithubError> {
        self.store.delete(host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeGh(Option<&'static str>);

    impl GhCli for FakeGh {
        fn token(&self, _host: &str) -> Option<String> {
            self.0.map(String::from)
        }
    }

    fn credentials(gh: Option<&'static str>) -> (Credentials, Arc<MemoryStore>) {
        let store = Arc::new(MemoryStore::default());
        (Credentials::new(store.clone(), Arc::new(FakeGh(gh))), store)
    }

    #[test]
    fn prefers_the_keychain_over_gh() {
        let (credentials, _) = credentials(Some("gho_from_gh"));
        assert_eq!(
            credentials.token("github.com").unwrap(),
            Some(("gho_from_gh".into(), TokenSource::Gh))
        );
        credentials.save("github.com", "  ghp_saved \n").unwrap();
        assert_eq!(
            credentials.token("github.com").unwrap(),
            Some(("ghp_saved".into(), TokenSource::Keychain))
        );
        assert_eq!(
            credentials.token("git.corp.example").unwrap().map(|found| found.1),
            Some(TokenSource::Gh)
        );
    }

    #[test]
    fn imports_the_gh_token_into_the_keychain() {
        let (credentials, store) = credentials(Some("gho_x"));
        assert_eq!(credentials.import_from_gh("github.com").unwrap(), "gho_x");
        assert_eq!(store.read("github.com").unwrap().as_deref(), Some("gho_x"));
        credentials.sign_out("github.com").unwrap();
        assert_eq!(store.read("github.com").unwrap(), None);
    }

    #[test]
    fn signed_out_without_gh_has_no_token() {
        let (credentials, _) = credentials(None);
        assert_eq!(credentials.token("github.com").unwrap(), None);
        assert!(matches!(
            credentials.import_from_gh("github.com"),
            Err(GithubError::Invalid(_))
        ));
        assert!(matches!(
            credentials.save("github.com", "  "),
            Err(GithubError::Invalid(_))
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "writes to the login keychain"]
    fn keychain_round_trip() {
        let store = KeychainStore::with_service("com.diffpilot.tests");
        let host = format!("test-{}.invalid", std::process::id());
        assert_eq!(store.read(&host).unwrap(), None);
        store.write(&host, "ghp_first").unwrap();
        store.write(&host, "ghp_second").unwrap();
        assert_eq!(store.read(&host).unwrap().as_deref(), Some("ghp_second"));
        store.delete(&host).unwrap();
        store.delete(&host).unwrap();
        assert_eq!(store.read(&host).unwrap(), None);
    }
}
