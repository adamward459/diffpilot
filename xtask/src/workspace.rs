use crate::{Result, root, run};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

pub const ROOT_FILES: [&str; 6] = [
    "Cargo.toml",
    "Cargo.lock",
    "rustfmt.toml",
    "rust-toolchain.toml",
    "clippy.toml",
    ".cargo/config.toml",
];

pub struct Crate {
    pub package: String,
    pub folder: String,
    pub has_lib: bool,
    pub deps: Vec<String>,
}

pub struct Workspace {
    pub crates: Vec<Crate>,
    pub build_dir: PathBuf,
}

impl Crate {
    pub fn contains(&self, path: &str) -> bool {
        path.strip_prefix(&self.folder)
            .is_some_and(|rest| rest.starts_with('/'))
    }

    pub fn builds_into_dependents(&self, path: &str) -> bool {
        let folder = &self.folder;
        ["src", "assets"]
            .iter()
            .any(|dir| path.starts_with(&format!("{folder}/{dir}/")))
            || path == format!("{folder}/Cargo.toml")
            || path == format!("{folder}/build.rs")
    }
}

impl Workspace {
    pub fn load() -> Result<Self> {
        let meta: Value = serde_json::from_str(&run(Command::new("cargo").args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
        ]))?)?;
        let packages = meta["packages"].as_array().ok_or("cargo metadata has no packages")?;
        let names: BTreeSet<&str> = packages.iter().filter_map(|p| p["name"].as_str()).collect();
        let root = root();
        let mut crates: Vec<Crate> = packages
            .iter()
            .map(|p| {
                let manifest = PathBuf::from(p["manifest_path"].as_str().unwrap_or_default());
                let folder = manifest.parent().and_then(|d| d.strip_prefix(&root).ok());
                Crate {
                    package: p["name"].as_str().unwrap_or_default().to_owned(),
                    folder: folder.map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(),
                    has_lib: p["targets"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|t| t["kind"].as_array().is_some_and(|k| k.iter().any(|k| k == "lib"))),
                    deps: p["dependencies"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|d| d["name"].as_str())
                        .filter(|d| names.contains(d))
                        .map(str::to_owned)
                        .collect(),
                }
            })
            .collect();
        crates.sort_by(|a, b| a.folder.cmp(&b.folder));
        let build_dir = meta["build_directory"]
            .as_str()
            .or(meta["target_directory"].as_str())
            .ok_or("cargo metadata has no build directory")?;
        Ok(Self {
            crates,
            build_dir: build_dir.into(),
        })
    }

    pub fn get(&self, package: &str) -> Option<&Crate> {
        self.crates.iter().find(|c| c.package == package)
    }

    pub fn resolve(&self, name: &str) -> Result<&Crate> {
        self.crates
            .iter()
            .find(|c| c.package == name || c.folder == name)
            .ok_or_else(|| {
                let known: Vec<_> = self.crates.iter().map(|c| c.folder.as_str()).collect();
                format!("unknown crate {name:?}; known: {}", known.join(", ")).into()
            })
    }

    pub fn closure(&self, package: &str) -> BTreeSet<&str> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![package];
        while let Some(name) = stack.pop() {
            if let Some(c) = self.get(name).filter(|c| seen.insert(c.package.as_str())) {
                stack.extend(c.deps.iter().map(String::as_str));
            }
        }
        seen
    }

    pub fn dependents(&self, package: &str) -> BTreeSet<&str> {
        self.crates
            .iter()
            .filter(|c| self.closure(&c.package).contains(package))
            .map(|c| c.package.as_str())
            .collect()
    }

    pub fn affected(&self, path: &str) -> BTreeSet<&str> {
        if ROOT_FILES.contains(&path) {
            return self.crates.iter().map(|c| c.package.as_str()).collect();
        }
        let mut out = BTreeSet::new();
        for c in self.crates.iter().filter(|c| c.contains(path)) {
            out.insert(c.package.as_str());
            if c.builds_into_dependents(path) {
                out.extend(self.dependents(&c.package));
            }
        }
        out
    }

    pub fn touched(&self, files: &[String]) -> BTreeSet<&str> {
        files.iter().flat_map(|f| self.affected(f)).collect()
    }

    pub fn built(&self, files: &[String]) -> BTreeSet<&str> {
        let mut out = BTreeSet::new();
        for f in files {
            if ROOT_FILES.contains(&f.as_str()) {
                out.extend(self.crates.iter().map(|c| c.package.as_str()));
            }
            for c in self.crates.iter().filter(|c| c.contains(f)) {
                if !f.contains("/tests/") {
                    out.insert(c.package.as_str());
                }
                if c.builds_into_dependents(f) {
                    out.extend(self.dependents(&c.package));
                }
            }
        }
        out
    }
}
