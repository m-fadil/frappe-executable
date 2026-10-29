use crate::env::Env;
use crate::errors::{CliError, Result};
use crate::prompt::{Choice, ask_select};
use std::fs;
use std::path::Path;

/// Site-facing commands act on the bench in the CURRENT directory, not on
/// wherever the CLI is installed. Fail loudly when cwd is not a bench.
pub fn require_bench_dir(sites_dir: &str) -> Result<()> {
    if !fs::metadata(sites_dir).map(|m| m.is_dir()).unwrap_or(false) {
        let cwd = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        return Err(CliError::new(format!(
            "not a bench directory: {cwd} (no '{sites_dir}/'). cd into your bench first."
        )));
    }
    Ok(())
}

pub fn site_exists(sites_dir: &str, site: &str) -> bool {
    Path::new(sites_dir).join(site).join("site_config.json").exists()
}

/// Every entry with a site_config.json; symlinked site dirs count, as with a shell glob.
pub fn list_sites(sites_dir: &str) -> Vec<String> {
    let Ok(rd) = fs::read_dir(sites_dir) else {
        return Vec::new();
    };
    let mut sites: Vec<String> = rd
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| site_exists(sites_dir, n))
        .collect();
    sites.sort();
    sites
}

/// Fill `name` with a site: keeps a set value, auto-picks a lone site, else asks.
pub fn pick_site(env: &mut Env, name: &str, sites_dir: &str) -> Result<String> {
    if env.filled(name) {
        return Ok(env.get(name).unwrap_or_default().to_string());
    }
    let sites = list_sites(sites_dir);
    match sites.len() {
        0 => Err(CliError::new(format!("no site found in {sites_dir}/"))),
        1 => {
            env.set(name, &sites[0]);
            println!("Site: {}", sites[0]);
            Ok(sites[0].clone())
        }
        n => {
            let choices: Vec<Choice> = sites.iter().map(Choice::new).collect();
            ask_select(env, name, "Pick site", &choices, &format!("{n} sites here, "))
        }
    }
}
