#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Health {
    Unknown,
    Ok,
    Degraded,
    Unreachable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instance {
    /// Short display name, unique across the fleet.
    pub name: String,
    /// kubeconfig context this instance is served by.
    pub context: String,
    /// Cluster (group) this instance belongs to.
    pub group: String,
    pub health: Health,
    pub version: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub instances: Vec<Instance>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fleet {
    pub groups: Vec<Group>,
}

/// Explicit cluster -> contexts mapping, usually read from k9mux' own config.
#[derive(Clone, Debug, Default)]
pub struct Grouping {
    pub clusters: Vec<(String, Vec<String>)>,
}

pub const UNGROUPED: &str = "contexts";

/// Readable label for a kubeconfig context. Real-world context names carry a
/// lot of addressing noise - EKS ARNs, GKE project paths, DNS suffixes - none
/// of which helps you tell two clusters apart in a 26 column sidebar.
pub fn display_name(context: &str) -> String {
    if let Some((_, name)) = context.rsplit_once("cluster/") {
        return name.to_string();
    }
    if let Some(rest) = context.strip_prefix("gke_")
        && let Some((_, name)) = rest.rsplit_once('_')
    {
        return name.to_string();
    }
    match context.split_once('.') {
        Some((head, _)) if !head.is_empty() => head.to_string(),
        _ => context.to_string(),
    }
}

impl Instance {
    fn new(context: &str, group: &str, name: String) -> Instance {
        Instance {
            name,
            context: context.to_string(),
            group: group.to_string(),
            health: Health::Unknown,
            version: None,
        }
    }
}

/// Short label per context, falling back to the full name wherever shortening
/// would make two contexts indistinguishable.
fn display_names(contexts: &[String]) -> Vec<String> {
    let short: Vec<String> = contexts.iter().map(|c| display_name(c)).collect();
    short
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let ambiguous = short.iter().enumerate().any(|(j, o)| j != i && o == s);
            if ambiguous {
                contexts[i].clone()
            } else {
                s.clone()
            }
        })
        .collect()
}

impl Fleet {
    pub fn from_contexts(contexts: &[String], grouping: &Grouping) -> Fleet {
        let mut groups: Vec<Group> = Vec::new();
        let mut claimed: Vec<&str> = Vec::new();
        let names = display_names(contexts);
        let label = |ctx: &str| {
            contexts
                .iter()
                .position(|c| c == ctx)
                .map(|i| names[i].clone())
                .unwrap_or_else(|| ctx.to_string())
        };

        for (name, members) in &grouping.clusters {
            let instances: Vec<Instance> = members
                .iter()
                .filter(|m| contexts.iter().any(|c| c == *m))
                .map(|m| {
                    claimed.push(m.as_str());
                    Instance::new(m, name, label(m))
                })
                .collect();
            if !instances.is_empty() {
                groups.push(Group {
                    name: name.clone(),
                    instances,
                });
            }
        }

        let leftovers: Vec<Instance> = contexts
            .iter()
            .filter(|c| !claimed.contains(&c.as_str()))
            .map(|c| Instance::new(c, UNGROUPED, label(c)))
            .collect();
        if !leftovers.is_empty() {
            groups.push(Group {
                name: UNGROUPED.to_string(),
                instances: leftovers,
            });
        }

        Fleet { groups }
    }
    pub fn instances(&self) -> Vec<&Instance> {
        self.groups
            .iter()
            .flat_map(|g| g.instances.iter())
            .collect()
    }
    pub fn find(&self, name: &str) -> Option<&Instance> {
        self.instances().into_iter().find(|i| i.name == name)
    }
    pub fn position(&self, name: &str) -> Option<usize> {
        self.instances().iter().position(|i| i.name == name)
    }
    /// Instance `steps` places along the flattened fleet, wrapping around.
    pub fn step(&self, from: &str, steps: isize) -> Option<&Instance> {
        let all = self.instances();
        if all.is_empty() {
            return None;
        }
        let len = all.len() as isize;
        let at = self.position(from)? as isize;
        let next = (at + steps).rem_euclid(len) as usize;
        Some(all[next])
    }
    /// First instance not already attached to a pane, for a fresh split.
    pub fn first_unused(&self, used: &[String]) -> Option<&Instance> {
        self.instances()
            .into_iter()
            .find(|i| !used.contains(&i.name))
    }
    pub fn set_health(&mut self, name: &str, health: Health) {
        for g in &mut self.groups {
            for i in &mut g.instances {
                if i.name == name {
                    i.health = health;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctxs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn grouping(pairs: &[(&str, &[&str])]) -> Grouping {
        Grouping {
            clusters: pairs
                .iter()
                .map(|(n, c)| (n.to_string(), ctxs(c)))
                .collect(),
        }
    }

    fn sample() -> Fleet {
        Fleet::from_contexts(
            &ctxs(&["use1-a", "use1-b", "euc1-a", "stg-blue"]),
            &grouping(&[
                ("prod-us", &["use1-a", "use1-b"]),
                ("prod-eu", &["euc1-a"]),
                ("staging", &["stg-blue"]),
            ]),
        )
    }

    #[test]
    fn contexts_are_grouped_into_clusters() {
        let f = sample();
        assert_eq!(f.groups.len(), 3);
        assert_eq!(f.groups[0].name, "prod-us");
        assert_eq!(f.groups[0].instances.len(), 2);
        assert_eq!(f.groups[1].name, "prod-eu");
        assert_eq!(f.groups[2].instances[0].name, "stg-blue");
    }

    #[test]
    fn group_order_follows_the_config_not_the_kubeconfig() {
        let f = Fleet::from_contexts(
            &ctxs(&["stg-blue", "use1-a"]),
            &grouping(&[("prod-us", &["use1-a"]), ("staging", &["stg-blue"])]),
        );
        assert_eq!(f.groups[0].name, "prod-us");
    }

    #[test]
    fn unmapped_contexts_land_in_a_default_group() {
        let f = Fleet::from_contexts(
            &ctxs(&["use1-a", "some-other-ctx"]),
            &grouping(&[("prod-us", &["use1-a"])]),
        );
        assert_eq!(f.groups.len(), 2);
        assert_eq!(f.groups[1].name, UNGROUPED);
        assert_eq!(f.groups[1].instances[0].name, "some-other-ctx");
    }

    #[test]
    fn configured_contexts_that_do_not_exist_are_skipped() {
        let f = Fleet::from_contexts(
            &ctxs(&["use1-a"]),
            &grouping(&[("prod-us", &["use1-a", "gone"])]),
        );
        assert_eq!(f.instances().len(), 1);
    }

    #[test]
    fn with_no_grouping_everything_is_one_cluster() {
        let f = Fleet::from_contexts(&ctxs(&["a", "b"]), &Grouping::default());
        assert_eq!(f.groups.len(), 1);
        assert_eq!(f.groups[0].name, UNGROUPED);
        assert_eq!(f.groups[0].instances.len(), 2);
    }

    #[test]
    fn instances_flatten_in_group_order() {
        let names: Vec<_> = sample()
            .instances()
            .iter()
            .map(|i| i.name.clone())
            .collect();
        assert_eq!(names, vec!["use1-a", "use1-b", "euc1-a", "stg-blue"]);
    }

    #[test]
    fn instances_know_their_group_and_context() {
        let f = sample();
        let i = f.find("euc1-a").unwrap();
        assert_eq!(i.group, "prod-eu");
        assert_eq!(i.context, "euc1-a");
        assert_eq!(i.health, Health::Unknown);
    }

    #[test]
    fn stepping_moves_through_the_flattened_fleet() {
        let f = sample();
        assert_eq!(f.step("use1-a", 1).unwrap().name, "use1-b");
        assert_eq!(f.step("use1-b", -1).unwrap().name, "use1-a");
    }

    #[test]
    fn stepping_wraps_at_both_ends() {
        let f = sample();
        assert_eq!(f.step("stg-blue", 1).unwrap().name, "use1-a");
        assert_eq!(f.step("use1-a", -1).unwrap().name, "stg-blue");
    }

    #[test]
    fn position_locates_an_instance() {
        assert_eq!(sample().position("euc1-a"), Some(2));
        assert_eq!(sample().position("nope"), None);
    }

    #[test]
    fn a_split_picks_the_first_instance_not_already_open() {
        let f = sample();
        let used = vec!["use1-a".to_string(), "use1-b".to_string()];
        assert_eq!(f.first_unused(&used).unwrap().name, "euc1-a");
    }

    #[test]
    fn first_unused_is_none_when_the_whole_fleet_is_open() {
        let f = sample();
        let used: Vec<String> = f.instances().iter().map(|i| i.name.clone()).collect();
        assert!(f.first_unused(&used).is_none());
    }

    #[test]
    fn eks_arns_shorten_to_the_cluster_name() {
        assert_eq!(
            display_name("arn:aws:eks:us-east-1:410000000000:cluster/use1-a"),
            "use1-a"
        );
    }

    #[test]
    fn gke_paths_shorten_to_the_cluster_name() {
        assert_eq!(display_name("gke_my-project_us-central1_apps"), "apps");
    }

    #[test]
    fn dns_suffixes_are_dropped() {
        assert_eq!(display_name("apps-eks.example.com"), "apps-eks");
        assert_eq!(display_name("edge-k8s.example.com"), "edge-k8s");
    }

    #[test]
    fn plain_names_are_left_alone() {
        assert_eq!(display_name("k3s"), "k3s");
        assert_eq!(display_name("microk8s"), "microk8s");
        assert_eq!(
            display_name("kubernetes-admin@kubernetes"),
            "kubernetes-admin@kubernetes"
        );
    }

    #[test]
    fn instances_show_a_short_name_but_keep_the_real_context() {
        let f = Fleet::from_contexts(&ctxs(&["apps-eks.example.com"]), &Grouping::default());
        let i = &f.groups[0].instances[0];
        assert_eq!(i.name, "apps-eks");
        assert_eq!(i.context, "apps-eks.example.com");
    }

    #[test]
    fn contexts_that_would_collide_keep_their_full_names() {
        let f = Fleet::from_contexts(
            &ctxs(&["apps.eu.example.com", "apps.us.example.com", "other"]),
            &Grouping::default(),
        );
        let names: Vec<_> = f.instances().iter().map(|i| i.name.clone()).collect();
        assert_eq!(
            names,
            vec!["apps.eu.example.com", "apps.us.example.com", "other"],
            "ambiguous short names must not hide which cluster is which"
        );
    }

    #[test]
    fn a_grouping_still_matches_on_the_full_context_name() {
        let f = Fleet::from_contexts(
            &ctxs(&["apps-eks.example.com"]),
            &grouping(&[("prod", &["apps-eks.example.com"])]),
        );
        assert_eq!(f.groups[0].name, "prod");
        assert_eq!(f.groups[0].instances[0].name, "apps-eks");
    }

    #[test]
    fn lookups_use_the_display_name() {
        let f = Fleet::from_contexts(
            &ctxs(&["apps-eks.example.com", "k3s"]),
            &Grouping::default(),
        );
        assert!(f.find("apps-eks").is_some());
        assert_eq!(f.step("apps-eks", 1).unwrap().name, "k3s");
    }

    #[test]
    fn health_can_be_updated_after_a_probe() {
        let mut f = sample();
        f.set_health("use1-b", Health::Unreachable);
        assert_eq!(f.find("use1-b").unwrap().health, Health::Unreachable);
        assert_eq!(f.find("use1-a").unwrap().health, Health::Unknown);
    }
}
