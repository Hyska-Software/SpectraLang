param(
    [string]$Executable = "target\spectravision-integration.exe",
    [string]$Spectralang = "target\debug\spectralang.exe"
)

$ErrorActionPreference = "Stop"
$root = (Get-Location).Path
$script:cliPath = [System.IO.Path]::GetFullPath((Join-Path $root $Spectralang))
$script:exePath = [System.IO.Path]::GetFullPath((Join-Path $root $Executable))
$script:projectPath = [System.IO.Path]::GetFullPath((Join-Path $root "examples\complete\22-spectravision"))

if (-not (Test-Path -LiteralPath $script:cliPath -PathType Leaf)) {
    throw "CLI não encontrada: $script:cliPath. Compile cargo build -p spectra-cli primeiro."
}

function Invoke-Cli {
    param([string[]]$Arguments)
    $lines = @(& $script:cliPath @Arguments 2>&1)
    $exitCode = $LASTEXITCODE
    $output = ($lines | ForEach-Object { [string]$_ }) -join "`n"
    if ($exitCode -ne 0) {
        throw "spectralang $($Arguments -join ' ') retornou $exitCode.`n$output"
    }
    return $output
}

function Invoke-Aot {
    $lines = @(& $script:exePath 2>&1)
    $exitCode = $LASTEXITCODE
    $output = ($lines | ForEach-Object { [string]$_ }) -join "`n"
    if ($exitCode -ne 0) {
        throw "SpectraVision AOT retornou $exitCode.`n$output"
    }
    return $output
}

function Assert-Contains {
    param([string]$Text, [string]$Expected, [string]$Message)
    if (-not $Text.Contains($Expected, [System.StringComparison]::Ordinal)) {
        throw "$Message`nEsperado: $Expected`nSaída:`n$Text"
    }
}

Invoke-Cli @("fmt", "--check", $script:projectPath) | Out-Null
Invoke-Cli @("check", "--json", $script:projectPath) | Out-Null
Invoke-Cli @("compile", "--debug-info=none", "--emit-exe", $script:exePath, $script:projectPath) | Out-Null

$jit = Invoke-Cli @("run", $script:projectPath)
$aot = Invoke-Aot
foreach ($output in @($jit, $aot)) {
    Assert-Contains $output "SPECTRAVISION ok" "execução deve completar o pipeline CNN"
    Assert-Contains $output '"accuracy":1' "teste deve reportar acurácia 1.0"
    Assert-Contains $output '"f1":1' "teste deve reportar F1 1.0"
    Assert-Contains $output "reload=verified" "pesos recarregados devem preservar previsões"
}

"spectravision JIT + AOT integration: ok"
