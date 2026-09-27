param(
    [string]$Executable = "target\spectraquant-integration.exe",
    [string]$Spectralang = "target\debug\spectralang.exe"
)

$ErrorActionPreference = "Stop"
$root = (Get-Location).Path
$script:cliPath = [System.IO.Path]::GetFullPath((Join-Path $root $Spectralang))
$script:exePath = [System.IO.Path]::GetFullPath((Join-Path $root $Executable))
$script:projectPath = [System.IO.Path]::GetFullPath((Join-Path $root "examples\complete\29-spectraquant"))
$script:regressionPath = [System.IO.Path]::GetFullPath((Join-Path $root "tests\validation\622_enum_structural_equality.spectra"))

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
        throw ("SpectraQuant AOT retornou {0}.{1}{2}" -f $exitCode, [Environment]::NewLine, $output)
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
Invoke-Cli @("fmt", "--check", $script:regressionPath) | Out-Null
$projectCheck = Invoke-Cli @("check", "--json", $script:projectPath) | ConvertFrom-Json
if (-not $projectCheck.success) {
    throw "spectralang check reportou falha semântica no SpectraQuant."
}
$regressionCheck = Invoke-Cli @("check", "--json", $script:regressionPath) | ConvertFrom-Json
if (-not $regressionCheck.success) {
    throw "spectralang check reportou falha na regressão de igualdade estrutural."
}
Invoke-Cli @("lint", $script:projectPath) | Out-Null
Invoke-Cli @("lint", $script:regressionPath) | Out-Null
Invoke-Cli @("run", $script:regressionPath) | Out-Null
Invoke-Cli @("compile", "--debug-info=none", "--emit-exe", $script:exePath, $script:projectPath) | Out-Null

$jit = Invoke-Cli @("run", $script:projectPath)
$aot = Invoke-Aot
if ($jit -cne $aot) {
    throw ("saídas JIT e AOT divergiram.{0}JIT:{0}{1}{0}AOT:{0}{2}" -f [Environment]::NewLine, $jit, $aot)
}
foreach ($output in @($jit, $aot)) {
    Assert-Contains $output "SPECTRAQUANT ok model=monte-carlo-gbm autodiff=pathwise-greeks samples=512" "a simulação deve concluir com os 512 cenários"
    Assert-Contains $output "call: fair_value=" "a cotação de call deve relatar valor e Greeks"
    Assert-Contains $output "put: fair_value=" "a cotação de put deve relatar valor e Greeks"
    Assert-Contains $output "risk: var95_cents=" "o relatório deve incluir VaR, CVaR e perda extrema"
    Assert-Contains $output "policy: accepted=true strict_rejection=var-limit" "as políticas devem aprovar e rejeitar o risco esperado"
}

"spectraquant JIT + AOT integration: ok"
