use std::collections::BTreeMap;
use std::env;
use std::fs::File;
use std::io::BufReader;
use std::str::FromStr;

use anyhow::{anyhow, bail, ensure, Context, Error as AnyhowError, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use url::Url;

use crate::PushStr;

/// Top-level deserialized struct of .crates2.json.
/// Note: this metadata format probably isn't "stable" and future Cargo versions might break this.
#[derive(Debug, Deserialize)]
pub struct Crates2 {
    pub installs: BTreeMap<String, PackageDetails>,
}

impl Crates2 {
    /// Find and load Cargo's .crates2.json file
    pub fn load() -> Result<Self> {
        let mut path = match env::var_os("CARGO_HOME") {
            Some(s) => s.into(),
            None => {
                let mut dir = dirs::home_dir().ok_or_else(|| {
                    anyhow!("Unable to find home directory, and CARGO_HOME is unset")
                })?;
                dir.push(".cargo");
                dir
            }
        };
        path.push(".crates2.json");

        let file = BufReader::new(
            File::open(&path).with_context(|| format!("Failed to open '{}'", path.display()))?,
        );
        serde_json::from_reader(file)
            .with_context(|| format!("Failed to parse '{}'", path.display()))
    }
}

#[derive(Debug)]
pub enum PackageSource {
    /// Package installed from a registry with this URL
    Registry(String),
    /// Package installed from git using this URL and Revision
    Git { url: String, branch: Option<String>, tag: Option<String> },
    /// Package installed from the filesystem
    Path(String),
}

impl FromStr for PackageSource {
    type Err = AnyhowError;

    /// Parse the package source from the "kind+url" field of its ID string
    /// Examples:
    ///   from a registry: registry+https://github.com/rust-lang/crates.io-index
    ///   from git: git+https://github.com/aswild/bcut#046894ca312298f260775687a87bd1f3b7df8e55
    ///   from git with a particular branch: git+https://github.com/aswild/bcut?branch=master#046894c
    ///   from a local path: path+file:///workspace/cargo-update-installed
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // url::Url supports parsing arbitrary schemes (e.g. "git+https") but it doesn't allow
        // changing the scheme arbitrarily. Thus, we need to strip off the "kind+" prefix before
        // parsing the rest as a URL.
        let (kind, url) =
            s.split_once('+').ok_or_else(|| anyhow!("No package source kind found"))?;
        ensure!(!url.is_empty(), "Package source URL is empty");

        // now parse the rest as a url
        let mut url = Url::parse(url).context("Failed to parse package source URL")?;

        // git URLs put the revision in the fragment, which we don't actually care about - yeet it
        url.set_fragment(None);

        // git URLs put the branch/tag into the query params, which we do want to save
        let mut branch = None;
        let mut tag = None;
        for (key, val) in url.query_pairs() {
            match key.as_ref() {
                "branch" => branch = Some(val.into_owned()),
                "tag" => tag = Some(val.into_owned()),
                k => bail!("Unknown URL query parameter '{}'", k),
            }
        }
        // yeet the query parameters now that we've saved them
        url.set_query(None);

        Ok(match kind {
            "registry" => Self::Registry(url.into()),
            "git" => Self::Git { url: url.into(), branch, tag },
            "path" => Self::Path(url.path().to_owned()),
            k => bail!("Unknown package source kind '{}'", k),
        })
    }
}

impl PackageSource {
    pub fn add_cargo_args(&self, args: &mut Vec<String>) {
        match self {
            Self::Registry(url) => args.push_str("--index").push_str(url),
            Self::Git { url, branch, tag } => {
                args.push_str("--git").push_str(url);
                if let Some(b) = branch {
                    args.push_str("--branch").push_str(b);
                }
                if let Some(t) = tag {
                    args.push_str("--tag").push_str(t);
                }
                args
            }
            Self::Path(path) => args.push_str("--path").push_str(path),
        };
    }
}

#[derive(Debug)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub source: PackageSource,
}

impl FromStr for Package {
    type Err = AnyhowError;

    /// Parse the Package ID string which is of the form "name version (kind+url)". Examples:
    /// bat 0.18.0 (registry+https://github.com/rust-lang/crates.io-index)
    /// bcut 1.0.2 (git+https://github.com/aswild/bcut#046894ca312298f260775687a87bd1f3b7df8e55)
    /// cargo-update-installed 0.1.0 (path+file:///workspace/cargo-update-installed)
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(\S+) (\S+) \((.+)\)$").unwrap());

        let m = RE.captures(s).ok_or_else(|| anyhow!("couldn't parse package name '{}'", s))?;
        Ok(Self {
            name: m.get(1).unwrap().as_str().into(),
            version: m.get(2).unwrap().as_str().into(),
            source: m.get(3).unwrap().as_str().parse()?,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct PackageDetails {
    pub version_req: Option<String>,
    pub bins: Vec<String>,
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    pub profile: String,
    pub target: String,
    pub rustc: String,
}

impl PackageDetails {
    pub fn add_cargo_args(&self, args: &mut Vec<String>) {
        if !self.features.is_empty() {
            args.push_str("--features").push_str(self.features.join(","));
        }
        if self.all_features {
            args.push_str("--all-features");
        }
        if self.no_default_features {
            args.push_str("--no-default-features");
        }
        //args.push_str("--profile").push_str(&self.profile); // --profile is unstable, omit it
        args.push_str("--target").push_str(&self.target);
    }
}
