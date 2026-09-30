param([Parameter(Mandatory=$true)][string]$Executable)
$env:CARGO_INCREMENTAL='0'
$cases = @(
    @{ Name='draft_full_chain_within_twenty_seconds'; Profile='draft'; Runs=3 },
    @{ Name='standard_full_chain_within_one_minute'; Profile='standard'; Runs=1 }
)
foreach ($case in $cases) {
    for ($run=1; $run -le $case.Runs; $run++) {
        $log=Join-Path $PSScriptRoot "old-$($case.Profile)-$run.log"
        $started=(Get-Date).ToUniversalTime().ToString('o')
        & $Executable $case.Name --ignored --exact --nocapture --test-threads=1 *> $log
        $result=$LASTEXITCODE
        [pscustomobject]@{Revision='a716138ad619ebc2cd335b7a7fdaa6b79c6876ab';Profile=$case.Profile;Run=$run;StartedUtc=$started;EndedUtc=(Get-Date).ToUniversalTime().ToString('o');ExitCode=$result;Seed=42;Cache='fresh_process_and_stage_cache';Log=[IO.Path]::GetFileName($log)} | ConvertTo-Json -Compress | Add-Content (Join-Path $PSScriptRoot 'old-runs.jsonl')
        Get-Content $log | Select-String '\[latency.*surface .*graph .*total |test result:' | ForEach-Object {$_.Line}
    }
}
