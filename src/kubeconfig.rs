use crate::fleet::Grouping;
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Only the part of a kubeconfig k9mux cares about. k9s reads the files itself;
/// k9mux just needs to know which contexts exist and where they came from.
#[derive(Deserialize)]
struct RawKubeconfig {
    #[serde(default)]
    contexts: Vec<RawContext>,
    #[serde(default)]
    clusters: Vec<RawCluster>,
}

#[derive(Deserialize)]
struct RawContext {
    name: String,
    #[serde(default)]
    context: Option<RawContextBody>,
}

#[derive(Deserialize)]
struct RawContextBody {
    #[serde(default)]
    cluster: Option<String>,
}

#[derive(Deserialize)]
struct RawCluster {
    name: String,
    #[serde(default)]
    cluster: Option<RawClusterBody>,
}

#[derive(Deserialize)]
struct RawClusterBody {
    #[serde(default)]
    server: Option<String>,
}

/// The contexts k9mux found, a cluster per source file, and the files
/// themselves so they can be handed to k9s as KUBECONFIG.
#[derive(Debug)]
pub struct Loaded {
    pub contexts: Vec<String>,
    pub grouping: Grouping,
    pub paths: Vec<PathBuf>,
    /// API server URL per context, where the kubeconfig gives one.
    pub servers: Vec<(String, String)>,
}

impl Loaded {
    /// Clusters the prober should watch.
    pub fn probe_targets(&self) -> Vec<crate::probe::Target> {
        self.servers
            .iter()
            .filter_map(|(ctx, url)| {
                crate::probe::endpoint(url).map(|(host, port)| crate::probe::Target {
                    instance: crate::fleet::display_name(ctx),
                    host,
                    port,
                })
            })
            .collect()
    }

    /// The value to set as KUBECONFIG for a child k9s.
    pub fn kubeconfig_env(&self) -> String {
        self.paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(":")
    }
}

/// Which kubeconfig files to read, in precedence order:
/// `--kubeconfig`, then `$KUBECONFIG`, then k9mux' config, then the default path.
pub fn resolve_paths(
    flag: Option<&str>,
    env: Option<&str>,
    configured: &[String],
    home: Option<&Path>,
) -> Vec<PathBuf> {
    let expand = |s: &str| -> Option<PathBuf> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        Some(match (s.strip_prefix("~/"), home) {
            (Some(rest), Some(home)) => home.join(rest),
            _ => PathBuf::from(s),
        })
    };
    let split = |s: &str| -> Vec<PathBuf> { s.split(':').filter_map(expand).collect() };

    let mut paths = match (flag, env) {
        (Some(f), _) if !split(f).is_empty() => split(f),
        (_, Some(e)) if !split(e).is_empty() => split(e),
        _ => configured.iter().filter_map(|s| expand(s)).collect(),
    };
    if paths.is_empty()
        && let Some(home) = home
    {
        paths.push(home.join(".kube").join("config"));
    }
    paths.dedup();
    let mut seen = Vec::new();
    paths.retain(|p| {
        let fresh = !seen.contains(p);
        seen.push(p.clone());
        fresh
    });
    paths
}

/// Cluster name for a kubeconfig file, e.g. `~/.kube/config-prod` -> `prod`.
pub fn group_name(path: &Path) -> String {
    let stem = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let stem = stem
        .strip_suffix(".yaml")
        .or_else(|| stem.strip_suffix(".yml"))
        .unwrap_or(&stem)
        .to_string();
    let trimmed = stem
        .strip_prefix("config-")
        .or_else(|| stem.strip_prefix("config."))
        .unwrap_or(&stem);
    if trimmed.is_empty() || trimmed == "config" {
        "default".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn load(paths: &[PathBuf]) -> Result<Loaded, String> {
    let mut contexts: Vec<String> = Vec::new();
    let mut clusters: Vec<(String, Vec<String>)> = Vec::new();
    let mut servers: Vec<(String, String)> = Vec::new();

    for path in paths {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let one: RawKubeconfig =
            serde_saphyr::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        // Only contexts this file introduces; a context defined twice belongs
        // to the first file that had it, matching kubectl's merge rules.
        let fresh: Vec<String> = one
            .contexts
            .iter()
            .map(|c| c.name.clone())
            .filter(|n| !contexts.contains(n))
            .collect();
        // Where each new context's API server lives, via its cluster entry.
        for c in &one.contexts {
            if !fresh.contains(&c.name) {
                continue;
            }
            let Some(cluster) = c.context.as_ref().and_then(|b| b.cluster.as_ref()) else {
                continue;
            };
            if let Some(url) = one
                .clusters
                .iter()
                .find(|k| &k.name == cluster)
                .and_then(|k| k.cluster.as_ref())
                .and_then(|b| b.server.clone())
            {
                servers.push((c.name.clone(), url));
            }
        }

        if !fresh.is_empty() {
            contexts.extend(fresh.clone());
            clusters.push((group_name(path), fresh));
        }
    }

    if contexts.is_empty() {
        return Err(format!(
            "no contexts in {}",
            paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok(Loaded {
        contexts,
        grouping: Grouping { clusters },
        paths: paths.to_vec(),
        servers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }
    fn home() -> PathBuf {
        p("/home/me")
    }

    fn kubeconfig_yaml(contexts: &[&str]) -> String {
        let mut s = String::from("apiVersion: v1\nkind: Config\nusers: []\nclusters:\n");
        for (i, c) in contexts.iter().enumerate() {
            s.push_str(&format!(
                "- name: {c}-cluster\n  cluster:\n    server: https://{c}.example.com:{}\n",
                6443 + i
            ));
        }
        s.push_str("contexts:\n");
        for c in contexts {
            s.push_str(&format!(
                "- name: {c}\n  context:\n    cluster: {c}-cluster\n    user: {c}-user\n"
            ));
        }
        s
    }

    fn write(dir: &Path, name: &str, contexts: &[&str]) -> PathBuf {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(kubeconfig_yaml(contexts).as_bytes()).unwrap();
        path
    }

    #[test]
    fn the_flag_wins_over_everything() {
        let got = resolve_paths(
            Some("/flag/cfg"),
            Some("/env/cfg"),
            &["/configured/cfg".into()],
            Some(&home()),
        );
        assert_eq!(got, vec![p("/flag/cfg")]);
    }

    #[test]
    fn the_env_var_wins_over_the_config_file() {
        let got = resolve_paths(
            None,
            Some("/env/cfg"),
            &["/configured/cfg".into()],
            Some(&home()),
        );
        assert_eq!(got, vec![p("/env/cfg")]);
    }

    #[test]
    fn the_config_file_is_used_when_nothing_else_is_set() {
        let got = resolve_paths(None, None, &["/configured/cfg".into()], Some(&home()));
        assert_eq!(got, vec![p("/configured/cfg")]);
    }

    #[test]
    fn the_default_path_is_the_last_resort() {
        assert_eq!(
            resolve_paths(None, None, &[], Some(&home())),
            vec![p("/home/me/.kube/config")]
        );
    }

    #[test]
    fn a_list_is_split_on_the_path_separator() {
        assert_eq!(
            resolve_paths(Some("/a:/b:/c"), None, &[], Some(&home())),
            vec![p("/a"), p("/b"), p("/c")]
        );
        assert_eq!(
            resolve_paths(None, Some("/a:/b"), &[], Some(&home())),
            vec![p("/a"), p("/b")]
        );
    }

    #[test]
    fn empty_segments_are_ignored() {
        assert_eq!(
            resolve_paths(Some("/a::/b:"), None, &[], Some(&home())),
            vec![p("/a"), p("/b")]
        );
    }

    #[test]
    fn an_empty_value_falls_through_to_the_next_source() {
        assert_eq!(
            resolve_paths(Some(""), Some("/env/cfg"), &[], Some(&home())),
            vec![p("/env/cfg")]
        );
    }

    #[test]
    fn a_leading_tilde_expands_to_home() {
        assert_eq!(
            resolve_paths(None, None, &["~/.kube/config-prod".into()], Some(&home())),
            vec![p("/home/me/.kube/config-prod")]
        );
    }

    #[test]
    fn the_same_file_listed_twice_is_read_once() {
        assert_eq!(
            resolve_paths(Some("/a:/b:/a"), None, &[], Some(&home())),
            vec![p("/a"), p("/b")]
        );
    }

    #[test]
    fn several_configured_files_are_all_used() {
        let got = resolve_paths(
            None,
            None,
            &["~/.kube/config-prod".into(), "~/.kube/config-remote".into()],
            Some(&home()),
        );
        assert_eq!(
            got,
            vec![
                p("/home/me/.kube/config-prod"),
                p("/home/me/.kube/config-remote")
            ]
        );
    }

    #[test]
    fn a_cluster_is_named_after_its_kubeconfig_file() {
        assert_eq!(group_name(&p("/home/me/.kube/config-prod")), "prod");
        assert_eq!(group_name(&p("/home/me/.kube/config.staging")), "staging");
        assert_eq!(group_name(&p("/tmp/prod.yaml")), "prod");
        assert_eq!(group_name(&p("/tmp/staging")), "staging");
    }

    #[test]
    fn the_default_kubeconfig_gets_a_sensible_cluster_name() {
        assert_eq!(group_name(&p("/home/me/.kube/config")), "default");
    }

    #[test]
    fn contexts_are_read_from_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let f = write(dir.path(), "config-prod", &["k3s", "microk8s"]);
        let loaded = load(&[f]).unwrap();
        assert_eq!(loaded.contexts, vec!["k3s", "microk8s"]);
    }

    #[test]
    fn contexts_from_several_files_are_merged_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(dir.path(), "config-a", &["one", "two"]);
        let b = write(dir.path(), "config-b", &["three"]);
        let loaded = load(&[a, b]).unwrap();
        assert_eq!(loaded.contexts, vec!["one", "two", "three"]);
    }

    #[test]
    fn a_context_in_two_files_is_listed_once() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(dir.path(), "config-a", &["shared", "only-a"]);
        let b = write(dir.path(), "config-b", &["shared", "only-b"]);
        let loaded = load(&[a, b]).unwrap();
        assert_eq!(loaded.contexts, vec!["shared", "only-a", "only-b"]);
    }

    #[test]
    fn each_file_becomes_a_cluster() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(dir.path(), "config-prod", &["one", "two"]);
        let b = write(dir.path(), "config-remote", &["three"]);
        let loaded = load(&[a, b]).unwrap();
        assert_eq!(loaded.grouping.clusters.len(), 2);
        assert_eq!(loaded.grouping.clusters[0].0, "prod");
        assert_eq!(loaded.grouping.clusters[0].1, vec!["one", "two"]);
        assert_eq!(loaded.grouping.clusters[1].0, "remote");
        assert_eq!(loaded.grouping.clusters[1].1, vec!["three"]);
    }

    #[test]
    fn a_shared_context_belongs_to_the_first_file_that_had_it() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(dir.path(), "config-a", &["shared"]);
        let b = write(dir.path(), "config-b", &["shared", "only-b"]);
        let loaded = load(&[a, b]).unwrap();
        assert_eq!(loaded.grouping.clusters[0].1, vec!["shared"]);
        assert_eq!(loaded.grouping.clusters[1].1, vec!["only-b"]);
    }

    #[test]
    fn the_kubeconfig_env_lists_every_file_for_the_child() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(dir.path(), "config-a", &["one"]);
        let b = write(dir.path(), "config-b", &["two"]);
        let loaded = load(&[a.clone(), b.clone()]).unwrap();
        assert_eq!(
            loaded.kubeconfig_env(),
            format!("{}:{}", a.display(), b.display())
        );
    }

    #[test]
    fn the_api_server_of_each_context_is_picked_up() {
        let dir = tempfile::tempdir().unwrap();
        let f = write(dir.path(), "config-prod", &["one", "two"]);
        let loaded = load(&[f]).unwrap();
        assert_eq!(
            loaded.servers,
            vec![
                (
                    "one".to_string(),
                    "https://one.example.com:6443".to_string()
                ),
                (
                    "two".to_string(),
                    "https://two.example.com:6444".to_string()
                ),
            ]
        );
    }

    #[test]
    fn probe_targets_use_the_display_name_and_the_parsed_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        let f = write(dir.path(), "config-prod", &["one"]);
        let targets = load(&[f]).unwrap().probe_targets();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].instance, "one");
        assert_eq!(targets[0].host, "one.example.com");
        assert_eq!(targets[0].port, 6443);
    }

    #[test]
    fn a_context_without_a_server_is_simply_not_probed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config-bare");
        std::fs::write(
            &path,
            "apiVersion: v1\nkind: Config\ncontexts:\n- name: bare\n",
        )
        .unwrap();
        let loaded = load(&[path]).unwrap();
        assert_eq!(loaded.contexts, vec!["bare"]);
        assert!(loaded.servers.is_empty());
        assert!(loaded.probe_targets().is_empty());
    }

    #[test]
    fn a_missing_file_is_reported_by_name() {
        let err = load(&[p("/nope/config-ghost")]).unwrap_err();
        assert!(err.contains("config-ghost"), "{err}");
    }

    #[test]
    fn a_file_that_is_not_a_kubeconfig_is_reported_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config-broken");
        std::fs::write(&path, "just: [some, text").unwrap();
        let err = load(&[path]).unwrap_err();
        assert!(err.contains("config-broken"), "{err}");
    }

    #[test]
    fn a_kubeconfig_without_contexts_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let f = write(dir.path(), "config-empty", &[]);
        assert!(load(&[f]).is_err());
    }

    #[test]
    fn an_empty_file_is_skipped_when_another_one_works() {
        let dir = tempfile::tempdir().unwrap();
        let empty = write(dir.path(), "config-empty", &[]);
        let good = write(dir.path(), "config-good", &["one"]);
        let loaded = load(&[empty, good]).unwrap();
        assert_eq!(loaded.contexts, vec!["one"]);
        assert_eq!(
            loaded.grouping.clusters.len(),
            1,
            "empty file adds no cluster"
        );
    }
}
