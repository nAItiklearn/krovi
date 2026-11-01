# Generates test speech with Windows' built-in TTS (System.Speech), fully offline.
#
#   powershell -File gen-wavs.ps1 -OutDir <dir> [-Manifest utterances.tsv] [-Rate 16000]
#              [-Voice "Microsoft Zira Desktop"] [-SpeechRate 0] [-Prefix zira-] [-Noise 0.0]
#
# -Manifest: UTF-8 file, one "slug<TAB>spoken text" per line. Without it, a built-in list is used.
# -Noise:    0..1, amplitude of white noise mixed into the 16-bit PCM after synthesis (crude
#            robustness check; real microphones are noisier and less clean than TTS).
# Each utterance is written to <dir>\<Prefix><slug>.wav.
param(
    [Parameter(Mandatory = $true)][string]$OutDir,
    [string]$Manifest = "",
    [int]$Rate = 16000,
    [string]$Voice = "",
    [int]$SpeechRate = 0,
    [string]$Prefix = "",
    [double]$Noise = 0.0
)

Add-Type -AssemblyName System.Speech
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

if ($Manifest -ne "") {
    $utterances = [ordered]@{}
    Get-Content -Path $Manifest -Encoding UTF8 | Where-Object { $_.Trim() -ne "" } | ForEach-Object {
        $parts = $_ -split "`t", 2
        $utterances[$parts[0]] = $parts[1]
    }
} else {
    $utterances = [ordered]@{
        "cmd-open-timers"      = "open timers"
        "cmd-start-pomodoro"   = "start pomodoro"
        "cmd-timer-five-min"   = "start a timer for five minutes"
        "cmd-new-note"         = "new note buy milk"
        "cmd-close"            = "close"
        "cmd-open-notes"       = "open notes"
        "cmd-open-clipboard"   = "show me the clipboard"
        "cmd-go-weather"       = "go to the weather"
        "cmd-timer-25"         = "set a timer for twenty five minutes"
        "cmd-open-settings"    = "open settings"
        "note-dentist"         = "remember to call the dentist tomorrow morning"
        "note-long"            = "The meeting with the design team moved to three thirty. Please send them the updated mockups before lunch."
        "chatter"              = "the quick brown fox jumps over the lazy dog"
    }
}

$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
if ($Voice -ne "") { $synth.SelectVoice($Voice) }
$synth.Rate = $SpeechRate
$fmt = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo($Rate, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, [System.Speech.AudioFormat.AudioChannel]::Mono)
$rng = New-Object System.Random 12345

function Add-Noise([string]$path, [double]$amount) {
    # Mix uniform noise into the PCM payload (assumes the canonical 44-byte header System.Speech writes).
    $bytes = [System.IO.File]::ReadAllBytes($path)
    for ($i = 44; $i + 1 -lt $bytes.Length; $i += 2) {
        $s = [BitConverter]::ToInt16($bytes, $i)
        $n = [int](($rng.NextDouble() * 2 - 1) * 32767 * $amount)
        $v = [Math]::Max(-32768, [Math]::Min(32767, $s + $n))
        $b = [BitConverter]::GetBytes([int16]$v)
        $bytes[$i] = $b[0]; $bytes[$i + 1] = $b[1]
    }
    [System.IO.File]::WriteAllBytes($path, $bytes)
}

foreach ($kv in $utterances.GetEnumerator()) {
    $path = Join-Path $OutDir ($Prefix + $kv.Key + ".wav")
    $synth.SetOutputToWaveFile($path, $fmt)
    $synth.Speak($kv.Value)
    $synth.SetOutputToNull()
    if ($Noise -gt 0) { Add-Noise $path $Noise }
}
$synth.Dispose()
Write-Output ("wrote {0} wavs to {1} (voice='{2}', {3} Hz, noise={4})" -f $utterances.Count, $OutDir, $Voice, $Rate, $Noise)
