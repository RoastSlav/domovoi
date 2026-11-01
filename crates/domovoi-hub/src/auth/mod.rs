//! Single-admin authentication: first-run setup, argon2id passwords, sessions, CSRF and
//! web approvals (ADR 0007, INV-1, INV-8).
//!
//! This module is the only place in the hub's web surface allowed to construct a
//! [`HumanApproval`](domovoi_core::HumanApproval); `xtask arch-check` enforces it.

pub mod approval;
pub mod csrf;
pub mod interactions;
pub mod limit;
pub mod password;
pub mod proxy;
pub mod session;

use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use domovoi_core::ports::{Clock, SessionRepo, StoreError, UserRepo};
use domovoi_core::{Session, User, UserId};
use jiff::{SignedDuration, Timestamp};
use subtle::ConstantTimeEq;
use url::Url;

use self::limit::LoginLimiter;

/// How long the first-run setup token works.
pub const SETUP_TTL: SignedDuration = SignedDuration::from_hours(24);

/// The longest username.
pub const MAX_USERNAME_CHARS: usize = 64;

/// Why authentication failed. Messages are deliberately generic.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    /// Wrong username or password.
    #[error("wrong username or password")]
    InvalidCredentials,
    /// Too many failed logins from this address.
    #[error("too many attempts; try again in {retry_after_secs} s")]
    LockedOut {
        /// Seconds until the next attempt is allowed.
        retry_after_secs: i64,
    },
    /// Setup is only possible before the first user exists.
    #[error("setup is closed")]
    SetupClosed,
    /// The setup token is wrong, used or expired.
    #[error("the setup link is not valid")]
    BadSetupToken,
    /// The password is shorter than the minimum.
    #[error(
        "the password must have at least {} characters",
        password::MIN_PASSWORD_CHARS
    )]
    WeakPassword,
    /// The username is empty, too long or has whitespace or control characters.
    #[error("the username must be 1 to 64 characters without spaces")]
    InvalidUsername,
    /// The store failed.
    #[error("store error: {0}")]
    Store(String),
    /// Hashing failed.
    #[error("could not hash the password")]
    Hash,
}

impl From<StoreError> for AuthError {
    fn from(err: StoreError) -> Self {
        Self::Store(err.to_string())
    }
}

/// Where the hub is reached, for cookies, the Origin check and the setup link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebIdentity {
    /// The URL people open, for example `https://domovoi.home.example`.
    pub public_url: Url,
    /// Addresses whose `X-Forwarded-*` headers are trusted.
    pub trusted_proxies: Vec<IpAddr>,
}

impl WebIdentity {
    /// Whether cookies need `Secure`.
    #[must_use]
    pub fn https(&self) -> bool {
        self.public_url.scheme() == "https"
    }

    /// The origin requests must come from, for example `https://domovoi.home.example`.
    #[must_use]
    pub fn origin(&self) -> String {
        self.public_url.origin().ascii_serialization()
    }
}

struct SetupToken {
    hash: [u8; 32],
    expires_at: Timestamp,
}

/// A logged-in session, found from its cookie.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Current {
    /// Who.
    pub user: User,
    /// The session's stored hash.
    pub session_hash: [u8; 32],
    /// The session's CSRF token.
    pub csrf_token: String,
}

/// A session just created, to be sent as a cookie.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSession {
    /// Who.
    pub user: User,
    /// The cookie value; never stored.
    pub id: String,
    /// The session's CSRF token.
    pub csrf_token: String,
}

/// Users, sessions, the setup token and login limits.
pub struct AuthState {
    users: Arc<dyn UserRepo>,
    sessions: Arc<dyn SessionRepo>,
    clock: Arc<dyn Clock>,
    web: WebIdentity,
    setup: Mutex<Option<SetupToken>>,
    limiter: Mutex<LoginLimiter>,
}

impl std::fmt::Debug for AuthState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthState")
            .field("web", &self.web)
            .finish_non_exhaustive()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn valid_username(name: &str) -> bool {
    let chars = name.chars().count();
    (1..=MAX_USERNAME_CHARS).contains(&chars)
        && !name.chars().any(|c| c.is_whitespace() || c.is_control())
}

impl AuthState {
    /// Auth over `users` and `sessions` for a hub reached as `web`.
    #[must_use]
    pub fn new(
        users: Arc<dyn UserRepo>,
        sessions: Arc<dyn SessionRepo>,
        clock: Arc<dyn Clock>,
        web: WebIdentity,
    ) -> Self {
        Self {
            users,
            sessions,
            clock,
            web,
            setup: Mutex::new(None),
            limiter: Mutex::new(LoginLimiter::default()),
        }
    }

    /// Where the hub is reached.
    #[must_use]
    pub fn web(&self) -> &WebIdentity {
        &self.web
    }

    /// Whether no user exists yet, so the setup link is the only way in.
    pub async fn setup_required(&self) -> Result<bool, AuthError> {
        Ok(self.users.count().await? == 0)
    }

    /// While no user exists, issues a single-use setup token valid for 24 h and returns the
    /// setup link, which the caller logs once. Returns `None` once a user exists or while
    /// an issued token is still valid.
    pub async fn open_setup(&self) -> Result<Option<String>, AuthError> {
        if self.users.count().await? > 0 {
            return Ok(None);
        }
        let now = self.clock.now();
        let mut setup = lock(&self.setup);
        if setup.as_ref().is_some_and(|t| t.expires_at > now) {
            return Ok(None);
        }
        let bytes: [u8; 32] = rand::random();
        let token = hex::encode(bytes);
        *setup = Some(SetupToken {
            hash: session::csrf_hash(&token),
            expires_at: now.checked_add(SETUP_TTL).unwrap_or(now),
        });
        let mut link = self.web.public_url.clone();
        link.set_path("/setup");
        link.set_query(Some(&format!("token={token}")));
        Ok(Some(link.to_string()))
    }

    /// Creates the admin account with the setup token and logs them in.
    pub async fn setup(
        &self,
        token: &str,
        username: &str,
        password: &str,
    ) -> Result<NewSession, AuthError> {
        if self.users.count().await? > 0 {
            return Err(AuthError::SetupClosed);
        }
        let now = self.clock.now();
        {
            let setup = lock(&self.setup);
            let valid = setup.as_ref().is_some_and(|t| {
                t.expires_at > now && bool::from(t.hash.ct_eq(&session::csrf_hash(token)))
            });
            if !valid {
                return Err(AuthError::BadSetupToken);
            }
        }
        if !valid_username(username) {
            return Err(AuthError::InvalidUsername);
        }
        if password.chars().count() < password::MIN_PASSWORD_CHARS {
            return Err(AuthError::WeakPassword);
        }
        let password_hash = password::hash(password).map_err(|_| AuthError::Hash)?;
        // Consume the token before the user exists, so it can never be used twice.
        if lock(&self.setup).take().is_none() {
            return Err(AuthError::BadSetupToken);
        }
        let user = User {
            id: UserId::new(),
            username: username.to_owned(),
            password_hash,
            created_at: now,
        };
        self.users.create(&user).await?;
        tracing::info!(username = %user.username, "admin account created");
        self.start(user).await
    }

    /// Logs in `username` from `ip`, subject to the per-address limit.
    pub async fn login(
        &self,
        ip: IpAddr,
        username: &str,
        password: &str,
    ) -> Result<NewSession, AuthError> {
        let now = self.clock.now();
        if let Some(wait) = lock(&self.limiter).locked(ip, now) {
            return Err(AuthError::LockedOut {
                retry_after_secs: wait.as_secs().max(1),
            });
        }
        let user = self.users.get_by_username(username).await?;
        let stored = user
            .as_ref()
            .map_or(password::dummy_hash(), |u| u.password_hash.as_str());
        let matches = password::verify(password, stored);
        match user {
            Some(user) if matches => {
                lock(&self.limiter).succeeded(ip);
                self.start(user).await
            }
            _ => {
                lock(&self.limiter).failed(ip, now);
                tracing::info!(%ip, "failed login");
                Err(AuthError::InvalidCredentials)
            }
        }
    }

    /// Checks the logged-in `user`'s password again before a change that widens trust.
    /// Shares the login limit for `ip`, so it cannot be used to guess faster.
    pub async fn confirm_password(
        &self,
        ip: IpAddr,
        user: domovoi_core::UserId,
        password: &str,
    ) -> Result<(), AuthError> {
        let now = self.clock.now();
        if let Some(wait) = lock(&self.limiter).locked(ip, now) {
            return Err(AuthError::LockedOut {
                retry_after_secs: wait.as_secs().max(1),
            });
        }
        let stored = self.users.get(user).await?;
        let hash = stored
            .as_ref()
            .map_or(password::dummy_hash(), |u| u.password_hash.as_str());
        if stored.is_some() && password::verify(password, hash) {
            lock(&self.limiter).succeeded(ip);
            Ok(())
        } else {
            lock(&self.limiter).failed(ip, now);
            tracing::info!(%ip, "failed password confirmation");
            Err(AuthError::InvalidCredentials)
        }
    }

    async fn start(&self, user: User) -> Result<NewSession, AuthError> {
        let now = self.clock.now();
        let id = session::new_id();
        let csrf_token = session::csrf_token(&id);
        self.sessions
            .create(&Session {
                token_hash: session::id_hash(&id),
                user: user.id,
                csrf_hash: session::csrf_hash(&csrf_token),
                created_at: now,
                expires_at: session::deadline(now, now),
            })
            .await?;
        Ok(NewSession {
            user,
            id,
            csrf_token,
        })
    }

    /// The live session for cookie value `id`, extending its idle deadline.
    pub async fn current(&self, id: &str) -> Result<Option<Current>, AuthError> {
        let now = self.clock.now();
        let hash = session::id_hash(id);
        let Some(stored) = self.sessions.get(&hash, now).await? else {
            return Ok(None);
        };
        let Some(user) = self.users.get(stored.user).await? else {
            return Ok(None);
        };
        let deadline = session::deadline(stored.created_at, now);
        let due = stored
            .expires_at
            .checked_add(session::EXTEND_EVERY)
            .is_ok_and(|t| t <= deadline);
        if due {
            self.sessions.extend(&hash, deadline).await?;
        }
        Ok(Some(Current {
            user,
            session_hash: hash,
            csrf_token: session::csrf_token(id),
        }))
    }

    /// Whether `token` is the CSRF token of session `id`.
    #[must_use]
    pub fn csrf_matches(id: &str, token: &str) -> bool {
        let expected = session::csrf_hash(&session::csrf_token(id));
        bool::from(expected.ct_eq(&session::csrf_hash(token)))
    }

    /// Ends the session with cookie value `id`.
    pub async fn logout(&self, id: &str) -> Result<(), AuthError> {
        self.sessions.delete(&session::id_hash(id)).await?;
        Ok(())
    }
}
