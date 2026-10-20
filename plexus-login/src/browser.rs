//! Opening a URL in the person's browser (TETHER-11; DIST-14: shared by
//! `menger --open` and `tether pair --open`). The house rule (DIST-14):
//! print the URL always, open it only when asked.

/// What opens a URL in the browser here, if anything can: `open` on macOS,
/// the URL handler on Windows (not `cmd /c start`, which reads `&` in the
/// URL as its own), `xdg-open` elsewhere — and nothing on a Linux or BSD
/// with no display (SSH, headless), where the URL is only printed. `env`
/// reads a variable (the process's, or a test's).
pub fn opener(os: &str, env: impl Fn(&str) -> Option<String>) -> Option<(String, Vec<String>)> {
    match os {
        "macos" => Some(("open".into(), vec![])),
        "windows" => Some(("rundll32".into(), vec!["url.dll,FileProtocolHandler".into()])),
        _ => {
            let shown = |v: &str| env(v).is_some_and(|s| !s.trim().is_empty());
            (shown("DISPLAY") || shown("WAYLAND_DISPLAY")).then(|| ("xdg-open".into(), vec![]))
        }
    }
}

/// Open `url` with `opener`; whether it started. The child is reaped on a
/// thread of its own, its output dropped.
pub fn open_with(opener: &(String, Vec<String>), url: &str) -> bool {
    let child = std::process::Command::new(&opener.0)
        .args(&opener.1)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    match child {
        Ok(mut c) => {
            std::thread::spawn(move || {
                let _ = c.wait();
            });
            true
        }
        Err(_) => false,
    }
}

/// Open `url` in this machine's browser; whether something started to.
/// `false` (no display, no opener) means: say so, and carry on.
pub fn open(url: &str) -> bool {
    opener(std::env::consts::OS, |v| std::env::var(v).ok()).is_some_and(|o| open_with(&o, url))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
    }

    #[test]
    fn the_browser_opens_only_where_there_is_one() {
        assert_eq!(opener("linux", env(&[("DISPLAY", ":0")])).unwrap().0, "xdg-open");
        assert_eq!(opener("linux", env(&[("WAYLAND_DISPLAY", "wayland-0")])).unwrap().0, "xdg-open");
        assert!(opener("linux", env(&[])).is_none(), "headless: print only");
        assert!(opener("linux", env(&[("SSH_CONNECTION", "1 2 3 4"), ("DISPLAY", "")])).is_none(), "over SSH, no display");
        assert!(opener("freebsd", env(&[])).is_none());
        assert_eq!(opener("macos", env(&[])).unwrap().0, "open");
        let (w, args) = opener("windows", env(&[])).unwrap();
        assert_eq!((w.as_str(), args), ("rundll32", vec!["url.dll,FileProtocolHandler".to_string()]));
    }

    #[test]
    fn an_opener_that_is_not_there_says_so() {
        assert!(!open_with(&("/nonexistent/opener-dist-14".into(), vec![]), "http://127.0.0.1:1/"));
    }
}
