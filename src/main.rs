use std::collections::HashMap;
use std::convert::TryFrom;
use std::env;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{anyhow, bail, Error as AnyhowError, Result};
use lazy_static::lazy_static;
use regex::Regex;
use serde::Deserialize;
use url::Url;

#[derive(Debug, Deserialize)]
struct Crates2 {
    installs: HashMap<PackageSpec, Details>,
}

#[derive(Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(try_from = "String")]
struct PackageSpec {
    name: String,
    version: String,
    source: PackageSource,
    url: Url,
}

impl FromStr for PackageSpec {
    type Err = AnyhowError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        lazy_static! {
            static ref RE: Regex = Regex::new(r"^(\S+) (\S+) \(([^+]+)\+(.+)\)$").unwrap();
        }
        let m = RE
            .captures(s)
            .ok_or_else(|| anyhow!("couldn't parse package name '{}'", s))?;
        Ok(Self {
            name: m.get(1).unwrap().as_str().into(),
            version: m.get(2).unwrap().as_str().into(),
            source: m.get(3).unwrap().as_str().parse()?,
            url: m.get(4).unwrap().as_str().parse()?,
        })
    }
}

impl TryFrom<String> for PackageSpec {
    type Error = <Self as FromStr>::Err;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Deserialize)]
enum PackageSource {
    Registry,
    Git,
    Path,
}

impl FromStr for PackageSource {
    type Err = AnyhowError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "registry" => Self::Registry,
            "git" => Self::Git,
            "path" => Self::Path,
            s => bail!("Invalid package source '{}'", s),
        })
    }
}

#[derive(Debug, Deserialize)]
struct Details {
    version_req: Option<String>,
    bins: Vec<String>,
    features: Vec<String>,
    all_features: bool,
    no_default_features: bool,
    profile: String,
    target: String,
    rustc: String,
}

fn crates2_json_path() -> PathBuf {
    let mut path = match env::var_os("CARGO_HOME") {
        Some(s) => s.into(),
        None => {
            let mut p: PathBuf = env::var_os("HOME").unwrap().into();
            p.push(".cargo");
            p
        }
    };
    path.push(".crates2.json");
    path
}

fn run() -> Result<()> {
    let file = BufReader::new(File::open(&crates2_json_path())?);
    let parsed = serde_json::from_reader::<_, Crates2>(file)?;
    dbg!(&parsed);

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {:#}", e);
        std::process::exit(1);
    }
}
