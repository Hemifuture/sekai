param(
    [Parameter(Mandatory=$true)][string]$OldExecutable,
    [Parameter(Mandatory=$true)][string]$CurrentExecutable
)
$env:CARGO_INCREMENTAL='0'
$cases=@(
    @{Revision='a716138ad619ebc2cd335b7a7fdaa6b79c6876ab';Name='standard_full_chain_within_one_minute';Profile='standard';Runs=1;Executable=$OldExecutable;Log='old-standard-valid.log';Cache='fresh_process_and_stage_cache';Origin='unchanged_original_budget'},
    @{Revision='f2ad407507668f9d02727c3b235920738940a8fc+T6_source_manifest';Name='observe_phase_a_latency';Profile='draft';Runs=3;Executable=$CurrentExecutable;Log='current-draft.log';Cache='sequential_cold_stage_caches_then_hot_same_process';Origin='non_gating_observer'},
    @{Revision='f2ad407507668f9d02727c3b235920738940a8fc+T6_source_manifest';Name='observe_phase_a_latency';Profile='standard';Runs=1;Executable=$CurrentExecutable;Log='current-standard.log';Cache='cold_then_hot_stage_cache_same_process';Origin='non_gating_observer'}
)
try {
    foreach ($case in $cases) {
        $env:SEKAI_PHASE_A_PROFILE=$case.Profile
        $env:SEKAI_PHASE_A_RUNS=[string]$case.Runs
        $started=(Get-Date).ToUniversalTime().ToString('o')
        & $case.Executable $case.Name --ignored --exact --nocapture --test-threads=1 *> (Join-Path $PSScriptRoot $case.Log)
        $result=$LASTEXITCODE
        [pscustomobject]@{Revision=$case.Revision;Profile=$case.Profile;RequestedColdRuns=$case.Runs;StartedUtc=$started;EndedUtc=(Get-Date).ToUniversalTime().ToString('o');ExitCode=$result;Seed=42;Cache=$case.Cache;Origin=$case.Origin;Log=$case.Log} | ConvertTo-Json -Compress | Add-Content (Join-Path $PSScriptRoot 'remaining-runs.jsonl')
        Get-Content (Join-Path $PSScriptRoot $case.Log) | Select-String '\[latency.*surface .*graph .*total |PHASE_A_SAMPLE|test result:' | ForEach-Object {$_.Line}
    }
} finally {
    Remove-Item Env:SEKAI_PHASE_A_PROFILE,Env:SEKAI_PHASE_A_RUNS -ErrorAction SilentlyContinue
}
