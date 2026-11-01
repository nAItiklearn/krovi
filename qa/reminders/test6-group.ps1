# Test 6: five reminders missed while Krovi was closed -> ONE summary toast (not five), every one marked fired + missed.
. D:\krovi\qa\reminders\qa.ps1
Stop-RemQA | Out-Host
$today = (Get-Date).ToString('yyyy-MM-dd')
$py = @'
import sqlite3, sys
c = sqlite3.connect(sys.argv[1]); d = sys.argv[2]
i, o = c.execute("select max(id), max(ord) from calendar_events").fetchone()
for k, name in enumerate("ABCDE"):
    c.execute("insert into calendar_events (id,date,title,time,ord,remind,sound,fired_at,snoozed_until,missed) values (?,?,?,?,?,1,1,NULL,NULL,0)",
              (i + 1 + k, d, "QA group " + name, "17:%02d" % (k * 10), o + 1 + k))
c.commit()
'@
$py | python -I - (Join-Path $Data 'krovi.db') $today
$mark = if (Test-Path $Log) { (Get-Content $Log | Measure-Object).Count } else { 0 }
Start-RemQA | Out-Host
Start-Sleep -Seconds 5
"--- log of this start"
Get-Content $Log | Select-Object -Skip $mark
"--- DB (the five group rows)"
Db-Events | Select-String -Pattern 'schema|id  date|QA group'

$src = "$env:LOCALAPPDATA\Microsoft\Windows\Notifications"
$tmp = Join-Path $env:TEMP 'krovi-wpn-copy'; New-Item -ItemType Directory -Force $tmp | Out-Null
Copy-Item "$src\wpndatabase.db*" $tmp -Force -ErrorAction SilentlyContinue
$py2 = @'
import sqlite3, sys, datetime, re
c = sqlite3.connect(sys.argv[1])
def ft(t): return datetime.datetime(1601,1,1) + datetime.timedelta(microseconds=t/10) + datetime.timedelta(hours=5, minutes=30)
rows = c.execute("select n.ArrivalTime, n.Payload from Notification n join NotificationHandler h on h.RecordId = n.HandlerId where h.PrimaryId like '%powershell.exe' and (n.Payload like '%QA group%' or n.Payload like '%reminders</text>%') order by n.ArrivalTime").fetchall()
print("toasts for the group (expect exactly one summary):", len(rows))
for a, p in rows:
    p = p.decode('utf-8','replace') if isinstance(p, bytes) else str(p)
    print(ft(a).strftime('%H:%M:%S.%f')[:-3], [t for t in re.findall(r'<text[^>]*>([^<]*)</text>', p) if t])
'@
$py2 | python -I - (Join-Path $tmp 'wpndatabase.db')
