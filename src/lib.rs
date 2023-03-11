//! A library to create a local clone of a Cargo package.
//!
//! This fetches the repository for a package as defined in the `repository`
//! field of a Cargo package. If there is no repository, it can also fetch the
//! `.crate` file from crates.io.

#![warn(missing_docs)]
use anyhow::{anyhow, bail, Context, Error};
use flate2::read::GzDecoder;
use regex::Regex;
use reqwest::StatusCode;
use semver;
use serde_json::Value;
use std::env;
use std::path::PathBuf;
use std::process::Command;
use tar::Archive;

#[macro_use]
extern crate log;

static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

/// https://api.bitbucket.org/2.0/repositories
pub const DEFAULT_BITBUCKET_URL: &'static str = "https://api.bitbucket.org/2.0/repositories";
/// https://github.com
pub const DEFAULT_GITHUB_URL: &'static str = "https://github.com";
/// https://gitlab.com
pub const DEFAULT_GITLAB_URL: &'static str = "https://gitlab.com";
/// https://crates.io
pub const DEFAULT_REGISTRY_URL: &'static str = "https://crates.io";

/// An enum representation of supported cloning methods.
#[derive(Debug, Clone)]
pub enum CloneMethodKind {
    /// Downloads the `.crate` file from the registry and extracts it.
    Crate,
    /// Clones using `git`.
    Git,
    /// Clones using `hg`.
    Mercurial,
    /// Clones using `pijul`.
    Pijul,
    /// Clones using `fossil`.
    Fossil,
    /// Attempts to automatically detect which method to use using heuristics.
    Auto,
}

impl CloneMethodKind {
    /// Returns the underlying command line command for the method.
    pub fn command(&self) -> &str {
        match *self {
            CloneMethodKind::Crate => "crate",
            CloneMethodKind::Git => "git",
            CloneMethodKind::Mercurial => "hg",
            CloneMethodKind::Pijul => "pijul",
            CloneMethodKind::Fossil => "fossil",
            CloneMethodKind::Auto => "auto",
        }
    }

    /// Creates a `CloneMethodKind` from a method name. If no name matches then None is returned.
    /// Current options are `crate`, `git`, `hg`, `mercurial`, `pijul`, `fossil`, and `auto`
    pub fn from(method_name: &str) -> Option<CloneMethodKind> {
        match method_name {
            "crate" => Some(CloneMethodKind::Crate),
            "git" => Some(CloneMethodKind::Git),
            "hg" => Some(CloneMethodKind::Mercurial),
            "mercurial" => Some(CloneMethodKind::Mercurial),
            "pijul" => Some(CloneMethodKind::Pijul),
            "fossil" => Some(CloneMethodKind::Fossil),
            "auto" => Some(CloneMethodKind::Auto),
            _ => None,
        }
    }
}

/// A struct containg all url and workspace information necessary to clone a crate.
#[derive(Debug, Clone)]
pub struct Cloner {
    /// Defaults to https://crates.io
    registry_url: String,

    /// Defaults to https://github.com
    github_url: String,

    /// Defaults to https://gitlab.com
    gitlab_url: String,

    /// Defaults to https://api.bitbucket.org/2.0/repositories
    bitbutcket_url: String,

    /// Output directory of the Crate source code.
    ///
    /// Uses `std::env::current_dir()` if `None`.
    out_dir: Option<PathBuf>,
}

fn check_semver_req(version: &str) -> Result<String, Error> {
    let first = version
        .chars()
        .nth(0)
        .ok_or_else(|| anyhow!("version is empty"))?;

    let is_req = "<>=^~".contains(first) || version.contains('*');
    if is_req {
        Ok(version.parse::<semver::VersionReq>()?.to_string())
    } else {
        match semver::Version::parse(version) {
            Ok(v) => Ok(format!("={}", v)),
            Err(e) => Err(e).context(anyhow!(
                "`{}` is not a valid semver version.\n\
                 Use an exact version like 1.2.3 or a version requirement expression.",
                version
            ))?,
        }
    }
}

/// Determine the repo path from the package info.
fn get_repo(pkg_info: &Value) -> Result<Option<String>, Error> {
    let krate = pkg_info
        .get("crate")
        .ok_or_else(|| anyhow!("`crate` expected in pkg info"))?;
    let repo = &krate["repository"];
    if repo.is_string() {
        return Ok(Some(repo.as_str().unwrap().to_string()));
    }
    let home = &krate["homepage"];
    if home.is_string() {
        return Ok(Some(home.as_str().unwrap().to_string()));
    }
    Ok(None)
}

/// A wrapper around `reqwest::blocking::get` that provides a User Agent. This
/// is required by crates.io
fn reqwest_get(url: &str) -> reqwest::Result<reqwest::blocking::Response> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(APP_USER_AGENT)
        .build()?;

    client.get(url).send()
}

impl Cloner {
    /// Create a Crate Cloner using all the default settings
    pub fn new() -> Cloner {
        Cloner {
            registry_url: DEFAULT_REGISTRY_URL.to_string(),
            github_url: DEFAULT_GITHUB_URL.to_string(),
            gitlab_url: DEFAULT_GITLAB_URL.to_string(),
            bitbutcket_url: DEFAULT_BITBUCKET_URL.to_string(),
            out_dir: None,
        }
    }

    /// Sets the URL to use for downloading `.crate` files from crates.io.
    pub fn set_registry_url(&mut self, value: impl Into<String>) -> &mut Self {
        self.registry_url = value.into();
        self
    }

    /// Sets the URL to use for downloading GitHub repositories.
    pub fn set_github_url(&mut self, value: impl Into<String>) -> &mut Self {
        self.github_url = value.into();
        self
    }

    /// Sets the URL to use for downloading GitLab repositories.
    pub fn set_gitlab_url(&mut self, value: impl Into<String>) -> &mut Self {
        self.gitlab_url = value.into();
        self
    }

    /// Sets the URL to use for downloading Bitbucket repositories.
    pub fn set_bitbucket_url(&mut self, value: impl Into<String>) -> &mut Self {
        self.bitbutcket_url = value.into();
        self
    }

    /// Sets the directory where the clone will be done.
    ///
    /// The package will appear as a directory underneath the given path.
    pub fn set_out_dir(&mut self, value: impl Into<PathBuf>) -> &mut Self {
        self.out_dir = Some(value.into());
        self
    }

    /// Returns the output directory.
    fn out_dir(&self) -> Result<PathBuf, Error> {
        Ok(self
            .out_dir
            .as_ref()
            .map_or_else(|| env::current_dir(), |v| Ok(v.to_path_buf()))?)
    }

    /// Clones a crate using the provided method.
    ///
    /// - `method_kind` - Method to fetch crate.
    /// - `spec` - The name of the crate to clone
    /// - `version` - The semantic version (semver) of the spec crate to clone
    /// - `extra` - Additional arguments passed to clone command.
    ///
    pub fn clone(
        &self,
        method_kind: CloneMethodKind,
        spec: &str,
        version: Option<&str>,
        extra: &[&str],
    ) -> Result<(), Error> {
        let mut parts = spec.splitn(2, &[':', '@']);
        let name = parts.next().unwrap();
        let spec_version_req = parts.next();
        if spec_version_req.is_some() && version.is_some() {
            bail!("Cannot specify both a :version and --version.");
        }
        let version_req = version
            .or(spec_version_req)
            .map(check_semver_req)
            .transpose()?;
        let pkg_info = self.get_pkg_info(name)?;
        let repo = get_repo(&pkg_info)?;
        let (method, repo) = match method_kind {
            CloneMethodKind::Auto => {
                if version_req.is_some() {
                    (CloneMethodKind::Crate, "".to_string())
                } else if let Some(repo) = repo {
                    self.detect_repo(&repo)?
                } else {
                    (CloneMethodKind::Crate, "".to_string())
                }
            }
            CloneMethodKind::Crate => (method_kind, "".to_string()),
            _ => {
                if repo.is_none() {
                    bail!("Could not find repository path in crates.io.");
                }
                (method_kind, repo.unwrap())
            }
        };
        match method {
            CloneMethodKind::Crate => {
                if !extra.is_empty() {
                    bail!("Got extra arguments, crate downloads take no extra arguments.");
                }
                self.clone_crate(name, version_req, &pkg_info)?;
            }
            CloneMethodKind::Git
            | CloneMethodKind::Mercurial
            | CloneMethodKind::Pijul
            | CloneMethodKind::Fossil => {
                if let Some(version_req) = version_req {
                    bail!(
                        "Specifying a version `{}` only works with the `crate` method.",
                        version_req
                    );
                }
                self.run_clone(method.command(), &repo, extra)?;
            }
            CloneMethodKind::Auto => unreachable!(),
        }

        Ok(())
    }

    fn detect_repo(&self, repo: &str) -> Result<(CloneMethodKind, String), Error> {
        if repo.ends_with(".git") {
            return Ok((CloneMethodKind::Git, repo.to_string()));
        }
        if let Some(c) = Regex::new(r"https?://(?:www\.)?github\.com/([^/]+)/([^/]+)")
            .unwrap()
            .captures(repo)
        {
            return Ok((
                CloneMethodKind::Git,
                format!(
                    "{}/{}/{}.git",
                    self.github_url,
                    c.get(1).unwrap().as_str(),
                    c.get(2).unwrap().as_str()
                ),
            ));
        }
        if let Some(c) = Regex::new(r"https?://(?:www\.)?gitlab\.com/([^/]+)/([^/]+)")
            .unwrap()
            .captures(repo)
        {
            return Ok((
                CloneMethodKind::Git,
                format!(
                    "{}/{}/{}.git",
                    self.gitlab_url,
                    c.get(1).unwrap().as_str(),
                    c.get(2).unwrap().as_str()
                ),
            ));
        }
        if let Some(c) = Regex::new(r"https?://(?:www\.)?bitbucket\.(?:org|com)/([^/]+)/([^/]+)")
            .unwrap()
            .captures(repo)
        {
            let user = c.get(1).unwrap().as_str();
            let name = c.get(2).unwrap().as_str();
            return self.bitbucket(user, name);
        }
        if repo.starts_with("https://nest.pijul.com/") {
            return Ok((CloneMethodKind::Pijul, repo.to_string()));
        }
        bail!(
            "Could not determine the VCS from repo `{}`, \
             use the `--method` option to specify how to download.",
            repo
        );
    }

    fn bitbucket(&self, user: &str, name: &str) -> Result<(CloneMethodKind, String), Error> {
        // Determine if it is git or hg.
        let api_url = &format!("{}/{}/{}", self.bitbutcket_url, user, name);
        let repo_info =
            reqwest_get(api_url).context("Failed to fetch repo info from bitbucket.")?;
        let code = repo_info.status();
        if !code.is_success() {
            bail!(
                "Failed to get repo info from bitbucket API `{}`: `{}`",
                api_url,
                code
            );
        }
        let repo_info: Value = repo_info
            .json()
            .context("Failed to convert to bitbucket json.")?;
        let method = repo_info["scm"]
            .as_str()
            .expect("Could not get `scm` from bitbucket.");
        let method = match method {
            "git" => CloneMethodKind::Git,
            "hg" => CloneMethodKind::Mercurial,
            _ => bail!("Unexpected bitbucket scm: `{}`", method),
        };
        let clones = repo_info["links"]["clone"]
            .as_array()
            .expect("Could not get `clone` from bitbucket.");
        let href = clones
            .iter()
            .find(|c| {
                c["name"]
                    .as_str()
                    .expect("Could not get clone `name` from bitbucket.")
                    == "https"
            })
            .expect("Could not find `https` clone in bitbucket.")["href"]
            .as_str()
            .expect("Could not get clone `href` from bitbucket.");
        Ok((method, href.to_string()))
    }

    /// Grab package info from crates.io.
    fn get_pkg_info(&self, name: &str) -> Result<Value, Error> {
        let url = format!("{}/api/v1/crates/{}", self.registry_url, name);
        debug!("GET {url}");
        let pkg_info = reqwest_get(&url).context("Failed to fetch package info from crates.io.")?;
        let code = pkg_info.status();
        match code {
            StatusCode::OK => {}
            StatusCode::NOT_FOUND => bail!("Package `{}` not found on crates.io.", name),
            _ => bail!("Failed to get package info from crates.io: `{}`", code),
        }
        let pkg_info: Value = pkg_info.json().context("Failed to convert to json.")?;
        Ok(pkg_info)
    }

    /// Download a crate from crates.io.
    fn clone_crate(
        &self,
        name: &str,
        version_req: Option<String>,
        pkg_info: &Value,
    ) -> Result<(), Error> {
        // Determine which version to download.
        let versions = pkg_info["versions"]
            .as_array()
            .expect("Could not find `versions` array on crates.io.");
        let versions = versions.iter().map(|crate_version| {
            let num = crate_version["num"]
                .as_str()
                .expect("Could not get `num` from version.");
            let v = semver::Version::parse(num).expect("Could not parse crate `num`.");
            (crate_version, v)
        });
        let mut versions: Vec<_> = if let Some(version_req) = version_req {
            let req = semver::VersionReq::parse(&version_req)?;
            versions
                .filter(|(_crate_version, ver)| req.matches(ver))
                .collect()
        } else {
            versions.collect()
        };
        // Find the largest version.
        if versions.is_empty() {
            bail!("Could not find any matching versions.");
        }
        versions.sort_unstable_by_key(|x| x.1.clone());
        let last = versions.last().unwrap().0;
        let dl_path = last["dl_path"]
            .as_str()
            .expect("Could not find `dl_path` in crate version info.");
        let dl_path = format!("{}{}", self.registry_url, dl_path);
        let version = last["num"]
            .as_str()
            .expect("Could not find `num` in crate version info.");
        info!("Downloading `{}`", dl_path);
        let mut response =
            reqwest_get(&dl_path).context(format!("Failed to download `{}`", dl_path))?;
        // TODO: This could be much better.
        let mut body = Vec::new();
        response.copy_to(&mut body)?;
        let gz = GzDecoder::new(body.as_slice());
        let mut tar = Archive::new(gz);
        let base = format!("{}-{}", name.to_lowercase(), version);

        for entry in tar.entries()? {
            let mut entry = entry.context("Failed to get tar entry.")?;
            let entry_path = entry
                .path()
                .context("Failed to read entry path.")?
                .into_owned();
            info!("{}", entry_path.display());

            // Sanity check.
            if !entry_path.starts_with(&base) {
                bail!(
                    "Expected path `{}` in tarball, got `{}`.",
                    base,
                    entry_path.display()
                );
            }

            entry.unpack_in(&self.out_dir()?).context(format!(
                "failed to unpack entry at `{}`",
                entry_path.display()
            ))?;
        }
        Ok(())
    }

    /// Runs the clone process.
    fn run_clone(&self, method: &str, repo: &str, extra: &[&str]) -> Result<(), Error> {
        info!("Running: {} clone {} {}", method, repo, extra.join(" "));
        let status = Command::new(method)
            .arg("clone")
            .arg(repo)
            .args(extra)
            .current_dir(&self.out_dir()?)
            .status()
            .context(format!("Failed to run `{}`.", method))?;
        if !status.success() {
            bail!("`{} clone` did not finish successfully.", method);
        }
        Ok(())
    }
}

/// A helper function for cloning a crate into the current working directory.
///
/// - `method_name` - Method to fetch crate. Options are "crate", "git", "hg", "pijul", "fossil", "auto"
/// - `spec` - The name of the crate to clone
/// - `version` - The semantic version (semver) of the spec crate to clone
/// - `extra` - Additional arguments passed to clone command.
///
pub fn clone(
    method_name: &str,
    spec: &str,
    version: Option<&str>,
    extra: &[&str],
) -> Result<(), Error> {
    Cloner::new().clone(
        CloneMethodKind::from(method_name).unwrap(),
        spec,
        version,
        extra,
    )
}
