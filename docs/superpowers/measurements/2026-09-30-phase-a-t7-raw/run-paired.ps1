param([string]$Profile='draft', [int]$Pairs=3, [int]$StartPair=1)
$ErrorActionPreference='Stop'
$env:CARGO_INCREMENTAL='0'
$env:SEKAI_PHASE_A_PROFILE=$Profile
$env:SEKAI_PHASE_A_RUNS='1'
$taskScratch=Join-Path (Get-Location) '.superpowers/sdd/2026-09-30-phase-a-authoring-loop'
$taskRaw=Join-Path (Get-Location) 'docs/superpowers/measurements/2026-09-30-phase-a-t7-raw'
for ($pair=$StartPair; $pair -lt $StartPair+$Pairs; $pair++) {
    $order=if ($pair % 2 -eq 1) { @('before','after') } else { @('after','before') }
    foreach ($side in $order) {
        $exe=Join-Path $taskScratch "t7-$side/generation_latency.exe"
        $stem=Join-Path $taskRaw "$Profile-pair-$pair-$side"
        $utc=[DateTime]::UtcNow.ToString('o')
        $clock=[System.Diagnostics.Stopwatch]::StartNew()
        $process=Start-Process -FilePath $exe -ArgumentList @('observe_phase_a_latency','--ignored','--exact','--nocapture','--test-threads=1') -WindowStyle Hidden -RedirectStandardOutput "$stem.stdout.log" -RedirectStandardError "$stem.stderr.log" -PassThru
        $process.WaitForExit()
        $clock.Stop()
        $process.Refresh()
        $receipt=@{
            side=$side; pair=$pair; profile=$Profile; pid=$process.Id;
            start_utc=$utc; end_utc=[DateTime]::UtcNow.ToString('o');
            process_wall_s=$clock.Elapsed.TotalSeconds;
            process_cpu_s=$process.TotalProcessorTime.TotalSeconds;
            exit=$process.ExitCode; binary=$exe; sha256=(Get-FileHash $exe).Hash;
            incremental=$env:CARGO_INCREMENTAL; rustflags=$env:RUSTFLAGS;
            rayon=$env:RAYON_NUM_THREADS; cold_runs=$env:SEKAI_PHASE_A_RUNS;
            cache='fresh MemoryStageCache; P1 cold; then in-process same-input hot';
            args=@('observe_phase_a_latency','--ignored','--exact','--nocapture','--test-threads=1')
        }
        $receipt | ConvertTo-Json -Depth 3 | Set-Content "$stem.receipt.json"
        Get-Content "$stem.stderr.log"
        Get-Content "$stem.stdout.log"
        if ($process.ExitCode -ne 0) { exit $process.ExitCode }
    }
}
exit 0
