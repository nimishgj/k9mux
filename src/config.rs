use crate::fleet::Grouping;
use crate::keymap::{KeySpec, Keymap, parse_command};

/// k9mux' config, a line per directive in the style of tmux.conf.
///
/// ```text
/// set prefix M-space
/// bind '"' split-horizontal
/// bind -n M-h focus-left
/// unbind x
/// kubeconfig ~/.kube/config-prod
/// cluster prod-us use1-a use1-b
/// ```
#[derive(Debug, Default)]
pub struct Config {
    pub kubeconfigs: Vec<String>,
    pub clusters: Vec<(String, Vec<String>)>,
    pub keymap: Keymap,
}

/// Splits a line into words, respecting single and double quotes so that
/// `bind '"' split-horizontal` and `bind "%" ...` both work.
pub fn tokenize(line: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;

    for c in line.chars() {
        match quote {
            Some(q) if c == q => {
                quote = None;
            }
            Some(_) => word.push(c),
            None if c == '\'' || c == '"' => {
                quote = Some(c);
                started = true;
            }
            // A `#` outside quotes starts a comment.
            None if c == '#' && !started => break,
            None if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            None => {
                word.push(c);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return Err("unclosed quote".to_string());
    }
    if started {
        out.push(word);
    }
    Ok(out)
}

impl Config {
    pub fn parse(text: &str) -> Result<Config, String> {
        let mut cfg = Config::default();
        for (n, line) in text.lines().enumerate() {
            let at = |msg: String| format!("line {}: {msg}", n + 1);
            let words = tokenize(line).map_err(&at)?;
            let Some(directive) = words.first() else {
                continue;
            };
            let args = &words[1..];

            match directive.as_str() {
                "set" => {
                    let [name, value] = args else {
                        return Err(at("usage: set prefix <key>".into()));
                    };
                    if name != "prefix" {
                        return Err(at(format!("unknown setting {name:?}")));
                    }
                    cfg.keymap.prefix = KeySpec::parse(value).map_err(at)?;
                }
                "bind" | "unbind" => {
                    let root = args.first().is_some_and(|a| a == "-n");
                    let args = if root { &args[1..] } else { args };
                    let binding = directive == "bind";
                    let wanted = if binding { 2 } else { 1 };
                    if args.len() != wanted {
                        return Err(at(format!(
                            "usage: {directive} [-n] <key>{}",
                            if binding { " <command>" } else { "" }
                        )));
                    }
                    let key = KeySpec::parse(&args[0]).map_err(at)?;
                    if binding {
                        let action = parse_command(&args[1])
                            .ok_or_else(|| at(format!("unknown command {:?}", args[1])))?;
                        cfg.keymap.bind(key, action, root);
                    } else {
                        cfg.keymap.unbind(key, root);
                    }
                }
                "kubeconfig" => {
                    let [path] = args else {
                        return Err(at("usage: kubeconfig <path>".into()));
                    };
                    cfg.kubeconfigs.push(path.clone());
                }
                "cluster" => {
                    let Some((name, contexts)) = args.split_first() else {
                        return Err(at("usage: cluster <name> <context>...".into()));
                    };
                    cfg.clusters.push((name.clone(), contexts.to_vec()));
                }
                other => return Err(at(format!("unknown directive {other:?}"))),
            }
        }
        Ok(cfg)
    }
    pub fn grouping(&self) -> Grouping {
        Grouping {
            clusters: self.clusters.clone(),
        }
    }
    /// `~/.config/k9mux/config`
    pub fn path() -> Option<std::path::PathBuf> {
        let home = std::env::var_os("HOME")?;
        Some(
            std::path::Path::new(&home)
                .join(".config")
                .join("k9mux")
                .join("config"),
        )
    }
    /// Reads the config if it is there; a missing file is not an error.
    pub fn load() -> Result<Config, String> {
        let Some(path) = Config::path() else {
            return Ok(Config::default());
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => Config::parse(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{Action, Dir, Outcome};
    use crate::layout::Split;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn k(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }
    fn alt(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT)
    }
    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn words_split_on_whitespace() {
        assert_eq!(
            tokenize("bind x close-pane").unwrap(),
            ["bind", "x", "close-pane"]
        );
        assert_eq!(tokenize("   bind    x   close-pane  ").unwrap().len(), 3);
    }

    #[test]
    fn quotes_hold_a_word_together() {
        assert_eq!(
            tokenize("bind '\"' split-horizontal").unwrap(),
            ["bind", "\"", "split-horizontal"]
        );
        assert_eq!(tokenize("bind \"%\" x").unwrap(), ["bind", "%", "x"]);
        assert_eq!(
            tokenize("cluster 'prod us' a").unwrap(),
            ["cluster", "prod us", "a"]
        );
    }

    #[test]
    fn an_unclosed_quote_is_an_error() {
        assert!(tokenize("bind 'x close-pane").is_err());
    }

    #[test]
    fn an_empty_config_keeps_the_defaults() {
        let c = Config::parse("").unwrap();
        assert!(c.kubeconfigs.is_empty());
        assert!(c.clusters.is_empty());
        assert_eq!(c.keymap.resolve(alt(' '), false), Outcome::Armed);
        assert_eq!(
            c.keymap.resolve(k('x'), true),
            Outcome::Do(Action::ClosePane)
        );
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let c = Config::parse("# a comment\n\n   \n# another\n").unwrap();
        assert!(c.clusters.is_empty());
    }

    #[test]
    fn a_trailing_comment_is_stripped() {
        let c = Config::parse("bind z quit   # why not").unwrap();
        assert_eq!(c.keymap.resolve(k('z'), true), Outcome::Do(Action::Quit));
    }

    #[test]
    fn the_prefix_can_be_changed() {
        let c = Config::parse("set prefix C-b").unwrap();
        assert_eq!(c.keymap.resolve(ctrl('b'), false), Outcome::Armed);
        assert_ne!(c.keymap.resolve(alt(' '), false), Outcome::Armed);
    }

    #[test]
    fn a_key_can_be_rebound() {
        let c = Config::parse("bind s split-vertical").unwrap();
        assert_eq!(
            c.keymap.resolve(k('s'), true),
            Outcome::Do(Action::Split(Split::Vertical))
        );
        // and the default is still there alongside it
        assert_eq!(
            c.keymap.resolve(k('%'), true),
            Outcome::Do(Action::Split(Split::Vertical))
        );
    }

    #[test]
    fn rebinding_a_default_replaces_it() {
        let c = Config::parse("bind % quit").unwrap();
        assert_eq!(c.keymap.resolve(k('%'), true), Outcome::Do(Action::Quit));
    }

    #[test]
    fn dash_n_binds_without_the_prefix() {
        let c = Config::parse("bind -n C-t focus-next").unwrap();
        assert_eq!(
            c.keymap.resolve(ctrl('t'), false),
            Outcome::Do(Action::FocusNext)
        );
    }

    #[test]
    fn focus_keys_can_be_moved_somewhere_else() {
        let c = Config::parse(
            "unbind -n M-h\nunbind -n M-l\nbind -n C-Left focus-left\nbind -n C-Right focus-right",
        )
        .unwrap();
        assert_eq!(
            c.keymap.resolve(alt('h'), false),
            Outcome::Forward(b"\x1bh".to_vec()),
            "the old binding should be gone"
        );
        assert_eq!(
            c.keymap
                .resolve(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL), false),
            Outcome::Do(Action::Focus(Dir::Left))
        );
    }

    #[test]
    fn unbind_hands_a_key_back_to_k9s() {
        let c = Config::parse("unbind x").unwrap();
        assert_eq!(c.keymap.resolve(k('x'), true), Outcome::Ignore);
    }

    #[test]
    fn kubeconfigs_accumulate_in_order() {
        let c = Config::parse("kubeconfig ~/.kube/a\nkubeconfig ~/.kube/b").unwrap();
        assert_eq!(c.kubeconfigs, vec!["~/.kube/a", "~/.kube/b"]);
    }

    #[test]
    fn a_cluster_lists_its_contexts() {
        let c = Config::parse("cluster prod-us use1-a use1-b\ncluster prod-eu euc1-a").unwrap();
        assert_eq!(c.clusters.len(), 2);
        assert_eq!(c.clusters[0].0, "prod-us");
        assert_eq!(c.clusters[0].1, vec!["use1-a", "use1-b"]);
        assert_eq!(c.grouping().clusters[1].0, "prod-eu");
    }

    #[test]
    fn errors_name_the_line_and_the_problem() {
        let err = Config::parse("bind x close-pane\nbind y not-a-command").unwrap_err();
        assert!(err.contains("line 2"), "{err}");
        assert!(err.contains("not-a-command"), "{err}");

        let err = Config::parse("set prefix Nope").unwrap_err();
        assert!(err.contains("line 1"), "{err}");

        let err = Config::parse("wobble x").unwrap_err();
        assert!(err.contains("wobble"), "{err}");

        let err = Config::parse("bind x").unwrap_err();
        assert!(err.contains("line 1"), "{err}");

        assert!(Config::parse("set something-else 1").is_err());
        assert!(Config::parse("cluster").is_err());
    }

    #[test]
    fn the_shipped_defaults_parse_and_match_the_built_in_keymap() {
        let from_text = Config::parse(crate::keymap::DEFAULTS).unwrap().keymap;
        let built_in = Keymap::default();
        assert_eq!(from_text.prefix, built_in.prefix);
        for (key, action) in &built_in.prefixed {
            assert!(
                from_text.prefixed.contains(&(*key, action.clone())),
                "{} missing from the documented defaults",
                key.label()
            );
        }
        for (key, action) in &built_in.root {
            assert!(
                from_text.root.contains(&(*key, action.clone())),
                "{} missing from the documented defaults",
                key.label()
            );
        }
    }

    #[test]
    fn a_full_config_reads_end_to_end() {
        let c = Config::parse(
            r#"
# k9mux
set prefix C-b

bind '"' split-horizontal
bind %   split-vertical
bind -n M-h focus-left

kubeconfig ~/.kube/config-prod
kubeconfig ~/.kube/config-remote

cluster prod use1-a use1-b
cluster edge k3s
"#,
        )
        .unwrap();
        assert_eq!(c.kubeconfigs.len(), 2);
        assert_eq!(c.clusters.len(), 2);
        assert_eq!(c.keymap.resolve(ctrl('b'), false), Outcome::Armed);
        assert_eq!(
            c.keymap.resolve(k('"'), true),
            Outcome::Do(Action::Split(Split::Horizontal))
        );
    }
}
