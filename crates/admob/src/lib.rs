//! AdMob API: accounts, apps, ad units, and rewarded ad unit creation.
//!
//! AdMob takes user OAuth only. [`auth::login`] runs the installed-app flow
//! once (browser, loopback redirect, PKCE) and caches a refresh token;
//! [`AdmobClient`] then refreshes access tokens silently. Creating an ad unit
//! (`accounts.adUnits.create`, v1beta) is limited access and answers 403
//! without a grant from Google; that case maps to
//! [`Error::CreateNotPermitted`].

pub mod auth;
pub mod client;
pub mod error;
pub mod types;

pub use auth::{OAuthClient, Pkce, StoredToken};
pub use client::AdmobClient;
pub use error::{Error, Result};
pub use types::{Account, AdFormat, AdUnit, App, CreateAdUnit, RewardSettings};
