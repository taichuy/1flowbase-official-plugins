//! WeChat's rolling reply window. Public evidence is separate from credentials.
use super::*;

pub(super) const WINDOW_MS: u64 = 24 * 60 * 60 * 1000;
type Window = NotificationDeliveryWindowView;
type WindowStatus = NotificationDeliveryWindowStatus;

pub(super) fn waiting() -> Window {
    Window {
        status: WindowStatus::WaitingForMessage,
        last_received_at_ms: None,
        expires_at_ms: None,
    }
}
pub(super) fn unknown(previous: Option<Window>) -> Window {
    let mut window = previous.unwrap_or(Window {
        status: WindowStatus::Unknown,
        last_received_at_ms: None,
        expires_at_ms: None,
    });
    window.status = WindowStatus::Unknown;
    window
}
pub(super) fn from_last(last: u64, now: u64) -> Option<Window> {
    if last == 0 || last > now {
        return None;
    }
    let expires = last.checked_add(WINDOW_MS)?;
    Some(Window {
        status: if now < expires {
            WindowStatus::Active
        } else {
            WindowStatus::Expired
        },
        last_received_at_ms: Some(last),
        expires_at_ms: Some(expires),
    })
}
pub(super) fn cached(window: Option<Window>, now: u64) -> Window {
    window
        .as_ref()
        .and_then(|window| window.last_received_at_ms)
        .and_then(|last| from_last(last, now))
        .unwrap_or_else(|| {
            if window
                .as_ref()
                .is_some_and(|window| window.status == WindowStatus::WaitingForMessage)
            {
                waiting()
            } else {
                unknown(None)
            }
        })
}

pub(super) fn failure(window: &Window) -> Option<NotificationFailureCode> {
    match window.status {
        WindowStatus::Active => None,
        WindowStatus::WaitingForMessage => Some(NotificationFailureCode::SubscriptionRequired),
        WindowStatus::Expired => Some(NotificationFailureCode::SubscriptionExpired),
        WindowStatus::Unknown => Some(NotificationFailureCode::SubscriptionUnavailable),
    }
}
