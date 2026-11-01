# Test 2: reminders that come due while Krovi is CLOSED fire once on the next start, marked missed;
# anything >24 h overdue is only marked; a further restart fires nothing (no double fire).
. D:\krovi\qa\reminders\qa.ps1
Remove-Item $Log -ErrorAction SilentlyContinue
Start-RemQA | Out-Host

# 1. a reminder due in ~1 minute, then the app is closed before it is due
$t = In-Minutes 1
$null = Inv 'reminder_add' "{date:'$($t.date)', title:'QA closed at the time', time:'$($t.time)', remind:true, sound:true}"
"added 'QA closed at the time' for $($t.time); closing Krovi (QA instance) at $((Get-Date).ToString('HH:mm:ss'))"
Stop-RemQA | Out-Host

# 2. while it is closed: seed two older, still-unfired reminders straight into the QA database
#    (one 12 h overdue today, one two days overdue = stale)
$today = (Get-Date).ToString('yyyy-MM-dd'); $old = (Get-Date).AddDays(-2).ToString('yyyy-MM-dd')
$py = @'
import sqlite3, sys
c = sqlite3.connect(sys.argv[1]); d, old = sys.argv[2], sys.argv[3]
mx = c.execute("select max(id), max(ord) from calendar_events").fetchone()
i, o = mx[0] + 1, mx[1] + 1
c.execute("insert into calendar_events (id,date,title,time,ord,remind,sound,fired_at,snoozed_until,missed) values (?,?,?,?,?,1,1,NULL,NULL,0)", (i, d, "QA overdue 12h (today 06:30)", "06:30", o))
c.execute("insert into calendar_events (id,date,title,time,ord,remind,sound,fired_at,snoozed_until,missed) values (?,?,?,?,?,1,1,NULL,NULL,0)", (i + 1, old, "QA stale 2 days", "09:00", o + 1))
c.commit()
'@
$py | python -I - (Join-Path $Data 'krovi.db') $today $old
"--- DB while Krovi is closed (the three unfired reminders):"
Db-Events

# 3. wait until the first one is >60 s overdue, then start Krovi
while ((Get-Date) -lt $t.at.AddSeconds(75)) { Start-Sleep -Milliseconds 500 }
"starting Krovi at $((Get-Date).ToString('HH:mm:ss.fff')) ('QA closed at the time' was due $($t.time):00, so ~75 s overdue)"
Start-RemQA | Out-Host
Start-Sleep -Seconds 4
"--- log after the first start"
Get-Content $Log
"--- DB after the first start"
Db-Events

# 4. restart again: nothing may fire a second time
$before = (Get-Content $Log | Measure-Object).Count
Stop-RemQA | Out-Host
Start-RemQA | Out-Host
Start-Sleep -Seconds 4
"--- log lines added by the SECOND start (expect only 'scheduler up ... 0 pending'):"
Get-Content $Log | Select-Object -Skip $before
"--- DB after the second start (unchanged)"
Db-Events
