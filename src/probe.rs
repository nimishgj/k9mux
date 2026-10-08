//! Reachability probes for the cluster API servers.
//!
//! The sidebar dot used to mean "did the k9s process start", which says
//! nothing about the cluster. This opens a TCP connection to each API server
//! instead, so the dot reflects whether the cluster is actually answering.

use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

/// Host and port of an API server URL. Defaults the port by scheme.
pub fn endpoint(server: &str) -> Option<(String, u16)> {
    let (scheme, rest) = server.split_once("://")?;
    let default = match scheme {
        "https" => 443,
        "http" => 80,
        _ => return None,
    };
    // Strip any path, then split off an explicit port.
    let authority = rest.split(['/', '?']).next()?;
    if authority.is_empty() {
        return None;
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => Some((host.to_string(), port.parse().ok()?)),
        _ => Some((authority.to_string(), default)),
    }
}

/// Whether something is listening, within `timeout`.
pub fn reachable(host: &str, port: u16, timeout: Duration) -> bool {
    let Ok(addrs) = (host, port).to_socket_addrs() else {
        return false;
    };
    addrs
        .into_iter()
        .any(|a| TcpStream::connect_timeout(&a, timeout).is_ok())
}

/// One cluster to watch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub instance: String,
    pub host: String,
    pub port: u16,
}

/// Probes clusters on a background thread and reports what it finds.
pub struct Prober {
    rx: Receiver<(String, bool)>,
}

impl Prober {
    pub fn spawn(targets: Vec<Target>, every: Duration, timeout: Duration) -> Prober {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            loop {
                for t in &targets {
                    let ok = reachable(&t.host, t.port, timeout);
                    if tx.send((t.instance.clone(), ok)).is_err() {
                        return; // the app is gone
                    }
                }
                if targets.is_empty() {
                    return;
                }
                std::thread::sleep(every);
            }
        });
        Prober { rx }
    }

    /// Results that have arrived since last time. Never blocks.
    pub fn drain(&self) -> Vec<(String, bool)> {
        self.rx.try_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::time::Instant;

    const QUICK: Duration = Duration::from_millis(500);

    #[test]
    fn an_api_server_url_splits_into_host_and_port() {
        assert_eq!(
            endpoint("https://10.0.0.1:6443"),
            Some(("10.0.0.1".into(), 6443))
        );
    }

    #[test]
    fn the_port_defaults_by_scheme() {
        assert_eq!(
            endpoint("https://api.example.com"),
            Some(("api.example.com".into(), 443))
        );
        assert_eq!(
            endpoint("http://api.example.com"),
            Some(("api.example.com".into(), 80))
        );
    }

    #[test]
    fn a_path_is_ignored() {
        assert_eq!(
            endpoint("https://api.example.com:6443/k8s/clusters/c-1"),
            Some(("api.example.com".into(), 6443))
        );
    }

    #[test]
    fn nonsense_urls_are_rejected() {
        assert_eq!(endpoint("api.example.com"), None);
        assert_eq!(endpoint("ftp://example.com"), None);
        assert_eq!(endpoint("https://"), None);
        assert_eq!(endpoint("https://host:notaport"), None);
    }

    #[test]
    fn a_listening_port_is_reachable() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        assert!(reachable("127.0.0.1", port, QUICK));
    }

    #[test]
    fn a_closed_port_is_not_reachable() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        assert!(!reachable("127.0.0.1", port, QUICK));
    }

    #[test]
    fn a_name_that_does_not_resolve_is_not_reachable() {
        assert!(!reachable("no-such-host.invalid", 443, QUICK));
    }

    #[test]
    fn an_unreachable_cluster_does_not_hang_the_probe() {
        let start = Instant::now();
        // TEST-NET-1, guaranteed not to route anywhere.
        reachable("192.0.2.1", 443, Duration::from_millis(300));
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "probe took {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn the_prober_reports_what_it_finds() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let up = l.local_addr().unwrap().port();
        let dead = TcpListener::bind("127.0.0.1:0").unwrap();
        let down = dead.local_addr().unwrap().port();
        drop(dead);

        let p = Prober::spawn(
            vec![
                Target {
                    instance: "up".into(),
                    host: "127.0.0.1".into(),
                    port: up,
                },
                Target {
                    instance: "down".into(),
                    host: "127.0.0.1".into(),
                    port: down,
                },
            ],
            Duration::from_millis(50),
            QUICK,
        );

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen: Vec<(String, bool)> = Vec::new();
        while !(seen.iter().any(|(n, _)| n == "up") && seen.iter().any(|(n, _)| n == "down")) {
            seen.extend(p.drain());
            assert!(Instant::now() < deadline, "no results: {seen:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(seen.iter().find(|(n, _)| n == "up").unwrap().1);
        assert!(!seen.iter().find(|(n, _)| n == "down").unwrap().1);
    }

    #[test]
    fn a_prober_with_nothing_to_watch_just_stops() {
        let p = Prober::spawn(Vec::new(), Duration::from_millis(10), QUICK);
        std::thread::sleep(Duration::from_millis(50));
        assert!(p.drain().is_empty());
    }
}
