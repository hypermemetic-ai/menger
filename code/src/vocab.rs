//! The service's typed parameters (ACCESS-8, `typed_params`): what names a
//! repository, a ref, a commit and a path in its tree. Each a transparent
//! text whose `Deserialize` is its one rule, so a bad value is
//! `invalid_params` naming the parameter before the method runs — and
//! before any request to GitHub. Whether the repository, ref, commit or
//! path is there is the service's answer, in band.

macro_rules! text_type {
    ($(#[$doc:meta])* $name:ident, $rule:expr) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, schemars::JsonSchema)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Its text, as it goes on the wire.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Its text, owned.
            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = String;
            fn try_from(text: String) -> Result<Self, String> {
                let rule: fn(&str) -> Result<String, String> = $rule;
                rule(&text).map(Self)
            }
        }

        impl std::str::FromStr for $name {
            type Err = String;
            fn from_str(text: &str) -> Result<Self, String> {
                let rule: fn(&str) -> Result<String, String> = $rule;
                rule(text).map(Self)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                Self::try_from(<String as serde::Deserialize>::deserialize(d)?).map_err(serde::de::Error::custom)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
    };
}

/// GitHub's grammar for an owner (a user or an organization): 1 to 39
/// letters, digits and hyphens, never a hyphen first or last.
fn owner_ok(o: &str) -> bool {
    (1..=39).contains(&o.len()) && o.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') && !o.starts_with('-') && !o.ends_with('-')
}

/// GitHub's grammar for a repository's name: 1 to 100 letters, digits,
/// `.`, `_` and `-`, never `.` or `..`.
fn name_ok(n: &str) -> bool {
    (1..=100).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)) && n != "." && n != ".."
}

fn repo(text: &str) -> Result<String, String> {
    let t = text.trim();
    match t.split_once('/') {
        Some((o, n)) if owner_ok(o) && name_ok(n) => Ok(t.to_ascii_lowercase()),
        _ => Err(format!("bad_repo: `{t}` is not a GitHub repository — owner/name, e.g. hypermemetic-ai/menger (nothing was asked of GitHub)")),
    }
}

text_type!(
    /// A public GitHub repository, `owner/name` in GitHub's grammar
    /// (lower-cased: GitHub reads both without regard to case). Anything
    /// else — another host, a path, `..` — is refused `bad_repo` before any
    /// request is made.
    RepoName,
    repo
);

fn ref_name(text: &str) -> Result<String, String> {
    let t = text.trim();
    let bad = t.is_empty()
        || t.len() > 255
        || t.starts_with(['-', '/'])
        || t.ends_with(['/', '.'])
        || t.ends_with(".lock")
        || t.contains("..")
        || t.contains("//")
        || t.contains("@{")
        || t.chars().any(|c| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(c));
    match bad {
        false => Ok(t.to_string()),
        true => Err(format!("`{t}` is not a branch or tag's name, e.g. main, v2 or v0.8.0 (refs/heads/… and refs/tags/… are read too)")),
    }
}

text_type!(
    /// A branch or a tag, by its name (`main`, `v0.8.0`), or in full
    /// (`refs/heads/main`, `refs/tags/v0.8.0`): git's grammar for a ref.
    RefName,
    ref_name
);

fn commit(text: &str) -> Result<String, String> {
    let t = text.trim();
    match (7..=40).contains(&t.len()) && t.bytes().all(|b| b.is_ascii_hexdigit()) {
        true => Ok(t.to_ascii_lowercase()),
        false => Err(format!("`{t}` is not a commit: its hash, 7 to 40 hex digits")),
    }
}

text_type!(
    /// A commit, by its hash: 7 to 40 hex digits (lower-cased); a short one
    /// names the kept commit it begins.
    CommitHash,
    commit
);

fn tree_path(text: &str) -> Result<String, String> {
    if text.len() > 4096 || text.chars().any(char::is_control) {
        return Err("a path in the tree is at most 4096 characters, with no control characters".into());
    }
    let parts: Vec<&str> = text.split('/').filter(|p| !p.is_empty()).collect();
    match parts.iter().any(|p| *p == "." || *p == "..") {
        false => Ok(parts.join("/")),
        true => Err(format!("`{text}` is not a path in the tree: no `.` or `..` segment")),
    }
}

text_type!(
    /// A path in a commit's tree, from its root (`ui/src/app.svelte`); a
    /// leading or trailing `/` is read past, and the root is empty. No `.`
    /// or `..` segment.
    TreePath,
    tree_path
);

#[cfg(test)]
mod tests {
    use super::*;

    fn holds<T: std::str::FromStr<Err = String> + std::fmt::Debug>(yes: &[&str], no: &[&str]) {
        for y in yes {
            assert!(y.parse::<T>().is_ok(), "{y:?} holds");
        }
        for n in no {
            assert!(n.parse::<T>().is_err(), "{n:?} is refused");
        }
    }

    /// Scenario "not GitHub": another host, a path, `..` — refused
    /// `bad_repo` by the type, so before any request.
    #[test]
    fn a_repository_is_githubs_owner_and_name_and_nothing_else() {
        holds::<RepoName>(&["hypermemetic-ai/menger", "a/b", "Rust-Lang/rust.vim", "o/.github", "o/a_b-c.d"], &["git.example.org/a/b", "a/b/c", "../b", "a/..", "a/.", "", "a", "/b", "a/", "-a/b", "a-/b", "a b/c", "https://github.com/a/b", "a/b?x", &format!("{}/b", "a".repeat(40))]);
        for bad in ["git.example.org/a/b", "a/b/c", "../b"] {
            let e = bad.parse::<RepoName>().unwrap_err();
            assert!(e.starts_with("bad_repo: "), "{e}");
            assert!(serde_json::from_str::<RepoName>(&format!("\"{bad}\"")).is_err(), "deserializing is the check");
        }
        assert_eq!("Hypermemetic-AI/Menger".parse::<RepoName>().unwrap().as_str(), "hypermemetic-ai/menger");
    }

    #[test]
    fn refs_commits_and_paths_hold_their_spellings() {
        holds::<RefName>(&["main", "v2", "v0.8.0", "CODE-2/code-service", "refs/heads/main", "refs/tags/v1"], &["", "a..b", "-x", "a b", "a:b", "x.lock", "a~1", "HEAD^", "/x", "x/"]);
        holds::<CommitHash>(&["9c41d07", "9C41D07E", &"a".repeat(40)], &["9c41d0", "xyzxyzx", &"a".repeat(41), ""]);
        assert_eq!("9C41D07E".parse::<CommitHash>().unwrap().as_str(), "9c41d07e");
        holds::<TreePath>(&["", "/", "ui/src", "/ui/src/", "a.b/.c"], &["../etc/passwd", "a/../b", "./a", "a\u{0}b"]);
        assert_eq!("/ui//src/".parse::<TreePath>().unwrap().as_str(), "ui/src");
    }
}
