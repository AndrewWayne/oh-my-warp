/// At the UI framework level, we have structs for interacting with
/// platform-level notification data (mostly in the form of strings).
/// Similar structs are available at the app level, which are more
/// specific to the data we make use of.
use chrono::NaiveDateTime;
use serde::Serialize;

/// Content to be sent as a notification to the user. Includes `data` that is sent back to
/// application when the notification is clicked--see `NotificationResponse` for more details.  
#[derive(Clone, Debug)]
pub struct UserNotification {
    title: String,
    body: String,
    // Arbitrary data associated with the notification.
    data: Option<String>,
    // Whether to play sound with the notification.
    play_sound: bool,
    // Named system/bundle sound to play (resolved against `~/Library/Sounds`
    // or the app bundle on macOS). `None` = platform default sound when
    // `play_sound` is true. Additive/optional: platforms that don't support it
    // fall back to their existing default-sound behavior.
    sound_name: Option<String>,
    // Coalescing identifier for the platform notification. `None` = platform
    // default (a shared constant). Set per-pane so repeated notifications from
    // the same pane replace each other and distinct panes never clobber.
    identifier: Option<String>,
}

impl UserNotification {
    /// These limits were discovered experimentally, by testing with example
    /// commands/outputs and ensuring the text was not truncated in most cases.
    /// The official MacOS docs do not mention specific byte/char constraints.
    /// In reality, the strings are limited by the sum of width of the chars,
    /// which is dependent on the string itself (e.g. 'W' is much wider than ' ').
    pub const MAX_TITLE_LENGTH: usize = 40;
    pub const MAX_BODY_LENGTH: usize = 120;

    pub fn new(title: String, body: String, data: Option<String>) -> Self {
        Self {
            title,
            body,
            data,
            play_sound: true,
            sound_name: None,
            identifier: None,
        }
    }

    pub fn new_with_sound(
        title: String,
        body: String,
        data: Option<String>,
        play_sound: bool,
    ) -> Self {
        Self {
            title,
            body,
            data,
            play_sound,
            sound_name: None,
            identifier: None,
        }
    }

    pub fn with_data(mut self, data: impl Into<String>) -> Self {
        self.data = Some(data.into());
        self
    }

    /// Set a named sound (system/bundle/`~/Library/Sounds`). Empty resolves to
    /// the platform default sound.
    pub fn with_sound_name(mut self, sound_name: Option<String>) -> Self {
        self.sound_name = sound_name.filter(|s| !s.is_empty());
        self
    }

    /// Set the coalescing identifier (typically per-pane).
    pub fn with_identifier(mut self, identifier: impl Into<String>) -> Self {
        let id = identifier.into();
        self.identifier = if id.is_empty() { None } else { Some(id) };
        self
    }

    pub fn title(&self) -> &str {
        self.title.as_str()
    }

    pub fn body(&self) -> &str {
        self.body.as_str()
    }

    pub fn data(&self) -> Option<&str> {
        self.data.as_deref()
    }

    pub fn play_sound(&self) -> bool {
        self.play_sound
    }

    pub fn sound_name(&self) -> Option<&str> {
        self.sound_name.as_deref()
    }

    pub fn identifier(&self) -> Option<&str> {
        self.identifier.as_deref()
    }
}

/// A response sent when a notification sent by the app was clicked.
#[derive(Debug)]
pub struct NotificationResponse {
    // Time the notification was sent.
    sent_date: NaiveDateTime,

    /// The data associated with the notification, if any. This matches the data included in the
    /// `NotificationContent` when the notification was sent.  
    data: Option<String>,
}

impl NotificationResponse {
    pub fn new(sent_date: NaiveDateTime, data: Option<String>) -> Self {
        NotificationResponse { sent_date, data }
    }

    pub fn sent_date(&self) -> NaiveDateTime {
        self.sent_date
    }

    pub fn data(&self) -> Option<&str> {
        self.data.as_deref()
    }
}

#[derive(Clone, Debug, Serialize)]
pub enum NotificationSendError {
    /// App does not have permissions to send notifications.
    PermissionsDenied,

    /// On web, there's a difference between permissions being default and being denied. While they are still default,
    /// we should prompt the user to accept or block notifications, since they haven't chosen yet.
    PermissionsNotYetGranted,

    /// Some unknown error occurred when sending the a notification.
    Other { error_message: String },
}

impl NotificationSendError {
    pub fn notifications_error_banner_title(&self) -> &str {
        match self {
            NotificationSendError::PermissionsDenied | NotificationSendError::PermissionsNotYetGranted => "Warp tried to send you a notification for the last block but does not have permission.",
            NotificationSendError::Other { .. } => "Warp tried to send you a notification for the last block, but something went wrong.",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub enum RequestPermissionsOutcome {
    /// User accepted the request for permissions.
    Accepted,
    /// User explicitly denied permissions.
    PermissionsDenied,
    /// Some unknown error occurred when requesting permissions.
    OtherError { error_message: String },
}

/// Render a notification text template by substituting `{key}` placeholders
/// with the provided values (MS3). Placeholders with no matching variable are
/// left untouched so typos stay visible rather than silently vanishing.
///
/// Used for pane-focus notification title/body templates, e.g.
/// `render_notification_template("{project}: done", &[("project", "omw")])`
/// → `"omw: done"`.
pub fn render_notification_template(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (key, value) in vars {
        out = out.replace(&format!("{{{key}}}"), value);
    }
    out
}

#[cfg(test)]
mod pane_focus_notification_tests {
    use super::{render_notification_template, UserNotification};

    #[test]
    fn render_template_substitutes_known_and_keeps_unknown() {
        let s = render_notification_template(
            "{project}: {status}",
            &[("project", "omw"), ("status", "done")],
        );
        assert_eq!(s, "omw: done");
        // Unknown placeholder left as-is.
        let s2 = render_notification_template("hi {who}", &[("project", "x")]);
        assert_eq!(s2, "hi {who}");
        // Empty template stays empty.
        assert_eq!(render_notification_template("", &[("a", "b")]), "");
    }

    /// MS2: builders set the per-pane identifier and named sound.
    #[test]
    fn builders_set_sound_name_and_identifier() {
        let n = UserNotification::new("t".into(), "b".into(), None)
            .with_identifier("omw-notif-pane-1")
            .with_sound_name(Some("Glass".into()));
        assert_eq!(n.identifier(), Some("omw-notif-pane-1"));
        assert_eq!(n.sound_name(), Some("Glass"));
    }

    /// MS2: empty identifier / sound name normalize to `None` (platform default).
    #[test]
    fn empty_values_normalize_to_none() {
        let n = UserNotification::new("t".into(), "b".into(), None)
            .with_identifier(String::new())
            .with_sound_name(Some(String::new()));
        assert_eq!(n.identifier(), None);
        assert_eq!(n.sound_name(), None);
    }

    /// MS2: constructors default to no custom sound / no identifier (legacy behavior).
    #[test]
    fn constructors_default_to_none() {
        let n = UserNotification::new_with_sound("t".into(), "b".into(), None, true);
        assert_eq!(n.sound_name(), None);
        assert_eq!(n.identifier(), None);
        assert!(n.play_sound());
    }
}
