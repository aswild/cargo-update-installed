use std::env;
use std::io::Write;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use glob::Pattern;
use is_terminal::IsTerminal;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

mod package_data;
use package_data::*;

const SUBCOMMAND_NAME: &str = "update-installed";

static USE_COLOR: AtomicBool = AtomicBool::new(false);
static VERBOSE: AtomicBool = AtomicBool::new(false);

// macros for printing colored stuff.

macro_rules! dbgmsg {
    ($($arg:tt)*) => {
        if VERBOSE.load(Ordering::Relaxed) {
            eprintln!($($arg)*);
        }
    };
}

macro_rules! msg {
    ($($arg:tt)*) => {
        color_println(Color::Cyan, format_args!($($arg)*));
    };
}

macro_rules! errmsg {
    ($($arg:tt)*) => {
        color_println(Color::Red, format_args!($($arg)*));
    };
}

#[allow(unused_must_use)]
fn color_println(color: Color, fargs: std::fmt::Arguments) {
    if USE_COLOR.load(Ordering::Relaxed) {
        let mut out = StandardStream::stderr(ColorChoice::Always);
        out.set_color(ColorSpec::new().set_fg(Some(color)));
        writeln!(out, "{}", fargs);
        out.reset();
    } else {
        eprintln!("{}", fargs);
    }
}

/// give Vec<String> builder semantics to work like std::process::Command::arg()
pub trait PushStr {
    fn push_str(&mut self, s: impl AsRef<str>) -> &mut Self;
}

impl PushStr for Vec<String> {
    fn push_str(&mut self, s: impl AsRef<str>) -> &mut Self {
        self.push(String::from(s.as_ref()));
        self
    }
}

/// Update all local packages installed by Cargo.
///
/// Read Cargo's metadata to list all local user-installed Rust packages and run `cargo install` on
/// them again to update to the latest version.
#[derive(Debug, Parser)]
#[command(bin_name = "cargo update-installed", no_binary_name = true, version)]
struct Args {
    /// Include matching packages
    ///
    /// PATTERN is a glob pattern matched against the package's name. If any include patterns are
    /// specified, then include patches which match any of them. If no include patterns are
    /// specified, then include all installed packages.
    #[arg(short, long, value_name = "PATTERN")]
    include: Vec<Pattern>,

    /// Exclude matching packages
    ///
    /// Like --include, but exclude packages with matching names. --exclude overrides --include.
    #[arg(short, long, value_name = "PATTERN")]
    exclude: Vec<Pattern>,

    /// Force reinstalling up-to-date packages (i.e. pass `--force` to `cargo install`).
    #[arg(short, long)]
    force: bool,

    /// Honor Cargo.lock in the source (i.e. pass `--locked` to `cargo install`).
    ///
    /// By default, `cargo install` builds with the latest semver-compatible versions of
    /// dependencies, ignoring any Cargo.lock file in the source repository.
    #[arg(short = 'L', long)]
    locked: bool,

    /// Dry-run: only list packages which we would attempt to update.
    #[arg(short = 'n', long)]
    dry_run: bool,

    /// Enable verbose output, including the full cargo commands executed.
    #[arg(short, long)]
    verbose: bool,
}

impl Args {
    /// Parse Args, handling both cases when being running directly and as a cargo subcommand.
    /// In subcommand mode, cargo sets argv[1] to "update-installed", which we skip.
    fn parse() -> Self {
        // always skip argv[0], used with no_binary_name
        let mut args = env::args_os().skip(1).peekable();
        if let Some(Some(SUBCOMMAND_NAME)) = args.peek().map(|s| s.to_str()) {
            args.next();
        }
        <Self as Parser>::parse_from(args)
    }

    /// Decide whether to include a package, based on --include/--exclude globs
    fn should_include(&self, s: &str) -> bool {
        if self.exclude.iter().any(|p| p.matches(s)) {
            false
        } else if self.include.is_empty() {
            true
        } else {
            self.include.iter().any(|p| p.matches(s))
        }
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    VERBOSE.store(args.verbose, Ordering::Relaxed);
    USE_COLOR.store(std::io::stdout().is_terminal(), Ordering::Relaxed);

    let crates2 = Crates2::load().context("Failed to load .crates2.json")?;

    let cargo_exe = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    dbgmsg!("Using Cargo executable '{}'", cargo_exe.to_string_lossy());

    let mut failed = Vec::new();
    for (pkg_id, details) in crates2.installs.iter() {
        let pkg = pkg_id
            .parse::<Package>()
            .with_context(|| format!("Failed to parse package id '{}'", pkg_id))?;

        if !args.should_include(&pkg.name) {
            msg!("Skipping {}", pkg.name);
            continue;
        }

        let mut cargo_args = vec!["install".to_owned()];
        if args.force {
            cargo_args.push_str("--force");
        }
        if args.locked {
            cargo_args.push_str("--locked");
        }
        details.add_cargo_args(&mut cargo_args);
        pkg.source.add_cargo_args(&mut cargo_args);
        cargo_args.push_str(&pkg.name);

        let mut cmd = Command::new(&cargo_exe);
        cmd.args(&cargo_args);

        msg!("Updating {}", pkg.name);
        dbgmsg!("{} {}", cargo_exe.to_string_lossy(), cargo_args.join(" "));

        if args.dry_run {
            continue;
        }

        let status = cmd.status().context("Failed to execute `cargo install ...`")?;

        if !status.success() {
            errmsg!("Error: failed to install '{}'", pkg.name);
            failed.push(pkg.name.clone());
        }
    }

    if failed.is_empty() {
        Ok(())
    } else {
        Err(anyhow!("Failed to install some packages: {}", failed.join(", ")))
    }
}

fn main() {
    if let Err(e) = run() {
        errmsg!("Error: {:#}", e);
        std::process::exit(1);
    }
}
