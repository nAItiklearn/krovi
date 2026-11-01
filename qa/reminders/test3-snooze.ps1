# Test 3: snooze rings again after N minutes (30 s here, via the fractional-minute QA argument);
# a dismissed snooze never rings. Same scheduler path as the 5/10-minute buttons.
. D:\krovi\qa\reminders\qa.ps1
$mark = (Get-Content $Log | Measure-Object).Count
$t0 = Get-Date
"snoozing 'QA fire test 2' (id 2) for 0.5 min at $($t0.ToString('HH:mm:ss.fff'))"
$null = Inv 'reminder_snooze' '{id: 2, minutes: 0.5}'
"snoozing 'QA fire test' (id 1) for 5 min, then dismissing it at once"
$null = Inv 'reminder_snooze' '{id: 1, minutes: 5}'
"--- DB right after snoozing (snoozed_until set, fired_at cleared for both):"
Db-Events
"--- pending per the scheduler:"
Cdp "window.__TAURI_INTERNALS__.invoke('reminders_status').then(s => s.pending.map(p => ({id: p.id, title: p.title, dueLocal: p.dueLocal, snoozed: p.snoozed, inMs: p.inMs})))"
$null = Inv 'reminder_dismiss' '{id: 1}'
"--- after dismissing id 1:"
Db-Events

# wait for the 30 s snooze to ring
while ((Get-Date) -lt $t0.AddSeconds(36)) { Start-Sleep -Milliseconds 300 }
"--- log since the snooze"
Get-Content $Log | Select-Object -Skip $mark
"--- DB after the snooze rang"
Db-Events
"--- (waiting 6 minutes is unnecessary: id 1 was dismissed, so no repeat can be pending)"
Cdp "window.__TAURI_INTERNALS__.invoke('reminders_status').then(s => ({pending: s.pending.length, fires: s.scheduler.fires, lastFire: {id: s.scheduler.lastFire.id, scheduled: s.scheduler.lastFire.scheduledLocal, shown: s.scheduler.lastFire.shownLocal, lateMs: s.scheduler.lastFire.shownLateMs}}))"
