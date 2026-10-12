//! SCAFFOLD-9: the names the login layer writes into a browser and into an
//! identity — the app's, never the crate's.
//!
//! | name | default for app `x` | where |
//! | --- | --- | --- |
//! | [`LoginNames::session_cookie`] | `x_session` | the `HttpOnly` session cookie ([`crate::sessions::Sessions`]) |
//! | [`LoginNames::login_cookie`] | `x_login` | a code-flow sign-in's transaction cookie (`Path=/login`) |
//! | [`LoginNames::dev_cookie`] | `x_dev_login` | the dev sign-in's checkbox (the page's script writes it) |
//! | [`LoginNames::session_key`] | `x_session` | the metadata key a session-backed identity carries: its session's hash, which `alive` asks the table about |
//! | [`LoginNames::flow_cookie`] | `x_flow` | AUTH-22: our own sign-in page's flow cookie (its CSRF token's key, its steps' handle) |
//!
//! The two cookies a shell sets are named per shell (IDENTITY-38):
//! [`LoginNames::session_cookie_for`] and [`LoginNames::login_cookie_for`].
//!
//! `session_key` has exactly one definition — here, in the table that
//! stamps it ([`crate::sessions::Sessions::session_key`]). An app's rules
//! that read it ask the table (menger: tickets-api's `SessionCheck::key`;
//! [`crate::LoginAccess::session_key`] for the auth backend), never a copy.

/// The names the login layer writes. See the module docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginNames {
    /// The session cookie's base name; a shell's own is
    /// [`LoginNames::session_cookie_for`].
    pub session_cookie: String,
    /// The login transaction cookie's base name
    /// ([`LoginNames::login_cookie_for`]).
    pub login_cookie: String,
    /// The dev sign-in's checkbox cookie.
    pub dev_cookie: String,
    /// The metadata key a session-backed identity carries (its session's
    /// hash).
    pub session_key: String,
    /// AUTH-22: our own sign-in page's flow cookie's base name
    /// ([`LoginNames::flow_cookie_for`]).
    pub flow_cookie: String,
}

impl LoginNames {
    /// The names for an app called `app`: `<app>_session`, `<app>_login`,
    /// `<app>_dev_login`, and `<app>_session` as the metadata key. The name
    /// is made a cookie token first: lower case, anything but a letter, a
    /// digit or `_` becomes `_` (`My App` → `my_app`).
    ///
    /// ```
    /// let n = plexus_login::LoginNames::for_app("menger");
    /// assert_eq!(n.session_cookie, "menger_session");
    /// assert_eq!(n.login_cookie, "menger_login");
    /// assert_eq!(n.dev_cookie, "menger_dev_login");
    /// assert_eq!(n.session_key, "menger_session");
    /// ```
    pub fn for_app(app: &str) -> Self {
        let mut token: String =
            app.trim().chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
        if token.is_empty() {
            token = "plexus".into();
        }
        Self {
            session_cookie: format!("{token}_session"),
            login_cookie: format!("{token}_login"),
            dev_cookie: format!("{token}_dev_login"),
            session_key: format!("{token}_session"),
            flow_cookie: format!("{token}_flow"),
        }
    }

    /// IDENTITY-38: the session cookie's name for a shell reached at
    /// `public_url`. Cookies ignore ports, so two servers on one host (prod
    /// on 127.0.0.1:4456, test on :4451) sharing one name would sign each
    /// other out and hand each other their ids: over http the name carries
    /// the port; over https it is `__Host-` (Secure, host-only, `Path=/`),
    /// which no other origin can set or shadow.
    pub fn session_cookie_for(&self, public_url: &str) -> String {
        per_shell(&self.session_cookie, "__Host-", public_url)
    }

    /// The login transaction cookie's name for a shell reached at
    /// `public_url`: named per shell as the session's is, `__Secure-` where
    /// the session's is `__Host-` (this one lives at `Path=/login`, which
    /// `__Host-` forbids).
    pub fn login_cookie_for(&self, public_url: &str) -> String {
        per_shell(&self.login_cookie, "__Secure-", public_url)
    }

    /// AUTH-22: the flow cookie's name for a shell at `public_url`:
    /// `__Host-` over https (it lives at `Path=/`), the port's over http.
    pub fn flow_cookie_for(&self, public_url: &str) -> String {
        per_shell(&self.flow_cookie, "__Host-", public_url)
    }
}

fn per_shell(base: &str, secure_prefix: &str, public_url: &str) -> String {
    match url::Url::parse(public_url) {
        Ok(u) if u.scheme() == "https" => format!("{secure_prefix}{base}"),
        Ok(u) => match u.port_or_known_default() {
            Some(p) => format!("{base}_{p}"),
            None => base.to_string(),
        },
        Err(_) => base.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_shell_names_its_own_cookies() {
        let n = LoginNames::for_app("menger");
        assert_eq!(n.session_cookie_for("http://127.0.0.1:4456"), "menger_session_4456");
        assert_eq!(n.session_cookie_for("http://127.0.0.1:4451/"), "menger_session_4451");
        assert_eq!(n.session_cookie_for("http://menger.localhost"), "menger_session_80");
        assert_eq!(n.session_cookie_for("https://menger.example"), "__Host-menger_session");
        assert_eq!(n.login_cookie_for("http://127.0.0.1:4456"), "menger_login_4456");
        assert_eq!(n.login_cookie_for("https://menger.example"), "__Secure-menger_login");
        assert_eq!(n.flow_cookie_for("https://menger.example"), "__Host-menger_flow");
        assert_eq!(n.flow_cookie_for("http://127.0.0.1:4471"), "menger_flow_4471");
    }

    #[test]
    fn an_app_name_becomes_a_cookie_token() {
        let n = LoginNames::for_app("My App-2");
        assert_eq!(n.session_cookie, "my_app_2_session");
        assert_eq!(n.dev_cookie, "my_app_2_dev_login");
        assert_eq!(LoginNames::for_app("  ").session_cookie, "plexus_session");
    }
}
