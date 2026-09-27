param(
    [string]$Executable = "target\spectrarelease-integration.exe",
    [string]$Spectralang = "target\debug\spectralang.exe"
)

$ErrorActionPreference = "Stop"
$root = (Get-Location).Path
$script:cliPath = [System.IO.Path]::GetFullPath((Join-Path $root $Spectralang))
$script:exePath = [System.IO.Path]::GetFullPath((Join-Path $root $Executable))
$script:projectPath = [System.IO.Path]::GetFullPath((Join-Path $root "examples\complete\28-spectrarelease"))

if (-not (Test-Path -LiteralPath $script:cliPath -PathType Leaf)) {
    throw "CLI não encontrada: $script:cliPath. Compile cargo build -p spectra-cli primeiro."
}

function Invoke-Cli {
    param([string[]]$Arguments)
    $lines = @(& $script:cliPath @Arguments 2>&1)
    $exitCode = $LASTEXITCODE
    $output = ($lines | ForEach-Object { [string]$_ }) -join [Environment]::NewLine
    if ($exitCode -ne 0) {
        throw ("spectralang {0} retornou {1}.{2}{3}" -f ($Arguments -join ' '), $exitCode, [Environment]::NewLine, $output)
    }
    return $output
}

function Invoke-Aot {
    $lines = @(& $script:exePath 2>&1)
    $exitCode = $LASTEXITCODE
    $output = ($lines | ForEach-Object { [string]$_ }) -join [Environment]::NewLine
    if ($exitCode -ne 0) {
        throw ("SpectraRelease AOT retornou {0}.{1}{2}" -f $exitCode, [Environment]::NewLine, $output)
    }
    return $output
}

function Assert-Contains {
    param([string]$Text, [string]$Expected, [string]$Message)
    if (-not $Text.Contains($Expected, [System.StringComparison]::Ordinal)) {
        throw ("{0}{1}Esperado: {2}{1}Saída:{1}{3}" -f $Message, [Environment]::NewLine, $Expected, $Text)
    }
}

Invoke-Cli @("fmt", "--check", $script:projectPath) | Out-Null
$check = Invoke-Cli @("check", "--json", $script:projectPath) | ConvertFrom-Json
if (-not $check.success) {
    throw "spectralang check reportou falha semântica no projeto SpectraRelease."
}
Invoke-Cli @("lint", $script:projectPath) | Out-Null
Invoke-Cli @("compile", "--debug-info=none", "--emit-exe", $script:exePath, $script:projectPath) | Out-Null

$jit = Invoke-Cli @("run", $script:projectPath)
$aot = Invoke-Aot
if ($jit -cne $aot) {
    throw ("saídas JIT e AOT divergiram.{0}JIT:{0}{1}{0}AOT:{0}{2}" -f [Environment]::NewLine, $jit, $aot)
}
foreach ($output in @($jit, $aot)) {
    Assert-Contains $output "SPECTRARELEASE ok" "a pipeline de release deve concluir"
    Assert-Contains $output "model=mlp-residual-layernorm-dropout autodiff=adamw" "a saída deve identificar o modelo treinado"
    Assert-Contains $output "parameters=8 steps=240" "o treino deve usar oito tensores de parâmetros e executar 240 atualizações"
    Assert-Contains $output "holdout_mse=" "a avaliação deve relatar erro no holdout"
    Assert-Contains $output "shifted_drift=" "a avaliação deve relatar drift sob mudança de distribuição"
    Assert-Contains $output "rejection: risk=risk-limit drift=feature-drift-limit rollback=true" "as políticas devem rejeitar e reverter os candidatos inválidos"
}

"spectrarelease JIT + AOT integration: ok"
