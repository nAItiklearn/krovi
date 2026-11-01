# Test 4: a voice-created reminder. Text-to-speech WAV -> Krovi's real SAPI command recogniser ->
# `add-reminder` -> commands::add_calendar_event_from -> reminder with alert on -> fires at its time.
param([string]$Said = 'remind me in two minutes to stretch your legs')
. D:\krovi\qa\reminders\qa.ps1
$mark = (Get-Content $Log | Measure-Object).Count
$wav = Join-Path $QR 'voice-reminder.wav'
Add-Type -AssemblyName System.Speech
$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
$fmt = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(16000, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, [System.Speech.AudioFormat.AudioChannel]::Mono)
$synth.SetOutputToWaveFile($wav, $fmt); $synth.Speak($Said); $synth.SetOutputToNull(); $synth.Dispose()
"spoken (TTS) at $((Get-Date).ToString('HH:mm:ss')): '$Said'  -> $wav"
$p = $wav.Replace('\', '/')
"--- voice_transcribe_file result (the same object the UI receives as voice-result):"
Cdp "window.__TAURI_INTERNALS__.invoke('voice_transcribe_file', {path: '$p', mode: 'command'}).then(r => ({text: r.text, command: r.command, executed: r.executed, feedback: r.feedback}))"
"--- the reminder it created (remind/sound default on):"
Cdp "window.__TAURI_INTERNALS__.invoke('load_calendar').then(l => l.slice(-1))"
Cdp "window.__TAURI_INTERNALS__.invoke('reminders_status').then(s => s.next)"
