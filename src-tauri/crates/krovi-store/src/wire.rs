//! Wire shapes for the clipboard and calendar commands/events.
//!
//! These are the JSON objects the UI receives (`src/lib/types.ts` mirrors them), kept
//! here — next to the models they flatten — so the exact field names and the
//! `clipboard-changed` payload are covered by unit tests that run without the Tauri
//! runtime (the Tauri crate's own test binary cannot start on Windows without the
//! app manifest).

use krovi_core::{CalendarEvent, ClipKind, ClipboardHistory};
use serde::Serialize;

/// A clipboard history row as the UI sees it. Field names are snake_case on the wire
/// to match `ClipEntry` in `types.ts`. For `Image` clips `content` is a
/// `data:image/png;base64,…` thumbnail.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ClipDto {
    pub id: u64,
    pub kind: String,
    pub content: String,
    pub pinned: bool,
    pub created_at: u64,
}

/// Payload of the `clipboard-changed` event (`newId` is camelCase).
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ClipChanged<'a> {
    pub entries: &'a [ClipDto],
    /// Id of a *new* entry (drives the pill's arrival glow), or `null` for a pure
    /// mutation (pin, remove, clear, copy-back).
    pub new_id: Option<u64>,
}

/// A calendar event; `time` is omitted entirely for all-day events. The reminder fields
/// are camelCase on the wire (`firedAt`, `snoozedUntil`); `firedAt` / `snoozedUntil` are
/// omitted until they exist (epoch ms).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CalendarDto {
    pub id: u64,
    pub date: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    /// Alert at `time`.
    pub remind: bool,
    /// Play a sound with the alert.
    pub sound: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fired_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snoozed_until: Option<u64>,
    /// The last alert was delivered late.
    pub missed: bool,
}

/// Payload of the `reminder-fired` event (docs/CONTRACTS.md §8).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReminderFired {
    pub id: u64,
    pub title: String,
    pub date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    /// Delivered late (the app was closed or the PC asleep at the time).
    pub missed: bool,
    /// Epoch ms it was delivered.
    pub fired_at: u64,
    /// A sound was requested for this alert.
    pub sound: bool,
    /// From the QA test-fire command: the stored reminder was not changed.
    pub test: bool,
}

pub fn clip_kind_str(k: ClipKind) -> &'static str {
    match k {
        ClipKind::Text => "Text",
        ClipKind::Link => "Link",
        ClipKind::Image => "Image",
    }
}

pub fn parse_clip_kind(s: &str) -> Result<ClipKind, String> {
    match s {
        "Text" => Ok(ClipKind::Text),
        "Link" => Ok(ClipKind::Link),
        "Image" => Ok(ClipKind::Image),
        other => Err(format!("unknown clip kind: {other}")),
    }
}

/// Display order: pinned first, then unpinned, each newest-first.
pub fn clip_dtos(h: &ClipboardHistory) -> Vec<ClipDto> {
    h.visible()
        .into_iter()
        .map(|e| ClipDto {
            id: e.id,
            kind: clip_kind_str(e.kind).to_string(),
            content: e.content.clone(),
            pinned: e.pinned,
            created_at: e.created_at,
        })
        .collect()
}

pub fn calendar_dtos(events: &[CalendarEvent]) -> Vec<CalendarDto> {
    events
        .iter()
        .map(|e| CalendarDto {
            id: e.id,
            date: e.date.clone(),
            title: e.title.clone(),
            time: e.time.clone(),
            remind: e.remind,
            sound: e.sound,
            fired_at: e.fired_at,
            snoozed_until: e.snoozed_until,
            missed: e.missed,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn kind_names_round_trip() {
        for k in [ClipKind::Text, ClipKind::Link, ClipKind::Image] {
            assert_eq!(parse_clip_kind(clip_kind_str(k)).unwrap(), k);
        }
        assert!(parse_clip_kind("Video").is_err());
    }

    #[test]
    fn clip_dtos_are_pinned_first_then_newest_first() {
        let mut h = ClipboardHistory::default();
        let a = h.add(ClipKind::Text, "a", 1);
        h.add(ClipKind::Link, "https://b.c", 2);
        h.pin(a);
        let dtos = clip_dtos(&h);
        assert_eq!(
            dtos.iter().map(|d| d.content.as_str()).collect::<Vec<_>>(),
            vec!["a", "https://b.c"]
        );
        assert_eq!(
            serde_json::to_value(&dtos[1]).unwrap(),
            json!({"id": 2, "kind": "Link", "content": "https://b.c", "pinned": false, "created_at": 2})
        );
    }

    #[test]
    fn clipboard_changed_matches_the_contract() {
        let dtos = vec![ClipDto {
            id: 7,
            kind: "Text".into(),
            content: "x".into(),
            pinned: false,
            created_at: 5,
        }];
        assert_eq!(
            serde_json::to_value(ClipChanged {
                entries: &dtos,
                new_id: Some(7)
            })
            .unwrap(),
            json!({
                "entries": [{"id": 7, "kind": "Text", "content": "x", "pinned": false, "created_at": 5}],
                "newId": 7
            })
        );
        let none = serde_json::to_value(ClipChanged {
            entries: &[],
            new_id: None,
        })
        .unwrap();
        assert_eq!(none, json!({"entries": [], "newId": null}));
    }

    #[test]
    fn calendar_dto_omits_a_missing_time_and_unset_reminder_stamps() {
        let events = vec![
            CalendarEvent::new(1, "2026-11-01", "all day", None),
            CalendarEvent::new(2, "2026-11-01", "standup", Some("09:30".into())),
        ];
        let v = serde_json::to_value(calendar_dtos(&events)).unwrap();
        assert_eq!(
            v[0],
            json!({"id": 1, "date": "2026-11-01", "title": "all day",
                   "remind": false, "sound": true, "missed": false})
        );
        assert_eq!(
            v[1],
            json!({"id": 2, "date": "2026-11-01", "title": "standup", "time": "09:30",
                   "remind": true, "sound": true, "missed": false})
        );
    }

    #[test]
    fn calendar_dto_carries_the_reminder_state_camel_cased() {
        let mut e = CalendarEvent::new(3, "2026-11-01", "tea", Some("10:00".into()));
        e.fired_at = Some(1_793_000_000_000);
        e.snoozed_until = Some(1_793_000_300_000);
        e.missed = true;
        e.sound = false;
        let v = serde_json::to_value(&calendar_dtos(&[e])[0]).unwrap();
        assert_eq!(v["firedAt"], 1_793_000_000_000u64);
        assert_eq!(v["snoozedUntil"], 1_793_000_300_000u64);
        assert_eq!(v["missed"], true);
        assert_eq!(v["sound"], false);
        assert!(v.get("fired_at").is_none() && v.get("snoozed_until").is_none());
    }

    #[test]
    fn reminder_fired_matches_the_contract() {
        let fired = ReminderFired {
            id: 4,
            title: "Call mum".into(),
            date: "2026-11-01".into(),
            time: Some("17:00".into()),
            missed: false,
            fired_at: 99,
            sound: true,
            test: false,
        };
        assert_eq!(
            serde_json::to_value(&fired).unwrap(),
            json!({"id": 4, "title": "Call mum", "date": "2026-11-01", "time": "17:00",
                   "missed": false, "firedAt": 99, "sound": true, "test": false})
        );
    }
}
