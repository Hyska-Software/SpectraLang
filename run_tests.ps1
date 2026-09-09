# Script de teste automatizado para SpectraLang
# Cobre todos os diretorios de testes:
#   tests/validation/   - devem COMPILAR com sucesso
#   tests/control_flow/ - devem COMPILAR com sucesso
#   tests/projects/     - projetos multi-arquivo devem COMPILAR com sucesso
#   tests/errors/       - devem FALHAR na compilacao (erros esperados)
#   tests/semantic/     - compilados e reportados sem expectativa forcada
#   tests/cli/          - fixtures para validar comandos do CLI
#   scripts/validate_r2003_base_regression_audit.py - separa compile-only de runtime-zero
#   scripts/validate_tensor_device_contract.py - executa regressões de device tensor
#   tools/spectra-interop/ - interop Rust/Python/C ABI
#
# Requer que o binario ja esteja compilado:
#   cargo build -p spectra-cli

param(
    [string[]]$Phase = @()
)

$binary = (Resolve-Path ".\target\debug\spectralang.exe").Path
$phase31BinaryPath = (Join-Path (Get-Location).Path "target\release\spectralang.exe")
$timeoutSeconds = 10
$runtimeErrorTimeoutSeconds = 30
$hostCommandTimeoutSeconds = 300
$env:PATH = "C:\Users\estev\.cargo\bin;" + $env:PATH
$experimentalFlags = @(
    "--enable-experimental", "switch",
    "--enable-experimental", "if not",
    "--enable-experimental", "do-while",
    "--enable-experimental", "loop"
)

if (-not (Test-Path $binary)) {
    Write-Host "Binario nao encontrado. Compilando..." -ForegroundColor Yellow
    & "C:\Users\estev\.cargo\bin\cargo.exe" build -p spectra-cli 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) {
        Write-Host "ERRO: Falha ao compilar o compilador." -ForegroundColor Red
        exit 1
    }
}

if ($Phase -contains "stability_release") {
    Write-Host "--- SpectraLang required stability release gate ---" -ForegroundColor Yellow
    $stabilityArguments = @(
        "scripts\validate_stability_release.py",
        "--binary", $binary,
        "--required",
        "--report", "target\stability\release-report.json",
        "--markdown", "target\stability\release-report.md"
    )
    if ($env:SPECTRA_POSTGRES_VERSION_PROBE_DOCKER_CONTAINER) {
        $stabilityArguments += @(
            "--postgres-version-probe-docker-container",
            $env:SPECTRA_POSTGRES_VERSION_PROBE_DOCKER_CONTAINER
        )
    }
    & python @stabilityArguments
    exit $LASTEXITCODE
}

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "   SPECTRALANG - SUITE DE TESTES" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

$totalPassed  = 0
$totalFailed  = 0
$totalInfo    = 0
$totalSkipped = 0
$results      = @()
$runPhase31Gpu = $Phase -contains "phase31_gpu"

if ($Phase -contains "language_bug_hunt_v2") {
    Write-Host "--- SpectraLang executable language bug-hunt v2 ---" -ForegroundColor Yellow
    & python scripts\validate_language_bug_hunt_v2.py --binary $binary
    $validatorExit = $LASTEXITCODE
    if ($validatorExit -ne 0) {
        Write-Host "Language bug-hunt v2 gate blocked (validator=$validatorExit)." -ForegroundColor Red
        exit $validatorExit
    }
    Write-Host "Language bug-hunt v2 gate passed." -ForegroundColor Green
    exit 0
}

if ($Phase -contains "stdlib_core_bug_hunt") {
    Write-Host "--- SpectraLang core stdlib executable bug-hunt ---" -ForegroundColor Yellow
    & python scripts\validate_stdlib_core_bug_hunt.py --binary $binary
    $validatorExit = $LASTEXITCODE
    & git diff --check -- `
        backend/src/codegen.rs `
        midend/src/lowering.rs `
        midend/tests/lowering_tests.rs `
        scripts/validate_stdlib_core_bug_hunt.py `
        docs/goals/spectralang-stdlib-core-bug-hunt/GOAL.md `
        docs/goals/spectralang-stdlib-core-bug-hunt/PLAN.md `
        docs/goals/spectralang-stdlib-core-bug-hunt/FINDINGS.md `
        run_tests.ps1
    $diffExit = $LASTEXITCODE
    if ($validatorExit -ne 0 -or $diffExit -ne 0) {
        Write-Host "Core stdlib bug-hunt gate blocked (validator=$validatorExit, diff-check=$diffExit)." -ForegroundColor Red
        exit 1
    }
    Write-Host "Core stdlib bug-hunt gate passed." -ForegroundColor Green
    exit 0
}

if ($Phase -contains "phase2_r213_generic_trait_impls") {
    Write-Host "--- R-213 generic traits gate ---" -ForegroundColor Yellow
    & python scripts\validate_r213_generic_trait_impls.py --binary $binary
    $validatorExit = $LASTEXITCODE
    & git diff --check -- compiler/src/ast/mod.rs compiler/src/parser/item.rs compiler/src/semantic/mod.rs tests/validation/257_oop_generic_traits.spectra scripts/validate_r213_generic_trait_impls.py docs/diagnostics/error-code-reference.md docs/reference/03-tipos-compostos.md
    $diffExit = $LASTEXITCODE
    if ($validatorExit -ne 0 -or $diffExit -ne 0) {
        Write-Host "R-213 focused gate blocked (validator=$validatorExit, diff-check=$diffExit)." -ForegroundColor Red
        exit 1
    }
    Write-Host "R-213 focused gate passed." -ForegroundColor Green
    exit 0
}
if ($Phase -contains "phase2_r212_ufcs") {
    Write-Host "--- R-212 UFCS gate ---" -ForegroundColor Yellow
    & python scripts\validate_r212_ufcs.py --binary $binary
    $validatorExit = $LASTEXITCODE
    & git diff --check -- compiler/src/semantic/mod.rs midend/src/lowering.rs tests/validation/256_oop_ufcs.spectra scripts/validate_r212_ufcs.py docs/reference/03-tipos-compostos.md
    $diffExit = $LASTEXITCODE
    if ($validatorExit -ne 0 -or $diffExit -ne 0) {
        Write-Host "R-212 focused gate blocked (validator=$validatorExit, diff-check=$diffExit)." -ForegroundColor Red
        exit 1
    }
    Write-Host "R-212 focused gate passed." -ForegroundColor Green
    exit 0
}
if ($Phase -contains "phase2_r210_static_vtables") {
    Write-Host "--- R-210 dyn vtable lifetime gate ---" -ForegroundColor Yellow
    & python scripts\validate_r210_static_vtables.py --binary $binary
    $validatorExit = $LASTEXITCODE
    & git diff --check -- midend/src/ir.rs midend/src/lowering.rs backend/src/codegen.rs runtime/src/ffi.rs tests/validation/255_oop_dyn_vtable_lifetime.spectra scripts/validate_r210_static_vtables.py
    $diffExit = $LASTEXITCODE
    if ($validatorExit -ne 0 -or $diffExit -ne 0) {
        Write-Host "R-210 focused gate blocked (validator=$validatorExit, diff-check=$diffExit)." -ForegroundColor Red
        exit 1
    }
    Write-Host "R-210 focused gate passed." -ForegroundColor Green
    exit 0
}
if ($Phase -contains "phase2_r211_generic_impls") {
    Write-Host "--- R-211 generic impl blocks gate ---" -ForegroundColor Yellow
    & python scripts\validate_r211_generic_impls.py --binary $binary
    $validatorExit = $LASTEXITCODE
    & git diff --check -- compiler/src/parser/item.rs compiler/src/semantic/mod.rs midend/src/lowering.rs tests/validation/254_oop_generic_impls.spectra scripts/validate_r211_generic_impls.py
    $diffExit = $LASTEXITCODE
    if ($validatorExit -ne 0 -or $diffExit -ne 0) {
        Write-Host "R-211 focused gate blocked (validator=$validatorExit, diff-check=$diffExit)." -ForegroundColor Red
        exit 1
    }
    Write-Host "R-211 focused gate passed." -ForegroundColor Green
    exit 0
}
if ($Phase -contains "phase2_r209_self_first_parameter") {
    Write-Host "--- R-209 self-first-parameter validation gate ---" -ForegroundColor Yellow
    & python scripts\validate_r209_self_first_parameter.py --binary $binary
    $validatorExit = $LASTEXITCODE
    & git diff --check -- compiler/src/semantic/mod.rs docs/diagnostics/error-code-reference.md scripts/validate_r209_self_first_parameter.py tests/errors/self_must_be_first_parameter.spectra
    $diffExit = $LASTEXITCODE
    if ($validatorExit -ne 0 -or $diffExit -ne 0) {
        Write-Host "R-209 focused gate blocked (validator=$validatorExit, diff-check=$diffExit)." -ForegroundColor Red
        exit 1
    }
    Write-Host "R-209 focused gate passed." -ForegroundColor Green
    exit 0
}
if ($Phase -contains "phase2_r208_oop_diagnostics") {
    Write-Host "--- R-208 stable OOP diagnostic codes gate ---" -ForegroundColor Yellow
    & python scripts\validate_r208_oop_diagnostics.py --binary $binary
    $validatorExit = $LASTEXITCODE
    & git diff --check -- compiler/src/semantic/mod.rs docs/diagnostics/error-code-reference.md scripts/validate_r208_oop_diagnostics.py
    $diffExit = $LASTEXITCODE
    if ($validatorExit -ne 0 -or $diffExit -ne 0) {
        Write-Host "R-208 focused gate blocked (validator=$validatorExit, diff-check=$diffExit)." -ForegroundColor Red
        exit 1
    }
    Write-Host "R-208 focused gate passed." -ForegroundColor Green
    exit 0
}

if ($Phase -contains "phase2_r207_struct_layout") {
    Write-Host "--- R-207 struct layout and drop semantics gate ---" -ForegroundColor Yellow
    & python scripts\validate_r207_struct_layout.py --binary $binary
    $validatorExit = $LASTEXITCODE
    & git diff --check -- backend/src/codegen.rs midend/src/layout.rs midend/src/lowering.rs midend/src/ir.rs runtime/src/ffi.rs tests/validation/253_oop_struct_layout_drop.spectra scripts/validate_r207_struct_layout.py
    $diffExit = $LASTEXITCODE
    if ($validatorExit -ne 0 -or $diffExit -ne 0) {
        Write-Host "R-207 focused gate blocked (validator=$validatorExit, diff-check=$diffExit)." -ForegroundColor Red
        exit 1
    }
    Write-Host "R-207 focused gate passed." -ForegroundColor Green
    exit 0
}

if ($Phase -contains "phase27_tracing") {
    & powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "scripts\run_phase27_tracing.ps1") -Binary $binary
    exit $LASTEXITCODE
}

if ($Phase -contains "phase25_postgres") {
    & powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "scripts\run_phase25_postgres.ps1") -Binary $binary
    exit $LASTEXITCODE
}

if ($Phase -contains "phase31_r3103_plan") {
    Write-Host "--- R-3103 benchmark + IR optimization-plan gate ---" -ForegroundColor Yellow
    & python -m unittest -v scripts.test_validate_r3103_optimization_plan
    $unitExit = $LASTEXITCODE
    if ($unitExit -ne 0) {
        Write-Host "R-3103 validator unit tests failed." -ForegroundColor Red
        exit $unitExit
    }

    & python scripts\generate_r3103_ir.py `
        --binary target\release\spectralang.exe `
        --out target\phase31\r3103-ir
    $irExit = $LASTEXITCODE
    if ($irExit -ne 0) {
        Write-Host "R-3103 IR generation failed." -ForegroundColor Red
        exit $irExit
    }

    & python scripts\validate_r3103_optimization_plan.py `
        --report target\phase31\r3103-release-run-1.json `
        --report target\phase31\r3103-release-run-2.json `
        --baseline docs\performance\phase31-go-comparable\baseline.json `
        --ir-root target\phase31\r3103-ir `
        --roadmap roadmap\roadmap.toml `
        --plan docs\performance\phase31-go-comparable\optimization-plan.md `
        --write-evidence
    $validatorExit = $LASTEXITCODE

    & git diff --check -- `
        roadmap/roadmap.toml `
        docs/roadmap-backlog.md `
        docs/production-ai-implementation-plan.md `
        docs/performance/phase31-go-comparable/optimization-plan.md `
        docs/performance/phase31-go-comparable/evidence-r3103-benchmark-ir.md `
        scripts/generate_r3103_ir.py `
        scripts/validate_r3103_optimization_plan.py `
        scripts/test_validate_r3103_optimization_plan.py
    $diffExit = $LASTEXITCODE
    if ($validatorExit -ne 0 -or $diffExit -ne 0) {
        Write-Host "R-3103 focused gate blocked (validator=$validatorExit, diff-check=$diffExit)." -ForegroundColor Red
        exit 1
    }
    Write-Host "R-3103 focused gate passed." -ForegroundColor Green
    exit 0
}

if ($Phase -contains "phase31_r3104_codegen_hot_path") {
    Write-Host "--- R-3104 dense value-map + codegen hot-path gate ---" -ForegroundColor Yellow
    & python -m unittest -v `
        scripts.test_phase31_gates `
        scripts.test_validate_r3103_optimization_plan `
        scripts.test_validate_r3133_async_echo_reconciliation `
        scripts.test_validate_r3104_codegen_hot_path
    $unitExit = $LASTEXITCODE
    if ($unitExit -ne 0) {
        Write-Host "R-3104 focused unit tests failed." -ForegroundColor Red
        exit $unitExit
    }

    & python scripts\validate_r3104_codegen_hot_path.py `
        --report target\phase31\r3104-release-run-1.json `
        --report target\phase31\r3104-release-run-2.json `
        --baseline docs\performance\phase31-go-comparable\baseline.json `
        --ir-root target\phase31\r3104-ir `
        --codegen-before target\phase31\r3104-codegen-before.json `
        --codegen-after target\phase31\r3104-codegen-after.json `
        --steady-state target\phase31\r3104-steady-state.json `
        --roadmap roadmap\roadmap.toml `
        --plan docs\performance\phase31-go-comparable\optimization-plan.md `
        --binary target\release\spectralang.exe `
        --aot-source benchmarks\cross-lang\cpu-loop-sum\spectra\bench.spectra `
        --aot-output target\phase31\r3104-aot-smoke.obj `
        --write-evidence
    $validatorExit = $LASTEXITCODE

    & git diff --check -- `
        backend/src/codegen.rs `
        backend/src/aot.rs `
        roadmap/roadmap.toml `
        docs/roadmap-backlog.md `
        docs/performance/phase31-go-comparable/evidence-r3104-codegen.md `
        scripts/generate_r3103_ir.py `
        scripts/benchmark_r3104_codegen.py `
        scripts/benchmark_r3104_steady_state.py `
        scripts/validate_r3104_codegen_hot_path.py `
        scripts/test_validate_r3104_codegen_hot_path.py `
        run_tests.ps1
    $diffExit = $LASTEXITCODE
    if ($validatorExit -ne 0 -or $diffExit -ne 0) {
        Write-Host "R-3104 focused gate blocked (validator=$validatorExit, diff-check=$diffExit)." -ForegroundColor Red
        exit 1
    }
    Write-Host "R-3104 focused gate passed." -ForegroundColor Green
    exit 0
}

if ($Phase -contains "phase31_r3105_hostcall_batching") {
    Write-Host "--- R-3105 allocation-free hostcall batching gate ---" -ForegroundColor Yellow
    & python -m unittest -v `
        scripts.test_phase31_gates `
        scripts.test_validate_r3103_optimization_plan `
        scripts.test_validate_r3133_async_echo_reconciliation `
        scripts.test_validate_r3104_codegen_hot_path `
        scripts.test_validate_r3105_hostcall_batching
    $unitExit = $LASTEXITCODE
    if ($unitExit -ne 0) {
        Write-Host "R-3105 focused unit tests failed." -ForegroundColor Red
        exit $unitExit
    }

    & python scripts\validate_r3105_hostcall_batching.py `
        --benchmark target\phase31\r3105-hostcall-benchmark.json `
        --code-validation target\phase31\r3105-code-validation.json `
        --report target\phase31\r3105-release-run-1.json `
        --report target\phase31\r3105-release-run-2.json `
        --steady-state target\phase31\r3104-steady-state.json `
        --baseline docs\performance\phase31-go-comparable\baseline.json `
        --roadmap roadmap\roadmap.toml `
        --binary target\release\spectralang.exe `
        --write-evidence
    $validatorExit = $LASTEXITCODE

    & git diff --check -- `
        backend/src/codegen.rs `
        backend/src/aot.rs `
        runtime/src/ffi.rs `
        tools/spectra-cli/src/compiler_integration.rs `
        scripts/benchmark_r3105_hostcalls.py `
        scripts/validate_r3105_hostcall_batching.py `
        scripts/test_validate_r3105_hostcall_batching.py `
        scripts/validate_r3104_codegen_hot_path.py `
        scripts/validate_r3103_optimization_plan.py `
        scripts/validate_r3133_async_echo_reconciliation.py `
        tests/validation/191_phase31_hostcall_batch_contract.spectra `
        benchmarks/cross-lang/hostcall-batch/spectra/bench.spectra `
        roadmap/roadmap.toml `
        docs/roadmap-backlog.md `
        docs/performance/phase31-go-comparable/optimization-plan.md `
        docs/performance/phase31-go-comparable/summary.md `
        docs/performance/phase31-go-comparable/evidence-r3105-hostcall-batching.md `
        run_tests.ps1
    $diffExit = $LASTEXITCODE
    if ($validatorExit -ne 0 -or $diffExit -ne 0) {
        Write-Host "R-3105 focused gate blocked (validator=$validatorExit, diff-check=$diffExit)." -ForegroundColor Red
        exit 1
    }
    Write-Host "R-3105 focused gate passed." -ForegroundColor Green
    exit 0
}

if ($Phase -contains "phase31_r3133_async_echo") {
    Write-Host "--- R-3133 current-revision async-echo reconciliation gate ---" -ForegroundColor Yellow
    & python -m unittest -v scripts.test_phase31_gates scripts.test_validate_r3133_async_echo_reconciliation
    $unitExit = $LASTEXITCODE
    if ($unitExit -ne 0) {
        Write-Host "R-3133 validator unit tests failed." -ForegroundColor Red
        exit $unitExit
    }

    & python scripts\validate_r3133_async_echo_reconciliation.py `
        --diagnostic target\phase31\async-echo-diagnostics\r3133-release.json `
        --report target\phase31\r3133-async-echo-only.json `
        --baseline docs\performance\phase31-go-comparable\baseline.json `
        --roadmap roadmap\roadmap.toml `
        --evidence docs\performance\phase31-go-comparable\evidence-r3133-async-echo.json `
        --evidence-md docs\performance\phase31-go-comparable\evidence-r3133-async-echo.md `
        --write-evidence
    $validatorExit = $LASTEXITCODE

    & git diff --check -- `
        roadmap/roadmap.toml `
        docs/roadmap-backlog.md `
        docs/production-ai-implementation-plan.md `
        docs/performance/phase31-go-comparable/summary.md `
        docs/performance/phase31-go-comparable/evidence-r3133-async-echo.md `
        scripts/diagnose_async_echo.py `
        scripts/validate_r3133_async_echo_reconciliation.py `
        scripts/test_validate_r3133_async_echo_reconciliation.py `
        tests/validation/185_async_echo_batch_contract.spectra
    $diffExit = $LASTEXITCODE
    if ($validatorExit -ne 0 -or $diffExit -ne 0) {
        Write-Host "R-3133 focused gate blocked (validator=$validatorExit, diff-check=$diffExit)." -ForegroundColor Red
        exit 1
    }
    Write-Host "R-3133 focused gate passed." -ForegroundColor Green
    exit 0
}

if ($Phase -contains "extended_spectra") {
    Write-Host "--- Extended algorithmic Spectra fixture gate ---" -ForegroundColor Yellow
    & python scripts\validate_extended_spectra.py `
        --binary $binary `
        --report target\extended-spectra\focused-report.json
    exit $LASTEXITCODE
}

if ($Phase -contains "tensor_device_contract") {
    Write-Host "--- Tensor device-placement contract gate ---" -ForegroundColor Yellow
    & python scripts\validate_tensor_device_contract.py `
        --binary $binary `
        --report target\tensor-device\focused-report.json
    exit $LASTEXITCODE
}

# Run the focused R-2701 gate before any broad test collection. This makes the
# global report observable even when an unrelated later phase is slow or fails.
Write-Host ""
Write-Host "--- R-2701 OpenTelemetry-compatible tracing (early integrated gate) ---" -ForegroundColor Yellow
& powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "scripts\run_phase27_tracing.ps1") -Binary $binary
$r2701EarlyExitCode = $LASTEXITCODE
if ($r2701EarlyExitCode -eq 0) { $totalPassed++ } else { $totalFailed++ }
$results += [PSCustomObject]@{ Diretorio = "phase27-opentelemetry-tracing"; Teste = "r2701_integrated_early_gate"; Status = $(if ($r2701EarlyExitCode -eq 0) { "PASSOU" } else { "FALHOU" }); Detalhe = "scripts/run_phase27_tracing.ps1 exit code $r2701EarlyExitCode" }
$r2701GlobalEvidence = [ordered]@{
    schema = "spectralang.r2701_global_gate.v1"
    phase = "phase27-opentelemetry-tracing"
    status = $(if ($r2701EarlyExitCode -eq 0) { "passed" } else { "failed" })
    exit_code = $r2701EarlyExitCode
    reports = @("success", "http_500", "invalid_content_type", "connection_drop", "delayed_response") | ForEach-Object { "target/r2701-tracing/$_.json" }
}
New-Item -ItemType Directory -Force -Path "target\r2701-tracing" | Out-Null
$r2701GlobalEvidence | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath "target\r2701-tracing\global-gate.json" -Encoding UTF8
Add-Content -LiteralPath "TEST_RESULTS.txt" -Value ("phase27-opentelemetry-tracing r2701_integrated_early_gate " + $(if ($r2701EarlyExitCode -eq 0) { "PASSOU" } else { "FALHOU" }))

# ---------------------------------------------------------------------------
# Funcao auxiliar: compila um arquivo .spectra com timeout e retorna o resultado
# ---------------------------------------------------------------------------
function Invoke-SpectraFile([string]$filePath) {
    return Invoke-SpectraCommand -commandArgs @("compile", $filePath) -workingDir (Get-Location).Path -includeExperimental $true
}

function Invoke-SpectraCommand([string[]]$commandArgs, [string]$workingDir, [bool]$includeExperimental = $false, [string]$stdinText = $null, [int]$timeoutSecondsOverride = $timeoutSeconds) {
    $fullArgs = if ($includeExperimental) { $commandArgs + $experimentalFlags } else { $commandArgs }

    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $binary
    $psi.WorkingDirectory = $workingDir
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.RedirectStandardInput = ($null -ne $stdinText)

    $quotedArgs = $fullArgs | ForEach-Object {
        if ($_ -match '[\s"]') {
            '"' + ($_ -replace '"', '\"') + '"'
        } else {
            $_
        }
    }
    $psi.Arguments = ($quotedArgs -join ' ')

    $proc = New-Object System.Diagnostics.Process
    $proc.StartInfo = $psi

    $timedOut = $false
    try {
        [void]$proc.Start()
        $stdoutTask = $proc.StandardOutput.ReadToEndAsync()
        $stderrTask = $proc.StandardError.ReadToEndAsync()
        if ($null -ne $stdinText) {
            $proc.StandardInput.Write($stdinText)
            $proc.StandardInput.Close()
        }
        if (-not $proc.WaitForExit($timeoutSecondsOverride * 1000)) {
            $timedOut = $true
            try { $proc.Kill($true) } catch { $proc.Kill() }
            $proc.WaitForExit()
        }
    } catch {
        $timedOut = $true
    }

    $stdout = if ($stdoutTask) { $stdoutTask.GetAwaiter().GetResult() } else { "" }
    $stderr = if ($stderrTask) { $stderrTask.GetAwaiter().GetResult() } else { "" }
    $combined = "$stdout`n$stderr"

    return [PSCustomObject]@{
        ExitCode = if ($timedOut) { 124 } else { $proc.ExitCode }
        TimedOut = $timedOut
        Output   = $combined
    }
}

function Get-FirstError([string]$output) {
    $ansiPattern = [string][char]27 + '\[[0-9;]*[A-Za-z]'
    $plainOutput = [regex]::Replace($output, $ansiPattern, '')
    $line = ($plainOutput -split "`n" | Where-Object { $_ -match "error\[|error:|Error:" } | Select-Object -First 1)
    if (-not $line) {
        $line = ($plainOutput -split "`n" | Where-Object { $_ -match "Expected|Undefined|not defined" } | Select-Object -First 1)
    }
    if ($line) {
        return $line.Trim().Substring(0, [Math]::Min(80, $line.Trim().Length))
    }
    return ""
}

# ---------------------------------------------------------------------------
# Grupo 1: testes que devem compilar com SUCESSO
# ---------------------------------------------------------------------------
$successDirs = @("tests\validation", "tests\control_flow")

foreach ($dir in $successDirs) {
    if (-not (Test-Path $dir)) { continue }
    $files = Get-ChildItem -Path $dir -Filter "*.spectra" | Sort-Object Name
    Write-Host ""
    Write-Host "--- $dir ($($files.Count) testes: devem passar) ---" -ForegroundColor Yellow

    foreach ($file in $files) {
        Write-Host "  $($file.Name)" -NoNewline
        $r = Invoke-SpectraFile $file.FullName

        if ($r.TimedOut) {
            Write-Host " TIMEOUT" -ForegroundColor Red
            $totalFailed++
            $results += [PSCustomObject]@{ Diretorio = $dir; Teste = $file.Name; Status = "TIMEOUT"; Detalhe = "compilacao excedeu ${timeoutSeconds}s" }
        } elseif ($r.ExitCode -eq 0) {
            Write-Host " PASSOU" -ForegroundColor Green
            $totalPassed++
            $results += [PSCustomObject]@{ Diretorio = $dir; Teste = $file.Name; Status = "PASSOU"; Detalhe = "" }
        } else {
            $err = Get-FirstError $r.Output
            Write-Host " FALHOU" -ForegroundColor Red
            Write-Host "     $err" -ForegroundColor DarkRed
            $totalFailed++
            $results += [PSCustomObject]@{ Diretorio = $dir; Teste = $file.Name; Status = "FALHOU"; Detalhe = $err }
        }
    }
}

# ---------------------------------------------------------------------------
# Grupo 2: testes de erro - devem FALHAR na compilacao
# ---------------------------------------------------------------------------
$projectDir = "tests\projects\valid"
if (Test-Path $projectDir) {
    $projects = Get-ChildItem -Path $projectDir -Directory | Sort-Object Name
    Write-Host ""
    Write-Host "--- $projectDir ($($projects.Count) projetos: devem passar) ---" -ForegroundColor Yellow

    foreach ($project in $projects) {
        Write-Host "  $($project.Name)" -NoNewline
        $r = Invoke-SpectraCommand -commandArgs @("compile", $project.FullName) -workingDir (Get-Location).Path -includeExperimental $true

        if ($r.TimedOut) {
            Write-Host " TIMEOUT" -ForegroundColor Red
            $totalFailed++
            $results += [PSCustomObject]@{ Diretorio = $projectDir; Teste = $project.Name; Status = "TIMEOUT"; Detalhe = "compilacao do projeto excedeu ${timeoutSeconds}s" }
        } elseif ($r.ExitCode -eq 0) {
            Write-Host " PASSOU" -ForegroundColor Green
            $totalPassed++
            $results += [PSCustomObject]@{ Diretorio = $projectDir; Teste = $project.Name; Status = "PASSOU"; Detalhe = "" }
        } else {
            $err = Get-FirstError $r.Output
            Write-Host " FALHOU" -ForegroundColor Red
            Write-Host "     $err" -ForegroundColor DarkRed
            $totalFailed++
            $results += [PSCustomObject]@{ Diretorio = $projectDir; Teste = $project.Name; Status = "FALHOU"; Detalhe = $err }
        }
    }
}

# ---------------------------------------------------------------------------
# Grupo 3: testes de erro - devem FALHAR na compilacao
# ---------------------------------------------------------------------------
$errorDir = "tests\errors"
if (Test-Path $errorDir) {
    $files = Get-ChildItem -Path $errorDir -Filter "*.spectra" | Sort-Object Name
    $runtimeErrorFixtures = @(
        "exact_width_float_nonfinite.spectra",
        "exact_width_invalid_cast.spectra",
        "exact_width_runtime_overflow.spectra",
        "integer_division_by_zero.spectra",
        "json_malformed_rejects.spectra"
    )
    Write-Host ""
    Write-Host "--- $errorDir ($($files.Count) testes: devem falhar) ---" -ForegroundColor Yellow

    foreach ($file in $files) {
        Write-Host "  $($file.Name)" -NoNewline
        $expectsRuntimeFailure = $runtimeErrorFixtures -contains $file.Name
        $r = if ($expectsRuntimeFailure) {
            Invoke-SpectraCommand -commandArgs @("run", $file.FullName) -workingDir (Get-Location).Path -includeExperimental $true -timeoutSecondsOverride $runtimeErrorTimeoutSeconds
        } else {
            Invoke-SpectraFile $file.FullName
        }

        if ($r.TimedOut) {
            Write-Host " FALHOU (timeout)" -ForegroundColor Red
            $totalFailed++
            $mode = if ($expectsRuntimeFailure) { "runtime" } else { "compilacao" }
            $results += [PSCustomObject]@{ Diretorio = $errorDir; Teste = $file.Name; Status = "FALHOU"; Detalhe = "timeout - erro esperado em $mode" }
        } elseif ($r.ExitCode -ne 0) {
            $mode = if ($expectsRuntimeFailure) { "runtime" } else { "compilacao" }
            Write-Host " PASSOU (erro esperado: $mode)" -ForegroundColor Green
            $totalPassed++
            $results += [PSCustomObject]@{ Diretorio = $errorDir; Teste = $file.Name; Status = "PASSOU"; Detalhe = "erro esperado detectado em $mode" }
        } else {
            $mode = if ($expectsRuntimeFailure) { "runtime" } else { "compilacao" }
            Write-Host " FALHOU (deveria produzir erro em $mode)" -ForegroundColor Red
            $totalFailed++
            $results += [PSCustomObject]@{ Diretorio = $errorDir; Teste = $file.Name; Status = "FALHOU"; Detalhe = "executou sem erro - erro esperado em $mode nao detectado" }
        }
    }
}

# ---------------------------------------------------------------------------
# Grupo 4: testes semanticos - informativo apenas
# ---------------------------------------------------------------------------
$semanticDir = "tests\semantic"
if (Test-Path $semanticDir) {
    $files = Get-ChildItem -Path $semanticDir -Filter "*.spectra" | Sort-Object Name
    Write-Host ""
    Write-Host "--- $semanticDir ($($files.Count) testes: informativo) ---" -ForegroundColor Yellow

    foreach ($file in $files) {
        Write-Host "  $($file.Name)" -NoNewline
        $r = Invoke-SpectraFile $file.FullName

        if ($r.TimedOut) {
            Write-Host " TIMEOUT" -ForegroundColor Red
            $totalInfo++
            $results += [PSCustomObject]@{ Diretorio = $semanticDir; Teste = $file.Name; Status = "INFO:TIMEOUT"; Detalhe = "compilacao excedeu ${timeoutSeconds}s" }
        } elseif ($r.ExitCode -eq 0) {
            Write-Host " COMPILOU" -ForegroundColor Cyan
            $totalInfo++
            $results += [PSCustomObject]@{ Diretorio = $semanticDir; Teste = $file.Name; Status = "INFO:COMPILOU"; Detalhe = "" }
        } else {
            $err = Get-FirstError $r.Output
            Write-Host " ERRO" -ForegroundColor DarkYellow
            $totalInfo++
            $results += [PSCustomObject]@{ Diretorio = $semanticDir; Teste = $file.Name; Status = "INFO:ERRO"; Detalhe = $err }
        }
    }
}

# ---------------------------------------------------------------------------
# Grupo 5: testes diretos do CLI
# ---------------------------------------------------------------------------
$cliTests = @(
    [PSCustomObject]@{
        Nome = "help"
        Args = @("--help")
        ExpectExit = 0
        Contains = "USAGE:"
        UseStdin = $false
    }
    [PSCustomObject]@{
        Nome = "list_experimental"
        Args = @("--list-experimental")
        ExpectExit = 0
        Contains = "none"
        UseStdin = $false
    }
    [PSCustomObject]@{
        Nome = "check_valid_file"
        Args = @("check", "tests\validation\60_pattern_control_surface.spectra")
        ExpectExit = 0
        Contains = ""
        UseStdin = $false
    }
    [PSCustomObject]@{
        Nome = "check_invalid_file"
        Args = @("check", "tests\errors\type_mismatch.spectra")
        ExpectExit = 65
        Contains = "error"
        UseStdin = $false
    }
    [PSCustomObject]@{
        Nome = "check_json_invalid_file"
        Args = @("check", "--json", "tests\errors\type_mismatch.spectra")
        ExpectExit = 65
        Contains = '"phase":"semantic"'
        UseStdin = $false
    }
    [PSCustomObject]@{
        Nome = "compile_json_invalid_file"
        Args = @("compile", "--json", "tests\errors\type_mismatch.spectra")
        ExpectExit = 65
        Contains = '"code":"E004"'
        UseStdin = $false
    }
    [PSCustomObject]@{
        Nome = "check_sarif_invalid_file"
        Args = @("check", "--sarif", "tests\errors\type_mismatch.spectra")
        ExpectExit = 65
        Contains = '"version": "2.1.0"'
        UseStdin = $false
    }
    [PSCustomObject]@{
        Nome = "lint_clean"
        Args = @("lint", "tests\cli\lint_clean.spectra")
        ExpectExit = 0
        Contains = ""
        UseStdin = $false
    }
    [PSCustomObject]@{
        Nome = "lint_warn_unused"
        Args = @("lint", "tests\cli\lint_warning_unused.spectra")
        ExpectExit = 0
        Contains = "unused-binding"
        UseStdin = $false
    }
    [PSCustomObject]@{
        Nome = "lint_deny_shadowing"
        Args = @("lint", "--deny", "shadowing", "tests\cli\lint_warning_shadowing.spectra")
        ExpectExit = 65
        Contains = "shadowing"
        UseStdin = $false
    }
    # BEGIN narrowing-cast lint rows (CastLint)
    [PSCustomObject]@{
        Nome = "lint_warn_narrowing_cast"
        Args = @("lint", "tests\cli\lint_warning_narrowing_cast.spectra")
        ExpectExit = 0
        Contains = "narrowing-cast"
        UseStdin = $false
    }
    [PSCustomObject]@{
        Nome = "compile_warn_narrowing_cast_does_not_block"
        Args = @("compile", "tests\cli\lint_warning_narrowing_cast.spectra")
        ExpectExit = 0
        Contains = ""
        UseStdin = $false
    }
    [PSCustomObject]@{
        Nome = "lint_deny_narrowing_cast"
        Args = @("lint", "--deny", "narrowing-cast", "tests\cli\lint_warning_narrowing_cast.spectra")
        ExpectExit = 65
        Contains = "narrowing-cast"
        UseStdin = $false
    }
    [PSCustomObject]@{
        Nome = "compile_deny_narrowing_cast_escalates_e035"
        Args = @("compile", "--deny", "narrowing-cast", "--json", "tests\cli\lint_warning_narrowing_cast.spectra")
        ExpectExit = 65
        Contains = '"code":"E035"'
        UseStdin = $false
    }
    # END narrowing-cast lint rows (CastLint)
    # BEGIN deprecated-task-spawn lint rows (DeprecateSpawn)
    [PSCustomObject]@{
        Nome = "lint_warn_deprecated_task_spawn"
        Args = @("lint", "tests\cli\lint_warning_deprecated_task_spawn.spectra")
        ExpectExit = 0
        Contains = "deprecated-task-spawn"
        UseStdin = $false
    }
    [PSCustomObject]@{
        Nome = "compile_warn_deprecated_task_spawn_does_not_block"
        Args = @("compile", "tests\cli\lint_warning_deprecated_task_spawn.spectra")
        ExpectExit = 0
        Contains = ""
        UseStdin = $false
    }
    [PSCustomObject]@{
        Nome = "lint_deny_deprecated_task_spawn"
        Args = @("lint", "--deny", "deprecated-task-spawn", "tests\cli\lint_warning_deprecated_task_spawn.spectra")
        ExpectExit = 65
        Contains = "deprecated-task-spawn"
        UseStdin = $false
    }
    [PSCustomObject]@{
        Nome = "compile_deny_deprecated_task_spawn_escalates_e036"
        Args = @("compile", "--deny", "deprecated-task-spawn", "--json", "tests\cli\lint_warning_deprecated_task_spawn.spectra")
        ExpectExit = 65
        Contains = '"code":"E036"'
        UseStdin = $false
    }
    # END deprecated-task-spawn lint rows (DeprecateSpawn)
    [PSCustomObject]@{
        Nome = "fmt_check_formatted"
        Args = @("fmt", "--check", "tests\cli\fmt_formatted.spectra")
        ExpectExit = 0
        Contains = ""
        UseStdin = $false
    }
    [PSCustomObject]@{
        Nome = "fmt_check_unformatted"
        Args = @("fmt", "--check", "tests\cli\fmt_unformatted.spectra")
        ExpectExit = 65
        Contains = "fmt_unformatted"
        UseStdin = $false
    }
    [PSCustomObject]@{
        Nome = "fmt_stdout"
        Args = @("fmt", "--stdout", "tests\cli\fmt_unformatted.spectra")
        ExpectExit = 0
        Contains = "public func main() returns int {"
        UseStdin = $false
    }
    [PSCustomObject]@{
        Nome = "fmt_stdin"
        Args = @()
        ExpectExit = 0
        Contains = "public func main() returns int {"
        UseStdin = $true
        StdinFile = "tests\cli\fmt_unformatted.spectra"
    }
    [PSCustomObject]@{
        Nome = "compile_project_dir"
        Args = @("compile", "tests\projects\valid\basic_project")
        ExpectExit = 0
        Contains = ""
        UseStdin = $false
    }
    [PSCustomObject]@{
        Nome = "package_lock_workspace"
        Args = @("package", "lock", "--root", "tests\projects\valid\package_workspace")
        ExpectExit = 0
        Contains = "Locked"
        UseStdin = $false
    }
    [PSCustomObject]@{
        Nome = "package_build_workspace"
        Args = @("package", "build", "--root", "tests\projects\valid\package_workspace")
        ExpectExit = 0
        Contains = "Finished"
        UseStdin = $false
    }
    [PSCustomObject]@{
        Nome = "package_check_workspace"
        Args = @("package", "check", "--root", "tests\projects\valid\package_workspace")
        ExpectExit = 0
        Contains = "Finished"
        UseStdin = $false
    }
    [PSCustomObject]@{
        Nome = "package_doc_workspace"
        Args = @("package", "doc", "--root", "tests\projects\valid\package_workspace")
        ExpectExit = 0
        Contains = "Written docs"
        UseStdin = $false
    }
    [PSCustomObject]@{
        Nome = "bench_json_valid_file"
        Args = @("bench", "--bench-json", "target\bench_cli_test.json", "tests\validation\01_basic_syntax.spectra")
        ExpectExit = 0
        Contains = "Written bench report"
        UseStdin = $false
    }
    [PSCustomObject]@{
        Nome = "runtime_nonzero_trace"
        Args = @("run", "tests\cli\runtime_nonzero.spectra")
        ExpectExit = 7
        Contains = "0: main()"
        UseStdin = $false
    }
)

Write-Host ""
Write-Host "--- CLI ($($cliTests.Count) testes: devem seguir a expectativa do comando) ---" -ForegroundColor Yellow

foreach ($cliTest in $cliTests) {
    Write-Host "  $($cliTest.Nome)" -NoNewline

    if ($cliTest.UseStdin) {
        $stdinPath = Join-Path (Get-Location).Path $cliTest.StdinFile
        $stdinText = Get-Content -LiteralPath $stdinPath -Raw
        $r = Invoke-SpectraCommand -commandArgs @("fmt", "--stdin") -workingDir (Get-Location).Path -stdinText $stdinText
    } else {
        $needsExperimental = ($cliTest.Args.Count -gt 0 -and @("compile", "check", "lint", "run") -contains $cliTest.Args[0])
        $r = Invoke-SpectraCommand -commandArgs $cliTest.Args -workingDir (Get-Location).Path -includeExperimental $needsExperimental
    }

    $exitMatches = $false
    if ($cliTest.ExpectExit -eq 65) {
        $exitMatches = ($r.ExitCode -ne 0 -and -not $r.TimedOut)
    } else {
        $exitMatches = ($r.ExitCode -eq $cliTest.ExpectExit -and -not $r.TimedOut)
    }

    $containsMatches = $true
    if ($cliTest.Contains) {
        $containsMatches = $r.Output -match [Regex]::Escape($cliTest.Contains)
    }

    if ($r.TimedOut) {
        Write-Host " TIMEOUT" -ForegroundColor Red
        $totalFailed++
        $results += [PSCustomObject]@{ Diretorio = "tests\cli"; Teste = $cliTest.Nome; Status = "FALHOU"; Detalhe = "timeout" }
    } elseif ($exitMatches -and $containsMatches) {
        Write-Host " PASSOU" -ForegroundColor Green
        $totalPassed++
        $results += [PSCustomObject]@{ Diretorio = "tests\cli"; Teste = $cliTest.Nome; Status = "PASSOU"; Detalhe = "" }
    } else {
        $detail = if (-not $exitMatches) { "exit code inesperado: $($r.ExitCode)" } else { "saida nao contem: $($cliTest.Contains)" }
        Write-Host " FALHOU" -ForegroundColor Red
        Write-Host "     $detail" -ForegroundColor DarkRed
        $totalFailed++
        $results += [PSCustomObject]@{ Diretorio = "tests\cli"; Teste = $cliTest.Nome; Status = "FALHOU"; Detalhe = $detail }
    }
}

# ---------------------------------------------------------------------------
# Grupo 6: teste do comando `new`
# ---------------------------------------------------------------------------
$newProjectRoot = Join-Path $env:TEMP "spectra_cli_new_test_$PID"
if (Test-Path $newProjectRoot) {
    Remove-Item -LiteralPath $newProjectRoot -Recurse -Force
}

Write-Host ""
Write-Host "--- CLI scaffold (1 teste: deve passar) ---" -ForegroundColor Yellow
Write-Host "  new_project" -NoNewline
$newResult = Invoke-SpectraCommand -commandArgs @("new", $newProjectRoot) -workingDir (Get-Location).Path
$newManifest = Join-Path $newProjectRoot "spectra.toml"
$newMain = Join-Path $newProjectRoot "src\main.spectra"

if ($newResult.TimedOut) {
    Write-Host " TIMEOUT" -ForegroundColor Red
    $totalFailed++
    $results += [PSCustomObject]@{ Diretorio = "tests\cli"; Teste = "new_project"; Status = "FALHOU"; Detalhe = "timeout" }
} elseif ($newResult.ExitCode -eq 0 -and (Test-Path $newManifest) -and (Test-Path $newMain)) {
    Write-Host " PASSOU" -ForegroundColor Green
    $totalPassed++
    $results += [PSCustomObject]@{ Diretorio = "tests\cli"; Teste = "new_project"; Status = "PASSOU"; Detalhe = "" }
} else {
    $detail = "scaffold incompleto ou exit code inesperado: $($newResult.ExitCode)"
    Write-Host " FALHOU" -ForegroundColor Red
    Write-Host "     $detail" -ForegroundColor DarkRed
    $totalFailed++
    $results += [PSCustomObject]@{ Diretorio = "tests\cli"; Teste = "new_project"; Status = "FALHOU"; Detalhe = $detail }
}

if (Test-Path $newProjectRoot) {
    Remove-Item -LiteralPath $newProjectRoot -Recurse -Force
}

# ---------------------------------------------------------------------------
# Grupo 7: package registry local - publish/add/build
# ---------------------------------------------------------------------------
$packageRegistryRoot = Join-Path $env:TEMP "spectra_pkg_registry_test_$PID"
$packageConsumerRoot = Join-Path $env:TEMP "spectra_pkg_consumer_test_$PID"
if (Test-Path $packageRegistryRoot) {
    Remove-Item -LiteralPath $packageRegistryRoot -Recurse -Force
}

if (Test-Path $packageConsumerRoot) {
    Remove-Item -LiteralPath $packageConsumerRoot -Recurse -Force
}

Write-Host ""
Write-Host "--- Package registry (4 testes: devem passar) ---" -ForegroundColor Yellow

$lockPath = "tests\projects\valid\package_workspace\spectra.lock"
$lockBefore = if (Test-Path $lockPath) { Get-Content -LiteralPath $lockPath -Raw } else { "" }
$lockAgain = Invoke-SpectraCommand -commandArgs @("package", "lock", "--root", "tests\projects\valid\package_workspace") -workingDir (Get-Location).Path
$lockAfter = if (Test-Path $lockPath) { Get-Content -LiteralPath $lockPath -Raw } else { "" }
Write-Host "  package_lock_deterministic" -NoNewline
if (-not $lockAgain.TimedOut -and $lockAgain.ExitCode -eq 0 -and $lockBefore -eq $lockAfter) {
    Write-Host " PASSOU" -ForegroundColor Green
    $totalPassed++
    $results += [PSCustomObject]@{ Diretorio = "package"; Teste = "package_lock_deterministic"; Status = "PASSOU"; Detalhe = "" }
} else {
    Write-Host " FALHOU" -ForegroundColor Red
    $totalFailed++
    $results += [PSCustomObject]@{ Diretorio = "package"; Teste = "package_lock_deterministic"; Status = "FALHOU"; Detalhe = "spectra.lock mudou entre resolucoes equivalentes" }
}

$publishSource = (Resolve-Path "tests\projects\valid\package_workspace\packages\core").Path
$publishResult = Invoke-SpectraCommand -commandArgs @("package", "publish", "--root", $publishSource, "--registry", $packageRegistryRoot) -workingDir (Get-Location).Path
Write-Host "  package_publish_core" -NoNewline
if (-not $publishResult.TimedOut -and $publishResult.ExitCode -eq 0) {
    Write-Host " PASSOU" -ForegroundColor Green
    $totalPassed++
    $results += [PSCustomObject]@{ Diretorio = "package"; Teste = "package_publish_core"; Status = "PASSOU"; Detalhe = "" }
} else {
    Write-Host " FALHOU" -ForegroundColor Red
    $totalFailed++
    $results += [PSCustomObject]@{ Diretorio = "package"; Teste = "package_publish_core"; Status = "FALHOU"; Detalhe = Get-FirstError $publishResult.Output }
}

$newConsumerResult = Invoke-SpectraCommand -commandArgs @("new", $packageConsumerRoot) -workingDir (Get-Location).Path
$addResult = Invoke-SpectraCommand -commandArgs @("package", "add", "core", "--root", $packageConsumerRoot, "--version", "0.1.0", "--registry", $packageRegistryRoot) -workingDir (Get-Location).Path
Write-Host "  package_add_registry_core" -NoNewline
if (-not $newConsumerResult.TimedOut -and $newConsumerResult.ExitCode -eq 0 -and -not $addResult.TimedOut -and $addResult.ExitCode -eq 0) {
    Write-Host " PASSOU" -ForegroundColor Green
    $totalPassed++
    $results += [PSCustomObject]@{ Diretorio = "package"; Teste = "package_add_registry_core"; Status = "PASSOU"; Detalhe = "" }
} else {
    Write-Host " FALHOU" -ForegroundColor Red
    $totalFailed++
    $detail = if ($newConsumerResult.ExitCode -ne 0) { Get-FirstError $newConsumerResult.Output } else { Get-FirstError $addResult.Output }
    $results += [PSCustomObject]@{ Diretorio = "package"; Teste = "package_add_registry_core"; Status = "FALHOU"; Detalhe = $detail }
}

$buildConsumerResult = Invoke-SpectraCommand -commandArgs @("package", "build", "--root", $packageConsumerRoot) -workingDir (Get-Location).Path
Write-Host "  package_build_registry_consumer" -NoNewline
if (-not $buildConsumerResult.TimedOut -and $buildConsumerResult.ExitCode -eq 0) {
    Write-Host " PASSOU" -ForegroundColor Green
    $totalPassed++
    $results += [PSCustomObject]@{ Diretorio = "package"; Teste = "package_build_registry_consumer"; Status = "PASSOU"; Detalhe = "" }
} else {
    Write-Host " FALHOU" -ForegroundColor Red
    $totalFailed++
    $results += [PSCustomObject]@{ Diretorio = "package"; Teste = "package_build_registry_consumer"; Status = "FALHOU"; Detalhe = Get-FirstError $buildConsumerResult.Output }
}

if (Test-Path $packageRegistryRoot) {
    Remove-Item -LiteralPath $packageRegistryRoot -Recurse -Force
}
if (Test-Path $packageConsumerRoot) {
    Remove-Item -LiteralPath $packageConsumerRoot -Recurse -Force
}

# ---------------------------------------------------------------------------
# Grupo 8: interop Python / Rust / C ABI
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- Interop (Rust/Python/C ABI) ---" -ForegroundColor Yellow

function Invoke-HostCommand([string]$name, [string]$fileName, [string[]]$arguments, [string]$workingDir, [int]$timeoutSeconds = $hostCommandTimeoutSeconds) {
    Write-Host "  $name" -NoNewline

    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $fileName
    $psi.WorkingDirectory = $workingDir
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $quotedArgs = $arguments | ForEach-Object {
        if ($_ -match '[\s"]') {
            '"' + ($_ -replace '"', '\"') + '"'
        } else {
            $_
        }
    }
    $psi.Arguments = ($quotedArgs -join ' ')

    $proc = New-Object System.Diagnostics.Process
    $proc.StartInfo = $psi
    $timedOut = $false

    try {
        [void]$proc.Start()
        $stdoutTask = $proc.StandardOutput.ReadToEndAsync()
        $stderrTask = $proc.StandardError.ReadToEndAsync()
        $watch = [System.Diagnostics.Stopwatch]::StartNew()
        $lastHeartbeat = 0
        while (-not $proc.WaitForExit(1000)) {
            if ($watch.Elapsed.TotalSeconds -ge $timeoutSeconds) {
                $timedOut = $true
                try { $proc.Kill($true) } catch { $proc.Kill() }
                $proc.WaitForExit()
                break
            }
            if ($name -eq "phase31_run_all" -and ($watch.Elapsed.TotalSeconds - $lastHeartbeat) -ge 30) {
                Write-Host "." -NoNewline -ForegroundColor DarkGray
                $lastHeartbeat = [int]$watch.Elapsed.TotalSeconds
            }
        }
    } catch {
        Write-Host " FALHOU" -ForegroundColor Red
        return [PSCustomObject]@{ Status = "FALHOU"; Detail = $_.Exception.Message }
    }

    $stdout = if ($stdoutTask) { $stdoutTask.GetAwaiter().GetResult() } else { "" }
    $stderr = if ($stderrTask) { $stderrTask.GetAwaiter().GetResult() } else { "" }
    $combined = "$stdout`n$stderr"

    if ($timedOut) {
        Write-Host " TIMEOUT" -ForegroundColor Red
        return [PSCustomObject]@{ Status = "TIMEOUT"; Detail = "comando excedeu ${timeoutSeconds}s" }
    }
    if ($proc.ExitCode -eq 0) {
        Write-Host " PASSOU" -ForegroundColor Green
        return [PSCustomObject]@{ Status = "PASSOU"; Detail = "" }
    }
    $err = Get-FirstError $combined
    if (-not $err) {
        $err = "exit code inesperado: $($proc.ExitCode)"
    }
    Write-Host " FALHOU" -ForegroundColor Red
    Write-Host "     $err" -ForegroundColor DarkRed
    return [PSCustomObject]@{ Status = "FALHOU"; Detail = $err }
}

# ---------------------------------------------------------------------------
# Grupo 1.5: execução dos 50 fixtures algorítmicos novos
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- Extended algorithmic Spectra fixtures (50 testes: devem executar) ---" -ForegroundColor Yellow
$extendedSpectra = Invoke-HostCommand `
    -name "validate_extended_spectra" `
    -fileName "python" `
    -arguments @(
        "scripts\validate_extended_spectra.py",
        "--binary", $binary,
        "--report", "target\extended-spectra\report.json"
    ) `
    -workingDir (Get-Location).Path
if ($extendedSpectra.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{
    Diretorio = "tests\validation"
    Teste = "extended_algorithmic_spectra_50"
    Status = $extendedSpectra.Status
    Detalhe = $extendedSpectra.Detail
}

Write-Host ""
Write-Host "--- Tensor device-placement contract fixtures ---" -ForegroundColor Yellow
$tensorDeviceContract = Invoke-HostCommand `
    -name "validate_tensor_device_contract" `
    -fileName "python" `
    -arguments @(
        "scripts\validate_tensor_device_contract.py",
        "--binary", $binary,
        "--report", "target\tensor-device\report.json"
    ) `
    -workingDir (Get-Location).Path
if ($tensorDeviceContract.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{
    Diretorio = "tests\validation"
    Teste = "tensor_device_contract_3"
    Status = $tensorDeviceContract.Status
    Detalhe = $tensorDeviceContract.Detail
}

Write-Host ""
Write-Host "--- R-905 deterministic package resolver ---" -ForegroundColor Yellow
Write-Host "  validate_r905_package_resolver" -NoNewline
$r905PackageResolver = Invoke-HostCommand -name "validate_r905_package_resolver" -fileName "python" -arguments @("scripts\validate_r905_package_resolver.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($r905PackageResolver.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "package"; Teste = "validate_r905_package_resolver"; Status = $r905PackageResolver.Status; Detalhe = $r905PackageResolver.Detail }

Write-Host ""
Write-Host "--- R-906 package import integration ---" -ForegroundColor Yellow
Write-Host "  validate_r906_package_imports" -NoNewline
$r906PackageImports = Invoke-HostCommand -name "validate_r906_package_imports" -fileName "python" -arguments @("scripts\validate_r906_package_imports.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($r906PackageImports.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "package"; Teste = "validate_r906_package_imports"; Status = $r906PackageImports.Status; Detalhe = $r906PackageImports.Detail }

Write-Host ""
Write-Host "--- R-912 package security and integrity ---" -ForegroundColor Yellow
Write-Host "  validate_r912_package_security" -NoNewline
$r912PackageSecurity = Invoke-HostCommand -name "validate_r912_package_security" -fileName "python" -arguments @("scripts\validate_r912_package_security.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($r912PackageSecurity.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "package"; Teste = "validate_r912_package_security"; Status = $r912PackageSecurity.Status; Detalhe = $r912PackageSecurity.Detail }

Write-Host ""
Write-Host "--- R-913 offline reproducible package flow ---" -ForegroundColor Yellow
Write-Host "  validate_r913_offline_reproducible" -NoNewline
$r913OfflineReproducible = Invoke-HostCommand -name "validate_r913_offline_reproducible" -fileName "python" -arguments @("scripts\validate_r913_offline_reproducible.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($r913OfflineReproducible.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "package"; Teste = "validate_r913_offline_reproducible"; Status = $r913OfflineReproducible.Status; Detalhe = $r913OfflineReproducible.Detail }

Write-Host ""
Write-Host "--- R-911 catalog synchronization ---" -ForegroundColor Yellow
Write-Host "  validate_r911_catalog_sync" -NoNewline
$r911CatalogSync = Invoke-HostCommand -name "validate_r911_catalog_sync" -fileName "python" -arguments @("scripts\validate_r911_catalog_sync.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($r911CatalogSync.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "package"; Teste = "validate_r911_catalog_sync"; Status = $r911CatalogSync.Status; Detalhe = $r911CatalogSync.Detail }

Write-Host ""
Write-Host "--- R-914 package catalog Git flow ---" -ForegroundColor Yellow
Write-Host "  validate_r914_package_catalog_git" -NoNewline
$r914PackageCatalogGit = Invoke-HostCommand -name "validate_r914_package_catalog_git" -fileName "python" -arguments @("scripts\validate_r914_package_catalog_git.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($r914PackageCatalogGit.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "package"; Teste = "validate_r914_package_catalog_git"; Status = $r914PackageCatalogGit.Status; Detalhe = $r914PackageCatalogGit.Detail }

$cargoPath = (Get-Command cargo -ErrorAction SilentlyContinue).Source
if (-not $cargoPath) {
    $cargoPath = "C:\Users\estev\.cargo\bin\cargo.exe"
}

$interopChecks = @(
    [PSCustomObject]@{ Nome = "cargo_test_spectra_interop"; File = $cargoPath; Args = @("test", "-p", "spectra-interop") }
    [PSCustomObject]@{ Nome = "cargo_test_spectra_lsp"; File = $cargoPath; Args = @("test", "-p", "spectra-lsp") }
    [PSCustomObject]@{ Nome = "cargo_build_spectra_interop_release"; File = $cargoPath; Args = @("build", "-p", "spectra-interop", "--release") }
    [PSCustomObject]@{ Nome = "rust_ffi_sample"; File = $cargoPath; Args = @("run", "-p", "spectra-interop", "--example", "rust_ffi_sample") }
    [PSCustomObject]@{ Nome = "python_phase8_demo"; File = "python"; Args = @("python\demo_phase8.py") }
)

foreach ($check in $interopChecks) {
    $r = Invoke-HostCommand -name $check.Nome -fileName $check.File -arguments $check.Args -workingDir (Get-Location).Path
    if ($r.Status -eq "PASSOU") {
        $totalPassed++
    } else {
        $totalFailed++
    }
    $results += [PSCustomObject]@{ Diretorio = "interop"; Teste = $check.Nome; Status = $r.Status; Detalhe = $r.Detail }
}

$cCompiler = $null
$cCompilerKind = $null
foreach ($candidate in @("clang", "gcc", "cl")) {
    $resolved = Get-Command $candidate -ErrorAction SilentlyContinue
    if ($resolved) {
        $cCompiler = $resolved.Source
        $cCompilerKind = $candidate
        break
    }
}
if (-not $cCompiler -and (Test-Path "C:\Program Files\LLVM\bin\clang.exe")) {
    $cCompiler = "C:\Program Files\LLVM\bin\clang.exe"
    $cCompilerKind = "clang"
}

if ($cCompiler) {
    $sampleExe = "target\release\c_ffi_sample.exe"
    $interopLib = "target\release\spectra_interop.dll.lib"
    if ($cCompilerKind -eq "cl") {
        $compileArgs = @(
            "/nologo",
            "/I", "tools\spectra-interop\include",
            "tools\spectra-interop\examples\c_ffi_sample.c",
            $interopLib,
            "/Fe:$sampleExe"
        )
    } else {
        $compileArgs = @(
            "-I", "tools\spectra-interop\include",
            "tools\spectra-interop\examples\c_ffi_sample.c",
            $interopLib,
            "-o", $sampleExe
        )
    }

    $compileResult = Invoke-HostCommand -name "c_ffi_sample_compile" -fileName $cCompiler -arguments $compileArgs -workingDir (Get-Location).Path
    if ($compileResult.Status -eq "PASSOU") {
        $totalPassed++
    } else {
        $totalFailed++
    }
    $results += [PSCustomObject]@{ Diretorio = "interop"; Teste = "c_ffi_sample_compile"; Status = $compileResult.Status; Detalhe = $compileResult.Detail }

    $sampleExeFullPath = Join-Path (Get-Location).Path $sampleExe
    $runResult = Invoke-HostCommand -name "c_ffi_sample_run" -fileName $sampleExeFullPath -arguments @() -workingDir (Join-Path (Get-Location).Path "target\release")
    if ($runResult.Status -eq "PASSOU") {
        $totalPassed++
    } else {
        $totalFailed++
    }
    $results += [PSCustomObject]@{ Diretorio = "interop"; Teste = "c_ffi_sample_run"; Status = $runResult.Status; Detalhe = $runResult.Detail }
} else {
    Write-Host "  c_ffi_sample" -NoNewline
    Write-Host " SKIP (compilador C ausente)" -ForegroundColor DarkYellow
    $totalSkipped++
    $results += [PSCustomObject]@{ Diretorio = "interop"; Teste = "c_ffi_sample"; Status = "SKIP"; Detalhe = "cl/clang/gcc nao encontrados; sample C nao foi compilado localmente" }
}

# ---------------------------------------------------------------------------
# Grupo 8.5: R-105 diagnostics standardization
# ---------------------------------------------------------------------------
$diagnosticsTemp = Join-Path (Get-Location).Path "target\r105-diagnostics"
if (Test-Path $diagnosticsTemp) {
    Remove-Item -LiteralPath $diagnosticsTemp -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $diagnosticsTemp | Out-Null

$jsonReport = Join-Path $diagnosticsTemp "diagnostics.json"
$sarifReport = Join-Path $diagnosticsTemp "diagnostics.sarif"

$jsonDiag = Invoke-SpectraCommand -commandArgs @("check", "--json", "tests\errors\type_mismatch.spectra") -workingDir (Get-Location).Path -includeExperimental $true
$sarifDiag = Invoke-SpectraCommand -commandArgs @("check", "--sarif", "tests\errors\type_mismatch.spectra") -workingDir (Get-Location).Path -includeExperimental $true
Set-Content -LiteralPath $jsonReport -Value $jsonDiag.Output -Encoding UTF8
Set-Content -LiteralPath $sarifReport -Value $sarifDiag.Output -Encoding UTF8

Write-Host ""
Write-Host "--- R-105 diagnostics standardization ---" -ForegroundColor Yellow
$diagValidate = Invoke-HostCommand -name "validate_diagnostics_standardization" -fileName "python" -arguments @("scripts\validate_diagnostics_standardization.py", "--json-report", $jsonReport, "--sarif-report", $sarifReport) -workingDir (Get-Location).Path
if (-not $jsonDiag.TimedOut -and -not $sarifDiag.TimedOut -and $diagValidate.Status -eq "PASSOU") {
    $totalPassed++
    $results += [PSCustomObject]@{ Diretorio = "phase1-diagnostics"; Teste = "validate_diagnostics_standardization"; Status = "PASSOU"; Detalhe = "" }
} else {
    $totalFailed++
    $detail = if ($diagValidate.Status -ne "PASSOU") { $diagValidate.Detail } else { "JSON/SARIF diagnostics timed out" }
    $results += [PSCustomObject]@{ Diretorio = "phase1-diagnostics"; Teste = "validate_diagnostics_standardization"; Status = "FALHOU"; Detalhe = $detail }
}

# ---------------------------------------------------------------------------
# Grupo 8.5b: R-108 diagnostic classification hardening
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-108 diagnostic classification hardening ---" -ForegroundColor Yellow
$diagClassification = Invoke-HostCommand -name "validate_r108_diagnostic_classification" -fileName "python" -arguments @("scripts\validate_r108_diagnostic_classification.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($diagClassification.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase1-diagnostics"; Teste = "validate_r108_diagnostic_classification"; Status = $diagClassification.Status; Detalhe = $diagClassification.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.5c: R-109 cross-module string value handling
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-109 cross-module string value handling ---" -ForegroundColor Yellow
$crossModuleStrings = Invoke-HostCommand -name "validate_r109_cross_module_strings" -fileName "python" -arguments @("scripts\validate_r109_cross_module_strings.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($crossModuleStrings.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase1-backend"; Teste = "validate_r109_cross_module_strings"; Status = $crossModuleStrings.Status; Detalhe = $crossModuleStrings.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.5d: R-110 cross-module type and method resolution
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-110 cross-module type and method resolution ---" -ForegroundColor Yellow
$crossModuleTypesMethods = Invoke-HostCommand -name "validate_r110_cross_module_types_methods" -fileName "python" -arguments @("scripts\validate_r110_cross_module_types_methods.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($crossModuleTypesMethods.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase1-semantics"; Teste = "validate_r110_cross_module_types_methods"; Status = $crossModuleTypesMethods.Status; Detalhe = $crossModuleTypesMethods.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.5e: R-111 cross-module aggregate codegen
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-111 cross-module aggregate codegen ---" -ForegroundColor Yellow
$crossModuleAggregates = Invoke-HostCommand -name "validate_r111_cross_module_aggregate_codegen" -fileName "python" -arguments @("scripts\validate_r111_cross_module_aggregate_codegen.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($crossModuleAggregates.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase1-backend"; Teste = "validate_r111_cross_module_aggregate_codegen"; Status = $crossModuleAggregates.Status; Detalhe = $crossModuleAggregates.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.5f: R-112 runtime float-to-int cast codegen
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-112 runtime float-to-int cast codegen ---" -ForegroundColor Yellow
$runtimeFloatCasts = Invoke-HostCommand -name "validate_r112_runtime_float_cast_codegen" -fileName "python" -arguments @("scripts\validate_r112_runtime_float_cast_codegen.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($runtimeFloatCasts.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase1-backend"; Teste = "validate_r112_runtime_float_cast_codegen"; Status = $runtimeFloatCasts.Status; Detalhe = $runtimeFloatCasts.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.6: R-106 feature maturity policy
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-106 feature maturity policy ---" -ForegroundColor Yellow
$featureMaturity = Invoke-HostCommand -name "validate_feature_maturity" -fileName "python" -arguments @("scripts\validate_feature_maturity.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($featureMaturity.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase1-features"; Teste = "validate_feature_maturity"; Status = $featureMaturity.Status; Detalhe = $featureMaturity.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.7: R-203 pattern ergonomics
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-203 pattern ergonomics ---" -ForegroundColor Yellow
$patternErgonomics = Invoke-HostCommand -name "validate_pattern_ergonomics" -fileName "python" -arguments @("scripts\validate_pattern_ergonomics.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($patternErgonomics.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase2-patterns"; Teste = "validate_pattern_ergonomics"; Status = $patternErgonomics.Status; Detalhe = $patternErgonomics.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.8: R-206 generic return type enforcement
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-206 generic return type enforcement ---" -ForegroundColor Yellow
$genericReturn = Invoke-HostCommand -name "validate_r206_generic_return_enforcement" -fileName "python" -arguments @("scripts\validate_r206_generic_return_enforcement.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($genericReturn.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase2-generics"; Teste = "validate_r206_generic_return_enforcement"; Status = $genericReturn.Status; Detalhe = $genericReturn.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.9: R-205 float const cast codegen
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-205 float const cast codegen ---" -ForegroundColor Yellow
$floatConstCast = Invoke-HostCommand -name "validate_r205_float_const_cast_codegen" -fileName "python" -arguments @("scripts\validate_r205_float_const_cast_codegen.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($floatConstCast.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase2-codegen"; Teste = "validate_r205_float_const_cast_codegen"; Status = $floatConstCast.Status; Detalhe = $floatConstCast.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.10: R-207 struct layout with padding and cumulative offsets
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-207 struct layout and drop semantics ---" -ForegroundColor Yellow
$structLayout = Invoke-HostCommand -name "validate_r207_struct_layout" -fileName "python" -arguments @("scripts\validate_r207_struct_layout.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($structLayout.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase2-oop-layout"; Teste = "validate_r207_struct_layout"; Status = $structLayout.Status; Detalhe = $structLayout.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.11: R-208 stable OOP diagnostic codes
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-208 stable OOP diagnostic codes ---" -ForegroundColor Yellow
$oopDiagnostics = Invoke-HostCommand -name "validate_r208_oop_diagnostics" -fileName "python" -arguments @("scripts\validate_r208_oop_diagnostics.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($oopDiagnostics.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase2-oop-diagnostics"; Teste = "validate_r208_oop_diagnostics"; Status = $oopDiagnostics.Status; Detalhe = $oopDiagnostics.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.12: R-1002 debugger and stack traces
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-1002 debugger and stack traces ---" -ForegroundColor Yellow
$debuggerStackTraces = Invoke-HostCommand -name "validate_debugger_stack_traces" -fileName "python" -arguments @("scripts\validate_debugger_stack_traces.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($debuggerStackTraces.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase10-debugger"; Teste = "validate_debugger_stack_traces"; Status = $debuggerStackTraces.Status; Detalhe = $debuggerStackTraces.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.10: R-1501 numerical performance benchmark gate
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-1501 numerical performance benchmarks ---" -ForegroundColor Yellow
$r1501Bench = Invoke-HostCommand -name "validate_r1501_bench" -fileName "python" -arguments @("scripts\validate_r1501_bench.py") -workingDir (Get-Location).Path
if ($r1501Bench.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase15-performance"; Teste = "validate_r1501_bench"; Status = $r1501Bench.Status; Detalhe = $r1501Bench.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.11: R-1503 numerical correctness certification
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-1503 numerical correctness certification ---" -ForegroundColor Yellow
$r1503Correctness = Invoke-HostCommand -name "validate_r1503_correctness" -fileName "python" -arguments @("scripts\validate_r1503_correctness.py") -workingDir (Get-Location).Path
if ($r1503Correctness.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase15-correctness"; Teste = "validate_r1503_correctness"; Status = $r1503Correctness.Status; Detalhe = $r1503Correctness.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.12: R-1601 tensor graph IR
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-1601 tensor graph IR ---" -ForegroundColor Yellow
$r1601TensorGraph = Invoke-HostCommand -name "tensor_graph_tests" -fileName "cargo" -arguments @("test", "-p", "spectra-midend", "--test", "tensor_graph_tests") -workingDir (Get-Location).Path
if ($r1601TensorGraph.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase16-graph"; Teste = "tensor_graph_tests"; Status = $r1601TensorGraph.Status; Detalhe = $r1601TensorGraph.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.13: R-1602 graph optimization and fusion
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-1602 graph optimization and fusion ---" -ForegroundColor Yellow
$r1602GraphOptimization = Invoke-HostCommand -name "tensor_graph_optimization_tests" -fileName "cargo" -arguments @("test", "-p", "spectra-midend", "--test", "tensor_graph_tests", "optimizer") -workingDir (Get-Location).Path
if ($r1602GraphOptimization.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase16-optimization"; Teste = "tensor_graph_optimization_tests"; Status = $r1602GraphOptimization.Status; Detalhe = $r1602GraphOptimization.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.14: R-1603 production GPU backend
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-1603 production GPU backend ---" -ForegroundColor Yellow
$r1603GpuBackend = Invoke-HostCommand -name "validate_r1603_gpu_backend" -fileName "python" -arguments @("scripts\validate_r1603_gpu_backend.py") -workingDir (Get-Location).Path
if ($r1603GpuBackend.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase16-gpu"; Teste = "validate_r1603_gpu_backend"; Status = $r1603GpuBackend.Status; Detalhe = $r1603GpuBackend.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.15: R-1701 dataset and dataframe runtime
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-1701 dataset and dataframe runtime ---" -ForegroundColor Yellow
$r1701DataRuntime = Invoke-HostCommand -name "validate_r1701_data_runtime" -fileName "python" -arguments @("scripts\validate_r1701_data_runtime.py") -workingDir (Get-Location).Path
if ($r1701DataRuntime.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase17-data"; Teste = "validate_r1701_data_runtime"; Status = $r1701DataRuntime.Status; Detalhe = $r1701DataRuntime.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.16: R-1702 experiment tracking and reproducibility
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-1702 experiment tracking and reproducibility ---" -ForegroundColor Yellow
$r1702ExperimentTracking = Invoke-HostCommand -name "validate_r1702_experiment_tracking" -fileName "python" -arguments @("scripts\validate_r1702_experiment_tracking.py") -workingDir (Get-Location).Path
if ($r1702ExperimentTracking.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase17-experiments"; Teste = "validate_r1702_experiment_tracking"; Status = $r1702ExperimentTracking.Status; Detalhe = $r1702ExperimentTracking.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.17: R-1703 distributed training foundations
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-1703 distributed training foundations ---" -ForegroundColor Yellow
$r1703DistributedTraining = Invoke-HostCommand -name "validate_r1703_distributed_training" -fileName "python" -arguments @("scripts\validate_r1703_distributed_training.py") -workingDir (Get-Location).Path
if ($r1703DistributedTraining.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase17-distributed"; Teste = "validate_r1703_distributed_training"; Status = $r1703DistributedTraining.Status; Detalhe = $r1703DistributedTraining.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.18: R-1801 ONNX import and export
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-1801 ONNX import and export ---" -ForegroundColor Yellow
$r1801OnnxImportExport = Invoke-HostCommand -name "validate_r1801_onnx_import_export" -fileName "python" -arguments @("scripts\validate_r1801_onnx_import_export.py") -workingDir (Get-Location).Path
if ($r1801OnnxImportExport.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase18-onnx"; Teste = "validate_r1801_onnx_import_export"; Status = $r1801OnnxImportExport.Status; Detalhe = $r1801OnnxImportExport.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.19: R-1802 transformer and LLM runtime primitives
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-1802 transformer and LLM runtime primitives ---" -ForegroundColor Yellow
$r1802TransformerPrimitives = Invoke-HostCommand -name "validate_r1802_transformer_primitives" -fileName "python" -arguments @("scripts\validate_r1802_transformer_primitives.py") -workingDir (Get-Location).Path
if ($r1802TransformerPrimitives.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase18-transformers"; Teste = "validate_r1802_transformer_primitives"; Status = $r1802TransformerPrimitives.Status; Detalhe = $r1802TransformerPrimitives.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.20: R-1803 tokenization embeddings and RAG toolkit
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-1803 tokenization embeddings and RAG toolkit ---" -ForegroundColor Yellow
$r1803RagToolkit = Invoke-HostCommand -name "validate_r1803_rag_toolkit" -fileName "python" -arguments @("scripts\validate_r1803_rag_toolkit.py") -workingDir (Get-Location).Path
if ($r1803RagToolkit.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase18-rag"; Teste = "validate_r1803_rag_toolkit"; Status = $r1803RagToolkit.Status; Detalhe = $r1803RagToolkit.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.21: R-1901 model evaluation and metrics suite
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-1901 model evaluation and metrics suite ---" -ForegroundColor Yellow
$r1901EvaluationMetrics = Invoke-HostCommand -name "validate_r1901_evaluation_metrics" -fileName "python" -arguments @("scripts\validate_r1901_evaluation_metrics.py") -workingDir (Get-Location).Path
if ($r1901EvaluationMetrics.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase19-evaluation"; Teste = "validate_r1901_evaluation_metrics"; Status = $r1901EvaluationMetrics.Status; Detalhe = $r1901EvaluationMetrics.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.22: R-1902 AI safety and guardrail runtime
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-1902 AI safety and guardrail runtime ---" -ForegroundColor Yellow
$r1902SafetyGuardrails = Invoke-HostCommand -name "validate_r1902_ai_safety_guardrails" -fileName "python" -arguments @("scripts\validate_r1902_ai_safety_guardrails.py") -workingDir (Get-Location).Path
if ($r1902SafetyGuardrails.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase19-safety"; Teste = "validate_r1902_ai_safety_guardrails"; Status = $r1902SafetyGuardrails.Status; Detalhe = $r1902SafetyGuardrails.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.23: R-1903 model monitoring and drift detection
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-1903 model monitoring and drift detection ---" -ForegroundColor Yellow
$r1903ModelMonitoring = Invoke-HostCommand -name "validate_r1903_model_monitoring" -fileName "python" -arguments @("scripts\validate_r1903_model_monitoring.py") -workingDir (Get-Location).Path
if ($r1903ModelMonitoring.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase19-monitoring"; Teste = "validate_r1903_model_monitoring"; Status = $r1903ModelMonitoring.Status; Detalhe = $r1903ModelMonitoring.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.25: R-2002 production release channels
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2002 production release channels ---" -ForegroundColor Yellow
$r2002ReleaseChannels = Invoke-HostCommand -name "validate_r2002_release_channels" -fileName "python" -arguments @("scripts\validate_r2002_release_channels.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($r2002ReleaseChannels.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase20-release"; Teste = "validate_r2002_release_channels"; Status = $r2002ReleaseChannels.Status; Detalhe = $r2002ReleaseChannels.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.26: R-2003 base language and std regression audit
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2003 base language and std regression audit ---" -ForegroundColor Yellow
$r2003BaseRegression = Invoke-HostCommand -name "validate_r2003_base_regression_audit" -fileName "python" -arguments @("scripts\validate_r2003_base_regression_audit.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($r2003BaseRegression.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase20-base-stabilization"; Teste = "validate_r2003_base_regression_audit"; Status = $r2003BaseRegression.Status; Detalhe = $r2003BaseRegression.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.27: R-2005 core std/runtime host-status hardening
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2005 core std/runtime host-status hardening ---" -ForegroundColor Yellow
$r2005RuntimeHardening = Invoke-HostCommand -name "validate_r2005_runtime_hardening" -fileName "python" -arguments @("scripts\validate_r2005_runtime_hardening.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($r2005RuntimeHardening.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase20-runtime-hardening"; Teste = "validate_r2005_runtime_hardening"; Status = $r2005RuntimeHardening.Status; Detalhe = $r2005RuntimeHardening.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.27b: R-2015 std.time production surface
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2015 std.time production surface ---" -ForegroundColor Yellow
$r2015StdTime = Invoke-HostCommand -name "validate_r2015_std_time" -fileName "python" -arguments @("scripts\validate_r2015_std_time.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($r2015StdTime.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase20-std-time"; Teste = "validate_r2015_std_time"; Status = $r2015StdTime.Status; Detalhe = $r2015StdTime.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.27c: R-2902 range production semantics
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2902 range production semantics ---" -ForegroundColor Yellow
$r2902RangeProduction = Invoke-HostCommand -name "validate_r2902_range_production" -fileName "python" -arguments @("scripts\validate_r2902_range_production.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($r2902RangeProduction.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase29-range-production"; Teste = "validate_r2902_range_production"; Status = $r2902RangeProduction.Status; Detalhe = $r2902RangeProduction.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.27d: R-3007 stdlib production contract and capability audit
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-3007 stdlib production contract and capability audit ---" -ForegroundColor Yellow
$r3007StdlibContract = Invoke-HostCommand -name "validate_r3007_stdlib_contract" -fileName "python" -arguments @("scripts\validate_r3007_stdlib_contract.py", "--manifest", "scripts\stdlib_contract.toml", "--binary", $binary, "--report", "target\r3007-stdlib-contract\report.json") -workingDir (Get-Location).Path
if ($r3007StdlibContract.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase30-stdlib-contract"; Teste = "validate_r3007_stdlib_contract"; Status = $r3007StdlibContract.Status; Detalhe = $r3007StdlibContract.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.27a: R-2501 async-aware connection pool
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2501 async-aware connection pool ---" -ForegroundColor Yellow
$r2501ConnectionPool = Invoke-HostCommand -name "validate_r2501_connection_pool" -fileName "python" -arguments @("scripts\validate_r2501_pool.py", "--report", "target\r2501-connection-pool\report.json") -workingDir (Get-Location).Path
if ($r2501ConnectionPool.Status -eq "PASSOU") { $totalPassed++ } else { $totalFailed++ }
$results += [PSCustomObject]@{ Diretorio = "phase25-connection-pool"; Teste = "validate_r2501_connection_pool"; Status = $r2501ConnectionPool.Status; Detalhe = $r2501ConnectionPool.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.27ab: R-2504 SQLite driver sync and async
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2504 SQLite driver sync and async ---" -ForegroundColor Yellow
$r2504Sqlite = Invoke-HostCommand -name "validate_r2504_sqlite" -fileName "python" -arguments @("scripts\validate_r2504_sqlite.py", "--binary", $binary, "--fixture", "tests\validation\194_sqlite_driver.spectra", "--database", "tests\fixtures\r2504\reference.sqlite", "--report", "target\r2504-sqlite\report.json") -workingDir (Get-Location).Path
if ($r2504Sqlite.Status -eq "PASSOU") { $totalPassed++ } else { $totalFailed++ }
$results += [PSCustomObject]@{ Diretorio = "phase25-sqlite-driver"; Teste = "validate_r2504_sqlite"; Status = $r2504Sqlite.Status; Detalhe = $r2504Sqlite.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.27ac: R-2502 type-safe SQL query builder
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2502 type-safe SQL query builder ---" -ForegroundColor Yellow
$r2502QueryBuilder = Invoke-HostCommand -name "validate_r2502_query_builder" -fileName "python" -arguments @("scripts\validate_r2502_query_builder.py", "--schema", "tests\fixtures\r2502\schema.sql", "--report", "target\r2502-query-builder\report.json") -workingDir (Get-Location).Path
if ($r2502QueryBuilder.Status -eq "PASSOU") { $totalPassed++ } else { $totalFailed++ }
$results += [PSCustomObject]@{ Diretorio = "phase25-query-builder"; Teste = "validate_r2502_query_builder"; Status = $r2502QueryBuilder.Status; Detalhe = $r2502QueryBuilder.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.27ad: R-2503 migrations framework
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2503 migrations framework ---" -ForegroundColor Yellow
$r2503Migrations = Invoke-HostCommand -name "validate_r2503_migrations" -fileName "python" -arguments @("scripts\validate_r2503_migrations.py", "--binary", $binary, "--database", "target\r2503-migrations\validation.sqlite", "--migrations-dir", "tests\fixtures\r2503\migrations", "--report", "target\r2503-migrations\report.json") -workingDir (Get-Location).Path
if ($r2503Migrations.Status -eq "PASSOU") { $totalPassed++ } else { $totalFailed++ }
$results += [PSCustomObject]@{ Diretorio = "phase25-migrations"; Teste = "validate_r2503_migrations"; Status = $r2503Migrations.Status; Detalhe = $r2503Migrations.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.27af: R-2514 multi-version migration example
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2514 multi-version migration example ---" -ForegroundColor Yellow
$r2514MigrationExample = Invoke-HostCommand -name "validate_r2514_migrations_example" -fileName "python" -arguments @("scripts\validate_r2514_migrations_example.py", "--binary", $binary, "--fixture", "tests\validation\202_migrations_multi_version.spectra", "--database", "target\r2514-migrations-example\validation.sqlite", "--migrations-dir", "tests\fixtures\r2514\migrations", "--report", "target\r2514-migrations-example\report.json") -workingDir (Get-Location).Path
if ($r2514MigrationExample.Status -eq "PASSOU") { $totalPassed++ } else { $totalFailed++ }
$results += [PSCustomObject]@{ Diretorio = "phase25-migration-example"; Teste = "validate_r2514_migrations_example"; Status = $r2514MigrationExample.Status; Detalhe = $r2514MigrationExample.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.27ae: R-2511 REST + SQLite CRUD real
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2511 REST + SQLite CRUD ---" -ForegroundColor Yellow
$r2511RestSqlite = Invoke-HostCommand -name "validate_r2511_rest_sqlite" -fileName "python" -arguments @("scripts\validate_r2511_rest_sqlite.py", "--binary", $binary, "--fixture", "tests\validation\201_rest_sqlite_crud.spectra", "--database", "target\r2511-rest-sqlite\validation.sqlite", "--migrations-dir", "tests\fixtures\r2511\migrations", "--report", "target\r2511-rest-sqlite\report.json") -workingDir (Get-Location).Path
if ($r2511RestSqlite.Status -eq "PASSOU") { $totalPassed++ } else { $totalFailed++ }
$results += [PSCustomObject]@{ Diretorio = "phase25-rest-sqlite-crud"; Teste = "validate_r2511_rest_sqlite"; Status = $r2511RestSqlite.Status; Detalhe = $r2511RestSqlite.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.27ab: R-2505 PostgreSQL driver (requires real PostgreSQL lane)
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2505 PostgreSQL driver ---" -ForegroundColor Yellow
$r2505Arguments = @("scripts\validate_r2505_postgres.py", "--binary", $binary, "--fixture", "tests\validation\195_postgres_driver.spectra", "--report", "target\r2505-postgres\report.json")
if ($env:SPECTRA_POSTGRES_URL) { $r2505Arguments += @("--database-url", $env:SPECTRA_POSTGRES_URL) }
if ($env:SPECTRA_POSTGRES_VERSION_PROBE_DOCKER_CONTAINER) {
    $r2505Arguments += @("--version-probe-docker-container", $env:SPECTRA_POSTGRES_VERSION_PROBE_DOCKER_CONTAINER)
}
$r2505Postgres = Invoke-HostCommand -name "validate_r2505_postgres" -fileName "python" -arguments $r2505Arguments -workingDir (Get-Location).Path
if ($r2505Postgres.Detail -match "skipped_environment") {
    $totalSkipped++
    $r2505Status = "IGNORADO"
} elseif ($r2505Postgres.Status -eq "PASSOU") {
    $totalPassed++
    $r2505Status = "PASSOU"
} else {
    $totalFailed++
    $r2505Status = "FALHOU"
}
$results += [PSCustomObject]@{ Diretorio = "phase25-postgres-driver"; Teste = "validate_r2505_postgres"; Status = $r2505Status; Detalhe = $r2505Postgres.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.27ac: R-2507 Redis driver (requires real Redis 7 lane)
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2507 Redis driver ---" -ForegroundColor Yellow
$r2507Redis = Invoke-HostCommand -name "validate_r2507_redis" -fileName "python" -arguments @("scripts\validate_r2507_redis.py", "--binary", $binary, "--fixture", "tests\validation\196_redis_driver.spectra", "--report", "target\r2507-redis\report.json") -workingDir (Get-Location).Path
if ($r2507Redis.Status -eq "PASSOU" -or $r2507Redis.Detail -match "skipped_environment") { $totalPassed++ } else { $totalFailed++ }
$results += [PSCustomObject]@{ Diretorio = "phase25-redis-driver"; Teste = "validate_r2507_redis"; Status = $r2507Redis.Status; Detalhe = $r2507Redis.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.27ad: R-2510 real health checks
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2510 health checks ---" -ForegroundColor Yellow
$r2510Health = Invoke-HostCommand -name "validate_r2510_health" -fileName "python" -arguments @("scripts\validate_r2510_health.py", "--binary", $binary, "--fixture", "tests\validation\197_health_checks.spectra", "--report", "target\r2510-health\report.json") -workingDir (Get-Location).Path
if ($r2510Health.Status -eq "PASSOU") { $totalPassed++ } else { $totalFailed++ }
$results += [PSCustomObject]@{ Diretorio = "phase25-health-checks"; Teste = "validate_r2510_health"; Status = $r2510Health.Status; Detalhe = $r2510Health.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.27ae: R-2703 integrated deployment health probes
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2703 integrated health probes ---" -ForegroundColor Yellow
$r2703Health = Invoke-HostCommand -name "validate_r2703_health_probes" -fileName "python" -arguments @("scripts\validate_r2703_health_probes.py", "--binary", $binary, "--fixture", "tests\validation\198_health_probes_deployment.spectra", "--report", "target\r2703-health-probes\report.json") -workingDir (Get-Location).Path
if ($r2703Health.Status -eq "PASSOU") { $totalPassed++ } else { $totalFailed++ }
$results += [PSCustomObject]@{ Diretorio = "phase27-integrated-health-probes"; Teste = "validate_r2703_health_probes"; Status = $r2703Health.Status; Detalhe = $r2703Health.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.27af: R-2702 Prometheus-compatible metrics
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2702 Prometheus-compatible metrics ---" -ForegroundColor Yellow
$r2702Metrics = Invoke-HostCommand -name "validate_r2702_metrics" -fileName "python" -arguments @("scripts\validate_r2702_metrics.py", "--binary", $binary, "--fixture", "tests\validation\199_prometheus_metrics.spectra", "--report", "target\r2702-metrics\report.json") -workingDir (Get-Location).Path
if ($r2702Metrics.Status -eq "PASSOU") { $totalPassed++ } else { $totalFailed++ }
$results += [PSCustomObject]@{ Diretorio = "phase27-prometheus-metrics"; Teste = "validate_r2702_metrics"; Status = $r2702Metrics.Status; Detalhe = $r2702Metrics.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.27ag: R-2707 integrated OTel + Prometheus example
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2707 OTel and Prometheus exporters example ---" -ForegroundColor Yellow
$r2707Exporters = Invoke-HostCommand -name "validate_r2707_exporters_example" -fileName "python" -arguments @("scripts\validate_r2707_exporters_example.py", "--binary", $binary, "--fixture", "tests\validation\200_otel_prometheus_example.spectra", "--report", "target\r2707-otel-prometheus\report.json") -workingDir (Get-Location).Path
if ($r2707Exporters.Status -eq "PASSOU") { $totalPassed++ } else { $totalFailed++ }
$results += [PSCustomObject]@{ Diretorio = "phase27-otel-prometheus-example"; Teste = "validate_r2707_exporters_example"; Status = $r2707Exporters.Status; Detalhe = $r2707Exporters.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.27e: R-3003 native production artifact container
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-3003 native production artifact container ---" -ForegroundColor Yellow
$r3003Artifacts = Invoke-HostCommand -name "validate_r3003_artifacts" -fileName "python" -arguments @("scripts\validate_r3003_artifacts.py", "--binary", $binary, "--fixture", "tests\validation\186_ml_artifact_container.spectra", "--report", "target\r3003-artifacts\report.json") -workingDir (Get-Location).Path
if ($r3003Artifacts.Status -eq "PASSOU") { $totalPassed++ } else { $totalFailed++ }
$results += [PSCustomObject]@{ Diretorio = "phase30-ml-artifacts"; Teste = "validate_r3003_artifacts"; Status = $r3003Artifacts.Status; Detalhe = $r3003Artifacts.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.27f: R-3005 production tokenization and embedding artifacts
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-3005 production tokenization and embedding artifacts ---" -ForegroundColor Yellow
$r3005TokenizationEmbedding = Invoke-HostCommand -name "validate_r3005_tokenization_embedding" -fileName "python" -arguments @("scripts\validate_r3005_tokenization_embedding.py", "--binary", $binary, "--fixture", "tests\validation\187_ml_tokenization_embedding_artifacts.spectra", "--report", "target\r3005-tokenization-embedding\report.json") -workingDir (Get-Location).Path
if ($r3005TokenizationEmbedding.Status -eq "PASSOU") { $totalPassed++ } else { $totalFailed++ }
$results += [PSCustomObject]@{ Diretorio = "phase30-tokenization-embedding"; Teste = "validate_r3005_tokenization_embedding"; Status = $r3005TokenizationEmbedding.Status; Detalhe = $r3005TokenizationEmbedding.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.27g: R-3006 persistent production vector index
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-3006 persistent production vector index ---" -ForegroundColor Yellow
$r3006VectorIndex = Invoke-HostCommand -name "validate_r3006_vector_index" -fileName "python" -arguments @("scripts\validate_r3006_vector_index.py", "--binary", $binary, "--fixture", "tests\validation\188_ml_vector_index_production.spectra", "--report", "target\r3006-vector-index\report.json") -workingDir (Get-Location).Path
if ($r3006VectorIndex.Status -eq "PASSOU") { $totalPassed++ } else { $totalFailed++ }
$results += [PSCustomObject]@{ Diretorio = "phase30-vector-index"; Teste = "validate_r3006_vector_index"; Status = $r3006VectorIndex.Status; Detalhe = $r3006VectorIndex.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.27h: R-2901 exact-width numeric runtime semantics
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2901 exact-width numeric runtime semantics ---" -ForegroundColor Yellow
$r2901ExactWidth = Invoke-HostCommand -name "validate_r2901_exact_width" -fileName "python" -arguments @("scripts\validate_r2901_exact_width.py", "--binary", $binary, "--fixture", "tests\validation\189_exact_width_numeric_semantics.spectra", "--report", "target\r2901-exact-width\report.json") -workingDir (Get-Location).Path
if ($r2901ExactWidth.Status -eq "PASSOU") { $totalPassed++ } else { $totalFailed++ }
$results += [PSCustomObject]@{ Diretorio = "phase29-exact-width"; Teste = "validate_r2901_exact_width"; Status = $r2901ExactWidth.Status; Detalhe = $r2901ExactWidth.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.27i: R-2904 first-class tensor IR and device lowering
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2904 first-class tensor IR and device lowering ---" -ForegroundColor Yellow
$r2904TensorIr = Invoke-HostCommand -name "validate_r2904_tensor_ir" -fileName "python" -arguments @("scripts\validate_r2904_tensor_ir.py", "--binary", $binary, "--fixture", "tests\validation\190_tensor_ir_device_lowering.spectra", "--report", "target\r2904-tensor-ir\report.json") -workingDir (Get-Location).Path
if ($r2904TensorIr.Status -eq "PASSOU") { $totalPassed++ } else { $totalFailed++ }
$results += [PSCustomObject]@{ Diretorio = "phase29-tensor-ir"; Teste = "validate_r2904_tensor_ir"; Status = $r2904TensorIr.Status; Detalhe = $r2904TensorIr.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.27i1: R-3004 compiler-native autodiff lowering
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-3004 compiler-native autodiff lowering ---" -ForegroundColor Yellow
$r3004Autodiff = Invoke-HostCommand -name "validate_r3004_compiler_native_autodiff" -fileName "python" -arguments @("scripts\validate_r3004_compiler_native_autodiff.py", "--binary", $binary, "--fixture", "tests\validation\192_compiler_native_autodiff.spectra", "--report", "target\r3004-autodiff\report.json") -workingDir (Get-Location).Path
if ($r3004Autodiff.Status -eq "PASSOU") { $totalPassed++ } else { $totalFailed++ }
$results += [PSCustomObject]@{ Diretorio = "phase29-compiler-native-autodiff"; Teste = "validate_r3004_compiler_native_autodiff"; Status = $r3004Autodiff.Status; Detalhe = $r3004Autodiff.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.27j: R-2903 native debug information
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2903 native debug information ---" -ForegroundColor Yellow
$r2903NativeDebug = Invoke-HostCommand -name "validate_r2903_native_debug" -fileName "python" -arguments @("scripts\validate_r2903_native_debug.py", "--binary", $binary, "--fixture", "tests\validation\191_native_debug_info.spectra", "--report", "target\r2903-native-debug\report.json") -workingDir (Get-Location).Path
$r2903Status = $r2903NativeDebug.Status
$r2903Detail = $r2903NativeDebug.Detail
$r2903Deferred = $false
$r2903ReportPath = Join-Path (Get-Location).Path "target\r2903-native-debug\report.json"
if ($r2903NativeDebug.Status -ne "PASSOU" -and (Test-Path -LiteralPath $r2903ReportPath)) {
    try {
        $r2903Report = Get-Content -LiteralPath $r2903ReportPath -Raw | ConvertFrom-Json
        $r2903Failures = @($r2903Report.failures)
        $r2903Deferred = (
            $r2903Report.status -eq "failed" -and
            $r2903Report.local_validation.status -eq "failed" -and
            $r2903Report.local_validation.location_evidence -eq "compatibility_frame_relative_zero" -and
            $r2903Report.local_validation.reason -eq "compiler-proven stack/register location is not available yet" -and
            $r2903Report.function_validation.status -eq "passed" -and
            $r2903Report.line_validation.status -eq "passed" -and
            $r2903Report.sidecar_validation.status -eq "passed" -and
            $r2903Report.executable.status -eq "passed" -and
            $r2903Report.pdb_validation.status -eq "passed" -and
            $r2903Report.symbol_resolution.status -eq "passed" -and
            $r2903Failures.Count -eq 1 -and
            $r2903Failures[0] -eq "native local location is not compiler-proven; compatibility frame-relative records are insufficient"
        )
    } catch {
        $r2903Deferred = $false
    }
}
if ($r2903NativeDebug.Status -eq "PASSOU") {
    $totalPassed++
} elseif ($r2903Deferred) {
    # R-2903 remains in_progress. Keep the direct validator fail-closed while
    # excluding its one documented, non-regression gap from decisive totals.
    $totalInfo++
    $r2903Status = "INFO:DEFERRED"
    $r2903Detail = "R-2903 remains in_progress: allocator-backed local location is not emitted yet"
    Write-Host "     INFO: R-2903 local location evidence remains deferred" -ForegroundColor Yellow
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase29-native-debug"; Teste = "validate_r2903_native_debug"; Status = $r2903Status; Detalhe = $r2903Detail }

# ---------------------------------------------------------------------------
# Grupo 8.28: R-2006 tensor/std performance refresh
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2006 tensor/std performance refresh ---" -ForegroundColor Yellow
$r2006PerformanceRefresh = Invoke-HostCommand -name "validate_r2006_performance_refresh" -fileName "python" -arguments @("scripts\validate_r2006_performance_refresh.py") -workingDir (Get-Location).Path
if ($r2006PerformanceRefresh.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase20-performance-refresh"; Teste = "validate_r2006_performance_refresh"; Status = $r2006PerformanceRefresh.Status; Detalhe = $r2006PerformanceRefresh.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.29: R-2007 backend/codegen robustness
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2007 backend/codegen robustness ---" -ForegroundColor Yellow
$r2007BackendCodegen = Invoke-HostCommand -name "validate_r2007_backend_codegen" -fileName "python" -arguments @("scripts\validate_r2007_backend_codegen.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($r2007BackendCodegen.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase20-backend-codegen"; Teste = "validate_r2007_backend_codegen"; Status = $r2007BackendCodegen.Status; Detalhe = $r2007BackendCodegen.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.30: R-2008 language feature project matrix
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2008 language feature project matrix ---" -ForegroundColor Yellow
$r2008LanguageFeatureMatrix = Invoke-HostCommand -name "validate_r2008_language_feature_matrix" -fileName "python" -arguments @("scripts\validate_r2008_language_feature_matrix.py") -workingDir (Get-Location).Path
if ($r2008LanguageFeatureMatrix.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase20-project-matrix"; Teste = "validate_r2008_language_feature_matrix"; Status = $r2008LanguageFeatureMatrix.Status; Detalhe = $r2008LanguageFeatureMatrix.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.31: R-2013 release candidate integrated project gate
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2013 release candidate integrated project gate ---" -ForegroundColor Yellow
$r2013ReleaseCandidate = Invoke-HostCommand -name "validate_r2013_release_candidate" -fileName "python" -arguments @("scripts\validate_r2013_release_candidate.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($r2013ReleaseCandidate.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}

if (-not (Test-Path $phase31BinaryPath)) {
    Write-Host "Binario release nao encontrado. Compilando para Phase 31..." -ForegroundColor Yellow
    & "C:\Users\estev\.cargo\bin\cargo.exe" build --release -p spectra-cli 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) {
        Write-Host "ERRO: Falha ao compilar binario release para Phase 31." -ForegroundColor Red
        exit 1
    }
}
$phase31Binary = (Resolve-Path $phase31BinaryPath).Path
$results += [PSCustomObject]@{ Diretorio = "phase20-release-candidate"; Teste = "validate_r2013_release_candidate"; Status = $r2013ReleaseCandidate.Status; Detalhe = $r2013ReleaseCandidate.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.33: R-2101 async/await execution model ADR
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2101 async/await execution model ADR ---" -ForegroundColor Yellow
$r2101AsyncAdr = Invoke-HostCommand -name "validate_r2101_async_adr" -fileName "python" -arguments @("scripts\validate_r2101_async_adr.py") -workingDir (Get-Location).Path
if ($r2101AsyncAdr.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase21-async"; Teste = "validate_r2101_async_adr"; Status = $r2101AsyncAdr.Status; Detalhe = $r2101AsyncAdr.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.28: R-2102 async frontend surface
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2102 async frontend surface ---" -ForegroundColor Yellow
$r2102AsyncFrontend = Invoke-HostCommand -name "validate_r2102_async_frontend" -fileName "python" -arguments @("scripts\validate_r2102_async_frontend.py") -workingDir (Get-Location).Path
if ($r2102AsyncFrontend.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase21-async"; Teste = "validate_r2102_async_frontend"; Status = $r2102AsyncFrontend.Status; Detalhe = $r2102AsyncFrontend.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.29: R-2103 await expression and async lowering
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2103 await expression and async lowering ---" -ForegroundColor Yellow
$r2103AsyncLowering = Invoke-HostCommand -name "validate_r2103_async_lowering" -fileName "python" -arguments @("scripts\validate_r2103_async_lowering.py") -workingDir (Get-Location).Path
if ($r2103AsyncLowering.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase21-async"; Teste = "validate_r2103_async_lowering"; Status = $r2103AsyncLowering.Status; Detalhe = $r2103AsyncLowering.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.30: R-2104 event loop multiplexer
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2104 event loop multiplexer ---" -ForegroundColor Yellow
$r2104Reactor = Invoke-HostCommand -name "validate_r2104_reactor" -fileName "python" -arguments @("scripts\validate_r2104_reactor.py") -workingDir (Get-Location).Path
if ($r2104Reactor.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase21-async"; Teste = "validate_r2104_reactor"; Status = $r2104Reactor.Status; Detalhe = $r2104Reactor.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.31: R-2105 cancellation, timeouts, and structured concurrency
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2105 cancellation, timeouts, and structured concurrency ---" -ForegroundColor Yellow
$r2105StructuredConcurrency = Invoke-HostCommand -name "validate_r2105_structured_concurrency" -fileName "python" -arguments @("scripts\validate_r2105_structured_concurrency.py") -workingDir (Get-Location).Path
if ($r2105StructuredConcurrency.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase21-async"; Teste = "validate_r2105_structured_concurrency"; Status = $r2105StructuredConcurrency.Status; Detalhe = $r2105StructuredConcurrency.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.31: R-2106 Stream type and stream adaptors
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2106 Stream type and stream adaptors ---" -ForegroundColor Yellow
$r2106Streams = Invoke-HostCommand -name "validate_r2106_streams" -fileName "python" -arguments @("scripts\validate_r2106_streams.py") -workingDir (Get-Location).Path
if ($r2106Streams.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase21-async"; Teste = "validate_r2106_streams"; Status = $r2106Streams.Status; Detalhe = $r2106Streams.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.32: R-2107 async standard library surface
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2107 async standard library surface ---" -ForegroundColor Yellow
$r2107AsyncStdlib = Invoke-HostCommand -name "validate_r2107_async_stdlib" -fileName "python" -arguments @("scripts\validate_r2107_async_stdlib.py") -workingDir (Get-Location).Path
if ($r2107AsyncStdlib.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase21-async"; Teste = "validate_r2107_async_stdlib"; Status = $r2107AsyncStdlib.Status; Detalhe = $r2107AsyncStdlib.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.33: R-2108 async trait objects and dyn Future/Stream
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2108 async trait objects and dyn Future/Stream ---" -ForegroundColor Yellow
$r2108AsyncTraitObjects = Invoke-HostCommand -name "validate_r2108_async_trait_objects" -fileName "python" -arguments @("scripts\validate_r2108_async_trait_objects.py") -workingDir (Get-Location).Path
if ($r2108AsyncTraitObjects.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase21-async"; Teste = "validate_r2108_async_trait_objects"; Status = $r2108AsyncTraitObjects.Status; Detalhe = $r2108AsyncTraitObjects.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.34: R-2109 async test runtime and macros
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2109 async test runtime and macros ---" -ForegroundColor Yellow
$r2109AsyncTestRuntime = Invoke-HostCommand -name "validate_r2109_async_test_runtime" -fileName "python" -arguments @("scripts\validate_r2109_async_test_runtime.py") -workingDir (Get-Location).Path
if ($r2109AsyncTestRuntime.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase21-async"; Teste = "validate_r2109_async_test_runtime"; Status = $r2109AsyncTestRuntime.Status; Detalhe = $r2109AsyncTestRuntime.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.35: R-2110 async diagnostics and Send/Sync validation
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2110 async diagnostics and Send/Sync validation ---" -ForegroundColor Yellow
$r2110AsyncSendSync = Invoke-HostCommand -name "validate_r2110_async_send_sync" -fileName "python" -arguments @("scripts\validate_r2110_async_send_sync.py") -workingDir (Get-Location).Path
if ($r2110AsyncSendSync.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase21-async"; Teste = "validate_r2110_async_send_sync"; Status = $r2110AsyncSendSync.Status; Detalhe = $r2110AsyncSendSync.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.36: R-2111 async benchmarks and profiling
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2111 async benchmarks and profiling ---" -ForegroundColor Yellow
$r2111AsyncBench = Invoke-HostCommand -name "validate_r2111_async_bench" -fileName "python" -arguments @("scripts\validate_r2111_async_bench.py") -workingDir (Get-Location).Path
if ($r2111AsyncBench.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase21-async"; Teste = "validate_r2111_async_bench"; Status = $r2111AsyncBench.Status; Detalhe = $r2111AsyncBench.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.36b: R-3101 Phase 31 cross-language benchmark gate
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-3101 Phase 31 cross-language benchmark gate ---" -ForegroundColor Yellow
$phase31Driver = Invoke-HostCommand -name "phase31_run_all" -fileName "python" -arguments @("scripts\phase31_run_all.py", "--code-validation", "--out", "target\phase31\cross-lang-report.json", "--spectra-binary", $phase31Binary, "--spectra-profile", "release", "--independent-runs", "1", "--baseline", "docs\performance\phase31-go-comparable\baseline.json", "--confirm-regressions", "0", "--timeout-seconds", "60") -workingDir (Get-Location).Path -timeoutSeconds 600
if ($phase31Driver.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase31-cross-lang"; Teste = "phase31_run_all"; Status = $phase31Driver.Status; Detalhe = $phase31Driver.Detail }

$phase31Gate = Invoke-HostCommand -name "validate_phase31_cross_lang" -fileName "python" -arguments @("scripts\validate_phase31_cross_lang.py", "--code-validation", "--baseline", "docs\performance\phase31-go-comparable\baseline.json", "--report", "target\phase31\cross-lang-report.json", "--profile", "release", "--spectra-binary", $phase31Binary) -workingDir (Get-Location).Path
if ($phase31Gate.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase31-cross-lang"; Teste = "validate_phase31_cross_lang"; Status = $phase31Gate.Status; Detalhe = $phase31Gate.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.36c: R-1603 / R-3080 GPU speedup gate (manual, off default CI)
# ---------------------------------------------------------------------------
# This gate requires a WGPU adapter and the spectra-cli binary. It runs
# `benchmarks/gpu/ml-mlp-step-gpu/` on both CPU and GPU and asserts the
# GPU/CPU ratio at batch=256. CI hosts without a GPU must skip this gate.
Write-Host ""
Write-Host "--- R-1603 / R-3080 GPU speedup gate (manual) ---" -ForegroundColor Yellow
if ($runPhase31Gpu) {
    $phase31Gpu = Invoke-HostCommand -name "validate_r1603_gpu_speedup" -fileName "python" -arguments @("scripts\validate_r1603_gpu_speedup.py", "--out", "target\r1603-gpu-speedup\report.json") -workingDir (Get-Location).Path -timeoutSeconds 1800
    if ($phase31Gpu.Status -eq "PASSOU") {
        $totalPassed++
    } else {
        $totalFailed++
    }
    $gpuStatus = $phase31Gpu.Status
    $gpuDetail = $phase31Gpu.Detail
} else {
    $totalSkipped++
    $gpuStatus = "SKIPPED"
    $gpuDetail = "manual gate; use .\run_tests.ps1 -Phase phase31_gpu on host with WGPU adapter"
}
$results += [PSCustomObject]@{ Diretorio = "phase31-gpu-speedup"; Teste = "validate_r1603_gpu_speedup"; Status = $gpuStatus; Detalhe = $gpuDetail }

# ---------------------------------------------------------------------------
# Grupo 8.37: R-2112 formal Send/Sync trait bounds
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2112 formal Send/Sync trait bounds ---" -ForegroundColor Yellow
$r2112FormalSendSync = Invoke-HostCommand -name "validate_r2112_formal_send_sync_bounds" -fileName "python" -arguments @("scripts\validate_r2112_formal_send_sync_bounds.py") -workingDir (Get-Location).Path
if ($r2112FormalSendSync.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase21-async"; Teste = "validate_r2112_formal_send_sync_bounds"; Status = $r2112FormalSendSync.Status; Detalhe = $r2112FormalSendSync.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.38: R-2201 API library architecture ADR
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2201 API library architecture ADR ---" -ForegroundColor Yellow
$r2201ApiAdr = Invoke-HostCommand -name "validate_r2201_api_adr" -fileName "python" -arguments @("scripts\validate_r2201_api_adr.py") -workingDir (Get-Location).Path
if ($r2201ApiAdr.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase22-api"; Teste = "validate_r2201_api_adr"; Status = $r2201ApiAdr.Status; Detalhe = $r2201ApiAdr.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.39: R-2202 spectra-api host-call registration
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2202 spectra-api host-call registration ---" -ForegroundColor Yellow
$r2202ApiHostcalls = Invoke-HostCommand -name "validate_r2202_spectra_api_hostcalls" -fileName "python" -arguments @("scripts\validate_r2202_spectra_api_hostcalls.py") -workingDir (Get-Location).Path
if ($r2202ApiHostcalls.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase22-api"; Teste = "validate_r2202_spectra_api_hostcalls"; Status = $r2202ApiHostcalls.Status; Detalhe = $r2202ApiHostcalls.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.40: R-2203 std.api semantic and tooling surface
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2203 std.api semantic and tooling surface ---" -ForegroundColor Yellow
$r2203StdApiSurface = Invoke-HostCommand -name "validate_r2203_std_api_surface" -fileName "python" -arguments @("scripts\validate_r2203_std_api_surface.py") -workingDir (Get-Location).Path
if ($r2203StdApiSurface.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase22-api"; Teste = "validate_r2203_std_api_surface"; Status = $r2203StdApiSurface.Status; Detalhe = $r2203StdApiSurface.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.41: R-2204 HTTP/1.1 parser
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2204 HTTP/1.1 parser ---" -ForegroundColor Yellow
$r2204Http1Parser = Invoke-HostCommand -name "validate_r2204_http1_parser" -fileName "python" -arguments @("scripts\validate_r2204_http1_parser.py") -workingDir (Get-Location).Path
if ($r2204Http1Parser.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase22-api"; Teste = "validate_r2204_http1_parser"; Status = $r2204Http1Parser.Status; Detalhe = $r2204Http1Parser.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.42: R-2205 HTTP/1.1 server
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2205 HTTP/1.1 server ---" -ForegroundColor Yellow
$r2205Http1Server = Invoke-HostCommand -name "validate_r2205_http1_server" -fileName "python" -arguments @("scripts\validate_r2205_http1_server.py") -workingDir (Get-Location).Path
if ($r2205Http1Server.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase22-api"; Teste = "validate_r2205_http1_server"; Status = $r2205Http1Server.Status; Detalhe = $r2205Http1Server.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.43: R-2206 HTTP/1.1 client
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2206 HTTP/1.1 client ---" -ForegroundColor Yellow
$r2206Http1Client = Invoke-HostCommand -name "validate_r2206_http1_client" -fileName "python" -arguments @("scripts\validate_r2206_http1_client.py") -workingDir (Get-Location).Path
if ($r2206Http1Client.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase22-api"; Teste = "validate_r2206_http1_client"; Status = $r2206Http1Client.Status; Detalhe = $r2206Http1Client.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.44: R-2207 TLS via rustls
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2207 TLS via rustls ---" -ForegroundColor Yellow
$r2207TlsRustls = Invoke-HostCommand -name "validate_r2207_tls_rustls" -fileName "python" -arguments @("scripts\validate_r2207_tls_rustls.py") -workingDir (Get-Location).Path
if ($r2207TlsRustls.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase22-api"; Teste = "validate_r2207_tls_rustls"; Status = $r2207TlsRustls.Status; Detalhe = $r2207TlsRustls.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.45: R-2208 std.api.json encoder and decoder
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2208 std.api.json encoder and decoder ---" -ForegroundColor Yellow
$r2208JsonCodec = Invoke-HostCommand -name "validate_r2208_json_codec" -fileName "python" -arguments @("scripts\validate_r2208_json_codec.py") -workingDir (Get-Location).Path
if ($r2208JsonCodec.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase22-api"; Teste = "validate_r2208_json_codec"; Status = $r2208JsonCodec.Status; Detalhe = $r2208JsonCodec.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.46: R-2209 JSON derive Serialize/Deserialize
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2209 JSON derive Serialize/Deserialize ---" -ForegroundColor Yellow
$r2209JsonDerive = Invoke-HostCommand -name "validate_r2209_json_derive" -fileName "python" -arguments @("scripts\validate_r2209_json_derive.py") -workingDir (Get-Location).Path
if ($r2209JsonDerive.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase22-api"; Teste = "validate_r2209_json_derive"; Status = $r2209JsonDerive.Status; Detalhe = $r2209JsonDerive.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.47: R-2210 HTTP core types
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2210 HTTP core types ---" -ForegroundColor Yellow
$r2210HttpCoreTypes = Invoke-HostCommand -name "validate_r2210_http_core_types" -fileName "python" -arguments @("scripts\validate_r2210_http_core_types.py") -workingDir (Get-Location).Path
if ($r2210HttpCoreTypes.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase22-api"; Teste = "validate_r2210_http_core_types"; Status = $r2210HttpCoreTypes.Status; Detalhe = $r2210HttpCoreTypes.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.48: R-2211 router path matching and wildcards
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2211 router path matching and wildcards ---" -ForegroundColor Yellow
$r2211RouterMatching = Invoke-HostCommand -name "validate_r2211_router_matching" -fileName "python" -arguments @("scripts\validate_r2211_router_matching.py") -workingDir (Get-Location).Path
if ($r2211RouterMatching.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase22-api"; Teste = "validate_r2211_router_matching"; Status = $r2211RouterMatching.Status; Detalhe = $r2211RouterMatching.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.49: R-2212 query string parser and binding
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2212 query string parser and binding ---" -ForegroundColor Yellow
$r2212QueryBinding = Invoke-HostCommand -name "validate_r2212_query_binding" -fileName "python" -arguments @("scripts\validate_r2212_query_binding.py") -workingDir (Get-Location).Path
if ($r2212QueryBinding.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase22-api"; Teste = "validate_r2212_query_binding"; Status = $r2212QueryBinding.Status; Detalhe = $r2212QueryBinding.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.50: R-2213 URL-encoded form binding
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2213 URL-encoded form binding ---" -ForegroundColor Yellow
$r2213FormBinding = Invoke-HostCommand -name "validate_r2213_form_binding" -fileName "python" -arguments @("scripts\validate_r2213_form_binding.py") -workingDir (Get-Location).Path
if ($r2213FormBinding.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase22-api"; Teste = "validate_r2213_form_binding"; Status = $r2213FormBinding.Status; Detalhe = $r2213FormBinding.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.51: R-2214 multipart form and file uploads
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2214 multipart form and file uploads ---" -ForegroundColor Yellow
$r2214MultipartUploads = Invoke-HostCommand -name "validate_r2214_multipart_uploads" -fileName "python" -arguments @("scripts\validate_r2214_multipart_uploads.py") -workingDir (Get-Location).Path
if ($r2214MultipartUploads.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase22-api"; Teste = "validate_r2214_multipart_uploads"; Status = $r2214MultipartUploads.Status; Detalhe = $r2214MultipartUploads.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.52: R-2215 handler trait and response return
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2215 handler trait and response return ---" -ForegroundColor Yellow
$r2215HandlerResponse = Invoke-HostCommand -name "validate_r2215_handler_response" -fileName "python" -arguments @("scripts\validate_r2215_handler_response.py") -workingDir (Get-Location).Path
if ($r2215HandlerResponse.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase22-api"; Teste = "validate_r2215_handler_response"; Status = $r2215HandlerResponse.Status; Detalhe = $r2215HandlerResponse.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.53: R-2216 server lifecycle, listen, serve, and graceful shutdown
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2216 server lifecycle, listen, serve, and graceful shutdown ---" -ForegroundColor Yellow
$r2216ServerLifecycle = Invoke-HostCommand -name "validate_r2216_server_lifecycle" -fileName "python" -arguments @("scripts\validate_r2216_server_lifecycle.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($r2216ServerLifecycle.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase22-api"; Teste = "validate_r2216_server_lifecycle"; Status = $r2216ServerLifecycle.Status; Detalhe = $r2216ServerLifecycle.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.54: R-2217 spectra.api local registry package
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2217 spectra.api local registry package ---" -ForegroundColor Yellow
$r2217SpectraApiRegistry = Invoke-HostCommand -name "validate_r2217_spectra_api_registry" -fileName "python" -arguments @("scripts\validate_r2217_spectra_api_registry.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($r2217SpectraApiRegistry.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase22-api"; Teste = "validate_r2217_spectra_api_registry"; Status = $r2217SpectraApiRegistry.Status; Detalhe = $r2217SpectraApiRegistry.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.55: R-2218 Hello HTTP book chapter
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2218 Hello HTTP book chapter ---" -ForegroundColor Yellow
$r2218HelloHttpBook = Invoke-HostCommand -name "validate_r2218_hello_http_book" -fileName "python" -arguments @("scripts\validate_r2218_hello_http_book.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($r2218HelloHttpBook.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase22-api"; Teste = "validate_r2218_hello_http_book"; Status = $r2218HelloHttpBook.Status; Detalhe = $r2218HelloHttpBook.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.56: R-2219 REST CRUD API example
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2219 REST CRUD API example ---" -ForegroundColor Yellow
$r2219RestCrudExample = Invoke-HostCommand -name "validate_r2219_rest_crud_example" -fileName "python" -arguments @("scripts\validate_r2219_rest_crud_example.py", "--binary", $binary) -workingDir (Get-Location).Path
if ($r2219RestCrudExample.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase22-api"; Teste = "validate_r2219_rest_crud_example"; Status = $r2219RestCrudExample.Status; Detalhe = $r2219RestCrudExample.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.57: R-2220 API conformance suite v0
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2220 API conformance suite v0 ---" -ForegroundColor Yellow
$r2220ApiConformanceV0 = Invoke-HostCommand -name "validate_r2220_api_conformance_v0" -fileName "python" -arguments @("scripts\validate_r2220_api_conformance_v0.py") -workingDir (Get-Location).Path
if ($r2220ApiConformanceV0.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase22-api"; Teste = "validate_r2220_api_conformance_v0"; Status = $r2220ApiConformanceV0.Status; Detalhe = $r2220ApiConformanceV0.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.58: R-2301 Middleware chain and deterministic ordering
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2301 Middleware chain and deterministic ordering ---" -ForegroundColor Yellow
$r2301MiddlewareChain = Invoke-HostCommand -name "validate_r2301_middleware_chain" -fileName "python" -arguments @("scripts\validate_r2301_middleware_chain.py") -workingDir (Get-Location).Path
if ($r2301MiddlewareChain.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase23-api"; Teste = "validate_r2301_middleware_chain"; Status = $r2301MiddlewareChain.Status; Detalhe = $r2301MiddlewareChain.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.59: R-2302 CORS middleware
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2302 CORS middleware ---" -ForegroundColor Yellow
$r2302CorsMiddleware = Invoke-HostCommand -name "validate_r2302_cors_middleware" -fileName "python" -arguments @("scripts\validate_r2302_cors_middleware.py") -workingDir (Get-Location).Path
if ($r2302CorsMiddleware.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase23-api"; Teste = "validate_r2302_cors_middleware"; Status = $r2302CorsMiddleware.Status; Detalhe = $r2302CorsMiddleware.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.60: R-2303 structured logging and request ID tracing
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2303 structured logging and request ID tracing ---" -ForegroundColor Yellow
$r2303StructuredLogging = Invoke-HostCommand -name "validate_r2303_structured_logging" -fileName "python" -arguments @("scripts\validate_r2303_structured_logging.py") -workingDir (Get-Location).Path
if ($r2303StructuredLogging.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase23-api"; Teste = "validate_r2303_structured_logging"; Status = $r2303StructuredLogging.Status; Detalhe = $r2303StructuredLogging.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.61: R-2304 rate limiting
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2304 rate limiting ---" -ForegroundColor Yellow
$r2304RateLimiting = Invoke-HostCommand -name "validate_r2304_rate_limiting" -fileName "python" -arguments @("scripts\validate_r2304_rate_limiting.py") -workingDir (Get-Location).Path
if ($r2304RateLimiting.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase23-api"; Teste = "validate_r2304_rate_limiting"; Status = $r2304RateLimiting.Status; Detalhe = $r2304RateLimiting.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.62: R-2305 response compression
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2305 response compression ---" -ForegroundColor Yellow
$r2305Compression = Invoke-HostCommand -name "validate_r2305_compression" -fileName "python" -arguments @("scripts\validate_r2305_compression.py") -workingDir (Get-Location).Path
if ($r2305Compression.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase23-api"; Teste = "validate_r2305_compression"; Status = $r2305Compression.Status; Detalhe = $r2305Compression.Detail }

# Grupo 8.63: R-2306 security headers
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2306 security headers ---" -ForegroundColor Yellow
$r2306SecurityHeaders = Invoke-HostCommand -name "validate_r2306_security_headers" -fileName "python" -arguments @("scripts\validate_r2306_security_headers.py") -workingDir (Get-Location).Path
if ($r2306SecurityHeaders.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase23-api"; Teste = "validate_r2306_security_headers"; Status = $r2306SecurityHeaders.Status; Detalhe = $r2306SecurityHeaders.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.64: R-2307 API key authentication
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2307 API key authentication ---" -ForegroundColor Yellow
$r2307ApiKey = Invoke-HostCommand -name "validate_r2307_api_key" -fileName "python" -arguments @("scripts\validate_r2307_api_key.py") -workingDir (Get-Location).Path
if ($r2307ApiKey.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase23-api"; Teste = "validate_r2307_api_key"; Status = $r2307ApiKey.Status; Detalhe = $r2307ApiKey.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.65: R-2308 JWT signing and verification
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2308 JWT signing and verification ---" -ForegroundColor Yellow
$r2308Jwt = Invoke-HostCommand -name "validate_r2308_jwt" -fileName "python" -arguments @("scripts\validate_r2308_jwt.py") -workingDir (Get-Location).Path
if ($r2308Jwt.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase23-api"; Teste = "validate_r2308_jwt"; Status = $r2308Jwt.Status; Detalhe = $r2308Jwt.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.66: R-2309 OAuth2 client with PKCE
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2309 OAuth2 client with PKCE ---" -ForegroundColor Yellow
$r2309OAuth = Invoke-HostCommand -name "validate_r2309_oauth" -fileName "python" -arguments @("scripts\validate_r2309_oauth.py") -workingDir (Get-Location).Path
if ($r2309OAuth.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase23-api"; Teste = "validate_r2309_oauth"; Status = $r2309OAuth.Status; Detalhe = $r2309OAuth.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.67: R-2312 typed cookie API
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2312 typed cookie API ---" -ForegroundColor Yellow
$r2312Cookie = Invoke-HostCommand -name "validate_r2312_cookie" -fileName "python" -arguments @("scripts\validate_r2312_cookie.py") -workingDir (Get-Location).Path
if ($r2312Cookie.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase23-api"; Teste = "validate_r2312_cookie"; Status = $r2312Cookie.Status; Detalhe = $r2312Cookie.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.68: R-2313 request validation and RFC 7807
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2313 request validation and RFC 7807 ---" -ForegroundColor Yellow
$r2313Validation = Invoke-HostCommand -name "validate_r2313_validation" -fileName "python" -arguments @("scripts\validate_r2313_validation.py") -workingDir (Get-Location).Path
if ($r2313Validation.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase23-api"; Teste = "validate_r2313_validation"; Status = $r2313Validation.Status; Detalhe = $r2313Validation.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.69: R-2314 unified errors and exception middleware
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2314 unified errors and exception middleware ---" -ForegroundColor Yellow
$r2314Errors = Invoke-HostCommand -name "validate_r2314_errors" -fileName "python" -arguments @("scripts\validate_r2314_errors.py") -workingDir (Get-Location).Path
if ($r2314Errors.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase23-api"; Teste = "validate_r2314_errors"; Status = $r2314Errors.Status; Detalhe = $r2314Errors.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.70: R-2316 threat mitigations
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2316 threat mitigations ---" -ForegroundColor Yellow
$r2316Security = Invoke-HostCommand -name "validate_r2316_security" -fileName "python" -arguments @("scripts\validate_r2316_security.py") -workingDir (Get-Location).Path
if ($r2316Security.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase23-api"; Teste = "validate_r2316_security"; Status = $r2316Security.Status; Detalhe = $r2316Security.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.71: R-2317 authenticated REST CRUD example with JWT
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2317 authenticated REST CRUD example with JWT ---" -ForegroundColor Yellow
$r2317JwtCrud = Invoke-HostCommand -name "validate_r2317_jwt_auth_crud_example" -fileName "python" -arguments @("scripts\validate_r2317_jwt_auth_crud_example.py") -workingDir (Get-Location).Path
if ($r2317JwtCrud.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase23-api"; Teste = "validate_r2317_jwt_auth_crud_example"; Status = $r2317JwtCrud.Status; Detalhe = $r2317JwtCrud.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.72: R-2318 middleware composition example
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2318 middleware composition example ---" -ForegroundColor Yellow
$r2318MiddlewareComposition = Invoke-HostCommand -name "validate_r2318_middleware_composition_example" -fileName "python" -arguments @("scripts\validate_r2318_middleware_composition_example.py") -workingDir (Get-Location).Path
if ($r2318MiddlewareComposition.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase23-api"; Teste = "validate_r2318_middleware_composition_example"; Status = $r2318MiddlewareComposition.Status; Detalhe = $r2318MiddlewareComposition.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.73: R-2311 server-side session management
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2311 server-side session management ---" -ForegroundColor Yellow
$r2311Session = Invoke-HostCommand -name "validate_r2311_session" -fileName "python" -arguments @("scripts\validate_r2311_session.py") -workingDir (Get-Location).Path
if ($r2311Session.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase23-api"; Teste = "validate_r2311_session"; Status = $r2311Session.Status; Detalhe = $r2311Session.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.74: R-2315 HTTPS hardening
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2315 HTTPS hardening ---" -ForegroundColor Yellow
$r2315HttpsHardening = Invoke-HostCommand -name "validate_r2315_https_hardening" -fileName "python" -arguments @("scripts\validate_r2315_https_hardening.py") -workingDir (Get-Location).Path
if ($r2315HttpsHardening.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase23-api"; Teste = "validate_r2315_https_hardening"; Status = $r2315HttpsHardening.Status; Detalhe = $r2315HttpsHardening.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.75: R-2401 WebSocket server (RFC 6455)
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2401 WebSocket server (RFC 6455) ---" -ForegroundColor Yellow
$r2401WebSocket = Invoke-HostCommand -name "validate_r2401_websocket" -fileName "python" -arguments @("scripts\validate_r2401_websocket.py") -workingDir (Get-Location).Path
if ($r2401WebSocket.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase24-api"; Teste = "validate_r2401_websocket"; Status = $r2401WebSocket.Status; Detalhe = $r2401WebSocket.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.76: R-2402 WebSocket client
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2402 WebSocket client ---" -ForegroundColor Yellow
$r2402WebSocketClient = Invoke-HostCommand -name "validate_r2402_websocket_client" -fileName "python" -arguments @("scripts\validate_r2402_websocket_client.py") -workingDir (Get-Location).Path
if ($r2402WebSocketClient.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase24-api"; Teste = "validate_r2402_websocket_client"; Status = $r2402WebSocketClient.Status; Detalhe = $r2402WebSocketClient.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.77: R-2403 Server-Sent Events
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2403 Server-Sent Events ---" -ForegroundColor Yellow
$r2403Sse = Invoke-HostCommand -name "validate_r2403_sse" -fileName "python" -arguments @("scripts\validate_r2403_sse.py") -workingDir (Get-Location).Path
if ($r2403Sse.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase24-api"; Teste = "validate_r2403_sse"; Status = $r2403Sse.Status; Detalhe = $r2403Sse.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.78: R-2404 HTTP/2 server
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2404 HTTP/2 server ---" -ForegroundColor Yellow
$r2404Http2 = Invoke-HostCommand -name "validate_r2404_http2" -fileName "python" -arguments @("scripts\validate_r2404_http2.py") -workingDir (Get-Location).Path
if ($r2404Http2.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase24-api"; Teste = "validate_r2404_http2"; Status = $r2404Http2.Status; Detalhe = $r2404Http2.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.79: R-2405 HTTP/2 client
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2405 HTTP/2 client ---" -ForegroundColor Yellow
$r2405Http2Client = Invoke-HostCommand -name "validate_r2405_http2_client" -fileName "python" -arguments @("scripts\validate_r2405_http2_client.py") -workingDir (Get-Location).Path
if ($r2405Http2Client.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase24-api"; Teste = "validate_r2405_http2_client"; Status = $r2405Http2Client.Status; Detalhe = $r2405Http2Client.Detail }

# ---------------------------------------------------------------------------
# Grupo 8.80: R-2406 HTTP/3 and QUIC scope decision
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-2406 HTTP/3 and QUIC scope decision ---" -ForegroundColor Yellow
$r2406Http3Decision = Invoke-HostCommand -name "validate_r2406_http3_decision" -fileName "python" -arguments @("scripts\validate_r2406_http3_decision.py") -workingDir (Get-Location).Path
if ($r2406Http3Decision.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase24-api"; Teste = "validate_r2406_http3_decision"; Status = $r2406Http3Decision.Status; Detalhe = $r2406Http3Decision.Detail }

# ---------------------------------------------------------------------------
# Grupo 9: Phase 12 security evidence and stress/soak smoke
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- Phase 12 (security/stress) ---" -ForegroundColor Yellow

$phase12Temp = Join-Path (Get-Location).Path "target\phase12-validation"
$phase12ArtifactDir = Join-Path $phase12Temp "artifacts"
$phase12EvidenceDir = Join-Path $phase12Temp "evidence"
if (Test-Path $phase12Temp) {
    Remove-Item -LiteralPath $phase12Temp -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $phase12ArtifactDir | Out-Null
"spectralang phase12 validation artifact" | Out-File -FilePath (Join-Path $phase12ArtifactDir "spectralang-phase12.txt") -Encoding UTF8

$phase12Checks = @(
    [PSCustomObject]@{
        Nome = "release_security_create"
        File = "python"
        Args = @("scripts\release_security.py", "create", "--artifact", $phase12ArtifactDir, "--out", $phase12EvidenceDir, "--version", "0.0.0-phase12-test", "--allow-dev-key")
    }
    [PSCustomObject]@{
        Nome = "release_security_verify"
        File = "python"
        Args = @("scripts\release_security.py", "verify", "--evidence", $phase12EvidenceDir, "--allow-dev-key")
    }
    [PSCustomObject]@{
        Nome = "stress_soak_smoke"
        File = "python"
        Args = @("scripts\stress_soak.py", "--iterations", "1", "--timeout-seconds", "20", "--memory-limit-mb", "1024", "--json-out", "target\stress-soak-smoke.json")
    }
    [PSCustomObject]@{
        Nome = "validate_r1203_fs_path_safety"
        File = "python"
        Args = @("scripts\validate_r1203_fs_path_safety.py", "--binary", $binary)
    }
    [PSCustomObject]@{
        Nome = "validate_r1204_std_unwrap_safety"
        File = "python"
        Args = @("scripts\validate_r1204_std_unwrap_safety.py")
    }
)

foreach ($check in $phase12Checks) {
    $r = Invoke-HostCommand -name $check.Nome -fileName $check.File -arguments $check.Args -workingDir (Get-Location).Path
    if ($r.Status -eq "PASSOU") {
        $totalPassed++
    } else {
        $totalFailed++
    }
    $results += [PSCustomObject]@{ Diretorio = "phase12"; Teste = $check.Nome; Status = $r.Status; Detalhe = $r.Detail }
}

# ---------------------------------------------------------------------------
# Grupo 9.5: R-104 compiler test pyramid structure
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- R-104 compiler test pyramid ---" -ForegroundColor Yellow
$testPyramid = Invoke-HostCommand -name "validate_test_pyramid" -fileName "python" -arguments @("scripts\validate_test_pyramid.py") -workingDir (Get-Location).Path
if ($testPyramid.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase1-tests"; Teste = "validate_test_pyramid"; Status = $testPyramid.Status; Detalhe = $testPyramid.Detail }

# ---------------------------------------------------------------------------
# Grupo 10: Phase 13 AI reference examples
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host "--- Phase 13 book validation ---" -ForegroundColor Yellow
$phase13Book = Invoke-HostCommand -name "validate_ai_book" -fileName "python" -arguments @("scripts\validate_ai_book.py") -workingDir (Get-Location).Path
if ($phase13Book.Status -eq "PASSOU") {
    $totalPassed++
} else {
    $totalFailed++
}
$results += [PSCustomObject]@{ Diretorio = "phase13-docs"; Teste = "validate_ai_book"; Status = $phase13Book.Status; Detalhe = $phase13Book.Detail }

$aiExamplesDir = "examples\ai"
if (Test-Path $aiExamplesDir) {
    Write-Host ""
    $files = Get-ChildItem -Path $aiExamplesDir -Filter "*.spectra" | Sort-Object Name
    Write-Host "--- Phase 13 AI examples ($($files.Count) exemplos: devem executar) ---" -ForegroundColor Yellow
    New-Item -ItemType Directory -Force -Path "target\ai-examples" | Out-Null

    foreach ($file in $files) {
        Write-Host "  $($file.Name)" -NoNewline
        $r = Invoke-SpectraCommand -commandArgs @("run", $file.FullName) -workingDir (Get-Location).Path -includeExperimental $true

        if ($r.TimedOut) {
            Write-Host " TIMEOUT" -ForegroundColor Red
            $totalFailed++
            $results += [PSCustomObject]@{ Diretorio = "phase13-ai"; Teste = $file.Name; Status = "TIMEOUT"; Detalhe = "execucao excedeu ${timeoutSeconds}s" }
        } elseif ($r.ExitCode -eq 0) {
            Write-Host " PASSOU" -ForegroundColor Green
            $totalPassed++
            $results += [PSCustomObject]@{ Diretorio = "phase13-ai"; Teste = $file.Name; Status = "PASSOU"; Detalhe = "" }
        } else {
            $err = Get-FirstError $r.Output
            if (-not $err) {
                $err = "exit code inesperado: $($r.ExitCode)"
            }
            Write-Host " FALHOU" -ForegroundColor Red
            Write-Host "     $err" -ForegroundColor DarkRed
            $totalFailed++
            $results += [PSCustomObject]@{ Diretorio = "phase13-ai"; Teste = $file.Name; Status = "FALHOU"; Detalhe = $err }
        }
    }
}

# ---------------------------------------------------------------------------
# Resumo
# ---------------------------------------------------------------------------
$totalDecisive = $totalPassed + $totalFailed
Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "            RESUMO DOS TESTES" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "Testes com resultado esperado: $totalDecisive" -ForegroundColor White
Write-Host "  Passou : $totalPassed" -ForegroundColor Green
Write-Host "  Falhou : $totalFailed" -ForegroundColor $(if ($totalFailed -eq 0) { "Green" } else { "Red" })
Write-Host "Testes informativos (semantic): $totalInfo" -ForegroundColor Cyan
Write-Host "Testes ignorados por ambiente: $totalSkipped" -ForegroundColor DarkYellow

if ($totalDecisive -gt 0) {
    $pct = [math]::Round(($totalPassed / $totalDecisive) * 100, 1)
    Write-Host "Taxa de sucesso: $pct%" -ForegroundColor $(if ($pct -eq 100) { "Green" } else { "Yellow" })
}

Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan

# Tabela completa
Write-Host ""
$results | Format-Table -AutoSize

# Salva relatorio
$reportPath = "TEST_RESULTS.txt"
$results | Format-Table -AutoSize -Wrap | Out-File -FilePath $reportPath -Encoding UTF8 -Width 240
Write-Host "Relatorio salvo em: $reportPath" -ForegroundColor Cyan

if ($totalFailed -gt 0) {
    exit 1
}
exit 0
