#[macro_use]
extern crate failure;
extern crate flate2;
extern crate regex;
extern crate reqwest;
extern crate semver;
extern crate serde_json;
extern crate tar;

use failure::{Error, ResultExt};
use flate2::read::GzDecoder;
use regex::Regex;
use reqwest::StatusCode;
use serde_json::Value;
use std::env;
use std::process::Command;
use tar::Archive;

pub fn clone(method_name: &str, spec: &str, extra: &[&str]) -> Result<(), Error> {
    let mut parts = spec.splitn(2, ':');
    let name = parts.next().unwrap();
    let pkg_info = get_pkg_info(name)?;
    let repo = get_repo(&pkg_info)?;
    let (method, repo) = match method_name {
        "auto" => {
            if let Some(repo) = repo {
                detect_repo(&repo)?
            } else {
                ("crate", "".to_string())
            }
        }
        "crate" => ("crate", "".to_string()),
        _ => {
            if repo.is_none() {
                bail!("Could not find repository path in crates.io.");
            }
            (method_name, repo.unwrap())
        }
    };
    match method {
        "crate" => clone_crate(spec, &pkg_info, extra)?,
        "git" | "hg" | "pijul" | "fossil" => run_clone(method, &repo, extra)?,
        _ => bail!("Unsupported method `{}`", method),
    }

    Ok(())
}

fn detect_repo(repo: &str) -> Result<(&'static str, String), Error> {
    if repo.ends_with(".git") {
        return Ok(("git", repo.to_string()));
    }
    if let Some(c) = Regex::new(r"https?://(?:www\.)?github\.com/([^/]+)/([^/]+)")
        .unwrap()
        .captures(repo)
    {
        return Ok((
            "git",
            format!(
                "https://github.com/{}/{}.git",
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
            "git",
            format!(
                "https://gitlab.com/{}/{}.git",
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
        return bitbucket(user, name);
    }
    if repo.starts_with("https://nest.pijul.com/") {
        return Ok(("pijul", repo.to_string()));
    }
    bail!(
        "Could not determine the VCS from repo `{}`, \
         use the `--method` option to specify how to download.",
        repo
    );
}

fn bitbucket(user: &str, name: &str) -> Result<(&'static str, String), Error> {
    // Determine if it is git or hg.
    let api_url = &format!(
        "https://api.bitbucket.org/2.0/repositories/{}/{}",
        user, name
    );
    let mut repo_info = reqwest::get(api_url).context("Failed to fetch repo info from bitbucket.")?;
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
        "git" => "git",
        "hg" => "hg",
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
                .expect("Could not get clone `name` from bitbucket.") == "https"
        })
        .expect("Could not find `https` clone in bitbucket.")["href"]
        .as_str()
        .expect("Could not get clone `href` from bitbucket.");
    Ok((method, href.to_string()))
}

/// Grab package info from crates.io.
fn get_pkg_info(name: &str) -> Result<Value, Error> {
    let mut pkg_info = reqwest::get(&format!("https://crates.io/api/v1/crates/{}", name))
        .context("Failed to fetch package info from crates.io.")?;
    let code = pkg_info.status();
    match code {
        StatusCode::OK => {}
        StatusCode::NOT_FOUND => bail!("Package `{}` not found on crates.io.", name),
        _ => bail!("Failed to get package info from crates.io: `{}`", code),
    }
    let pkg_info: Value = pkg_info.json().context("Failed to convert to json.")?;
    Ok(pkg_info)
}

/// Determine the repo path from the package info.
fn get_repo(pkg_info: &Value) -> Result<Option<String>, Error> {
    let krate = pkg_info
        .get("crate")
        .ok_or_else(|| format_err!("`crate` expected in pkg info"))?;
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

/// Download a crate from crates.io.
fn clone_crate(spec: &str, pkg_info: &Value, extra: &[&str]) -> Result<(), Error> {
    let mut parts = spec.splitn(2, ':');
    let name = parts.next().unwrap();
    let version = parts.next();

    if !extra.is_empty() {
        bail!("Got extra arguments, crate downloads take no extra arguments.");
    }

    let dst = env::current_dir()?;

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
    let mut versions: Vec<_> = if let Some(version) = version {
        let req = semver::VersionReq::parse(version)?;
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
    let dl_path = format!("https://crates.io{}", dl_path);
    let version = last["num"]
        .as_str()
        .expect("Could not find `num` in crate version info.");
    println!("Downloading `{}`", dl_path);
    let mut response = reqwest::get(&dl_path).context(format!("Failed to download `{}`", dl_path))?;
    // TODO: This could be much better.
    let mut body = Vec::new();
    response.copy_to(&mut body)?;
    let gz = GzDecoder::new(body.as_slice());
    let mut tar = Archive::new(gz);
    let base = format!("{}-{}", name, version);

    for entry in tar.entries()? {
        let mut entry = entry.context("Failed to get tar entry.")?;
        let entry_path = entry
            .path()
            .context("Failed to read entry path.")?
            .into_owned();
        println!("{}", entry_path.display());

        // Sanity check.
        if !entry_path.starts_with(&base) {
            bail!(
                "Expected path `{}` in tarball, got `{}`.",
                base,
                entry_path.display()
            );
        }

        entry.unpack_in(&dst).context(format!(
            "failed to unpack entry at `{}`",
            entry_path.display()
        ))?;
    }
    Ok(())
}

/// Runs the clone process.
fn run_clone(method: &str, repo: &str, extra: &[&str]) -> Result<(), Error> {
    println!("Running: {} clone {} {}", method, repo, extra.join(" "));
    let status = Command::new(method)
        .arg("clone")
        .arg(repo)
        .args(extra)
        .status()
        .context(format!("Failed to run `{}`.", method))?;
    if !status.success() {
        bail!("`{} clone` did not finish successfully.", method);
    }
    Ok(())
}
