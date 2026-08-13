//! Passive release notification: once a day, ask GitHub whether a newer
//! version has been published and, if so, offer a link to the releases
//! page.
//!
//! Deliberately passive. Vibez never downloads or installs anything on
//! the user's behalf, so the only action the notice offers is opening a
//! browser. Every failure path here is silent: a DAW is routinely run on
//! machines with no network (studios, stages), and a release check that
//! could not complete is not a problem the user needs to hear about. An
//! unreachable network therefore behaves exactly like the feature being
//! switched off.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Where the notice's action sends the user. Human-facing page rather
/// than a binary, because this feature never installs anything.
pub const RELEASES_PAGE_URL: &str = "https://github.com/alexanderwanyoike/vibez/releases";

/// Unauthenticated endpoint for the newest published, non-draft release.
const LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/alexanderwanyoike/vibez/releases/latest";

/// Minimum spacing between two network checks. GitHub rate-limits
/// unauthenticated callers by IP, and knowing about a release a few
/// hours sooner is worth neither that budget nor the traffic.
pub const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

/// Ceiling on the whole request, so a connection that opens but never
/// answers cannot park a runtime worker for the life of the session.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// GitHub rejects unauthenticated API requests that arrive without a
/// User-Agent, so this is required rather than decorative.
const USER_AGENT: &str = concat!("vibez/", env!("CARGO_PKG_VERSION"));

/// A release version reduced to the three numbers that order it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReleaseVersion {
    major: u64,
    minor: u64,
    patch: u64,
}

impl ReleaseVersion {
    /// Parse a release tag such as `0.2.0` or `v0.2.0`.
    ///
    /// Returns `None` for anything that is not exactly three numeric
    /// components, pre-release suffixes included. Two reasons: a tag we
    /// cannot parse is indistinguishable from a tagging scheme this
    /// build predates, where the safe answer is to stay quiet; and a
    /// passive notifier should never nudge a user on a stable build
    /// towards `0.2.0-rc1`.
    pub fn parse(tag: &str) -> Option<Self> {
        let trimmed = tag.trim();
        let digits = trimmed.strip_prefix('v').unwrap_or(trimmed);
        let mut parts = digits.split('.');
        let major = parts.next()?.parse::<u64>().ok()?;
        let minor = parts.next()?.parse::<u64>().ok()?;
        let patch = parts.next()?.parse::<u64>().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
        })
    }
}

impl std::fmt::Display for ReleaseVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The version string to advertise, or `None` when `tag` is not strictly
/// newer than `current`.
///
/// The returned string is re-rendered from the parsed numbers rather
/// than echoed, so the notice reads the same whether or not upstream
/// tagged with a `v` prefix.
pub fn newer_release(tag: &str, current: &str) -> Option<String> {
    let latest = ReleaseVersion::parse(tag)?;
    let current = ReleaseVersion::parse(current)?;
    (latest > current).then(|| latest.to_string())
}

/// The version this binary was built as, for comparison against a tag.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Whether a startup check is due.
///
/// Split out as a pure function so the opt-out and the throttle can be
/// tested without a clock or a network.
pub fn should_check(enabled: bool, last_check_unix: Option<u64>, now_unix: u64) -> bool {
    if !enabled {
        return false;
    }
    match last_check_unix {
        None => true,
        // A timestamp in the future means the clock moved backwards
        // (or the settings file was hand-edited). Treat that as due
        // rather than locking the check out until real time catches up.
        Some(last) => now_unix < last || now_unix.saturating_sub(last) >= CHECK_INTERVAL_SECS,
    }
}

/// Wall-clock seconds since the Unix epoch, saturating at 0 for clocks
/// set before 1970 so a nonsense clock cannot panic startup.
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// Runtime half of the feature: the persisted preference and timestamp,
/// plus whatever the last check turned up.
#[derive(Debug, Clone)]
pub struct UpdateCheckState {
    /// Persisted preference. When false no network call is made at all.
    pub enabled: bool,
    /// Persisted timestamp of the last completed attempt, recorded
    /// whether or not it found anything, so a machine that is offline
    /// every morning still backs off instead of retrying every launch.
    pub last_check_unix: Option<u64>,
    /// Version to advertise, once a check has found a newer one.
    pub available: Option<String>,
    /// Dismissed for this session only. Not persisted: the next launch
    /// re-raises it, which is the gentlest way to keep a genuinely
    /// stale install from going unnoticed forever.
    pub dismissed: bool,
    /// A check is currently on the wire. Runtime-only: gates the manual
    /// "Check now" button so a slow request cannot be stacked.
    pub in_flight: bool,
}

impl Default for UpdateCheckState {
    fn default() -> Self {
        Self {
            enabled: true,
            last_check_unix: None,
            available: None,
            dismissed: false,
            in_flight: false,
        }
    }
}

impl UpdateCheckState {
    /// Whether the status bar should currently show the notice.
    pub fn notice(&self) -> Option<&str> {
        if self.dismissed {
            return None;
        }
        self.available.as_deref()
    }

    /// Claim the right to start a check. Returns false when one is
    /// already on the wire, so callers can skip spawning a duplicate.
    pub fn begin_check(&mut self) -> bool {
        if self.in_flight {
            return false;
        }
        self.in_flight = true;
        true
    }

    /// Fold the outcome of a check into the state. Called for both
    /// outcomes so the throttle advances even when the request failed.
    pub fn record_result(&mut self, tag: Option<String>, now_unix: u64) {
        self.in_flight = false;
        self.last_check_unix = Some(now_unix);
        let Some(tag) = tag else {
            return;
        };
        if let Some(version) = newer_release(&tag, current_version()) {
            // A newer release than the one already showing re-raises a
            // notice the user dismissed earlier this session.
            if self.available.as_deref() != Some(version.as_str()) {
                self.dismissed = false;
            }
            self.available = Some(version);
        }
    }
}

/// The one field of the GitHub release payload this feature reads.
#[derive(serde::Deserialize)]
struct LatestRelease {
    tag_name: String,
}

/// Ask GitHub for the newest release tag, yielding `None` on any
/// failure whatsoever: DNS, TLS, timeout, rate limit, malformed JSON.
/// Callers cannot distinguish the failures because none of them should
/// change what the user sees.
pub async fn fetch_latest_tag() -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .ok()?;
    let response = client
        .get(LATEST_RELEASE_API_URL)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let release: LatestRelease = response.json().await.ok()?;
    Some(release.tag_name)
}

/// Hand the releases page to the platform's browser.
///
/// Reuses the Dropbox OAuth flow's opener rather than taking a second
/// dependency on `open`. `open::that` blocks while it spawns the
/// handler, so it runs on a blocking thread: the UI thread must never
/// wait on a browser launch.
pub async fn open_releases_page() {
    let _ = tokio::task::spawn_blocking(|| {
        use vibez_dropbox::BrowserOpener;
        let _ = vibez_dropbox::SystemBrowserOpener.open(RELEASES_PAGE_URL);
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = CHECK_INTERVAL_SECS;

    #[test]
    fn an_identical_release_offers_no_update() {
        assert_eq!(newer_release("0.1.2", "0.1.2"), None);
    }

    #[test]
    fn a_check_can_be_claimed_once_until_its_result_is_recorded() {
        let mut state = UpdateCheckState::default();

        assert!(state.begin_check());
        assert!(!state.begin_check());

        state.record_result(None, DAY);
        assert!(!state.in_flight);
        assert_eq!(state.last_check_unix, Some(DAY));
        assert!(state.begin_check());
    }

    #[test]
    fn a_newer_release_is_offered() {
        assert_eq!(newer_release("0.2.0", "0.1.2"), Some("0.2.0".to_string()));
    }

    #[test]
    fn an_older_release_is_not_offered() {
        assert_eq!(newer_release("0.1.1", "0.1.2"), None);
    }

    #[test]
    fn a_v_prefixed_tag_is_compared_and_shown_without_its_prefix() {
        assert_eq!(newer_release("v0.2.0", "0.1.2"), Some("0.2.0".to_string()));
        assert_eq!(newer_release("v0.1.2", "0.1.2"), None);
    }

    #[test]
    fn version_components_are_compared_numerically_not_lexically() {
        assert_eq!(newer_release("0.10.0", "0.9.9"), Some("0.10.0".to_string()));
        assert_eq!(newer_release("1.0.0", "0.99.99"), Some("1.0.0".to_string()));
        assert_eq!(newer_release("0.9.9", "0.10.0"), None);
    }

    #[test]
    fn a_malformed_tag_offers_no_update() {
        for tag in ["", "nightly", "0.2", "0.2.0.1", "v", "1.2.x", "0..1"] {
            assert_eq!(newer_release(tag, "0.1.2"), None, "tag {tag:?}");
        }
    }

    #[test]
    fn a_prerelease_tag_offers_no_update() {
        assert_eq!(newer_release("0.2.0-rc1", "0.1.2"), None);
        assert_eq!(newer_release("v0.2.0-beta.1", "0.1.2"), None);
    }

    #[test]
    fn the_shipped_package_version_is_a_comparable_tag() {
        assert!(ReleaseVersion::parse(current_version()).is_some());
    }

    #[test]
    fn a_first_run_checks_immediately() {
        assert!(should_check(true, None, 1_000_000));
    }

    #[test]
    fn a_check_within_the_last_day_is_skipped() {
        assert!(!should_check(true, Some(1_000_000), 1_000_000 + DAY - 1));
    }

    #[test]
    fn a_check_a_full_day_old_runs_again() {
        assert!(should_check(true, Some(1_000_000), 1_000_000 + DAY));
    }

    #[test]
    fn an_opted_out_user_never_checks() {
        assert!(!should_check(false, None, 1_000_000));
        assert!(!should_check(false, Some(0), 1_000_000 + DAY * 30));
    }

    #[test]
    fn a_clock_moved_backwards_still_checks() {
        assert!(should_check(true, Some(2_000_000), 1_000_000));
    }

    #[test]
    fn a_failed_check_advances_the_throttle_without_raising_a_notice() {
        let mut state = UpdateCheckState::default();
        state.record_result(None, 1_000_000);
        assert_eq!(state.last_check_unix, Some(1_000_000));
        assert_eq!(state.notice(), None);
        assert!(!should_check(
            state.enabled,
            state.last_check_unix,
            1_000_100
        ));
    }

    #[test]
    fn an_older_upstream_tag_raises_no_notice() {
        let mut state = UpdateCheckState::default();
        state.record_result(Some("0.0.1".to_string()), 1_000_000);
        assert_eq!(state.notice(), None);
    }

    #[test]
    fn a_dismissed_notice_stays_hidden_for_the_session() {
        let mut state = UpdateCheckState {
            available: Some("9.9.9".to_string()),
            ..Default::default()
        };
        assert_eq!(state.notice(), Some("9.9.9"));
        state.dismissed = true;
        assert_eq!(state.notice(), None);
    }

    #[test]
    fn a_release_newer_than_the_dismissed_one_raises_the_notice_again() {
        let mut state = UpdateCheckState {
            available: Some("9.9.9".to_string()),
            dismissed: true,
            ..Default::default()
        };
        state.record_result(Some("9.9.10".to_string()), 1_000_000);
        assert_eq!(state.notice(), Some("9.9.10"));
    }

    #[test]
    fn re_reporting_the_same_release_leaves_it_dismissed() {
        let mut state = UpdateCheckState {
            available: Some("9.9.9".to_string()),
            dismissed: true,
            ..Default::default()
        };
        state.record_result(Some("9.9.9".to_string()), 1_000_000);
        assert_eq!(state.notice(), None);
    }
}
