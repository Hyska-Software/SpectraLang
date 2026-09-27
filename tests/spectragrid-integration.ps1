param(
    [string]$Executable = "target\spectragrid-integration.exe",
    [string]$Spectralang = "target\debug\spectralang.exe"
)

$ErrorActionPreference = "Stop"
$root = (Get-Location).Path
$script:cliPath = [System.IO.Path]::GetFullPath((Join-Path $root $Spectralang))
$script:exePath = [System.IO.Path]::GetFullPath((Join-Path $root $Executable))
$script:projectPath = [System.IO.Path]::GetFullPath((Join-Path $root "examples\complete\30-spectragrid"))

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
        throw ("SpectraGrid AOT retornou {0}.{1}{2}" -f $exitCode, [Environment]::NewLine, $output)
    }
    return $output
}

function Assert-Contains {
    param([string]$Text, [string]$Expected, [string]$Message)
    if ($Text.IndexOf($Expected, [System.StringComparison]::Ordinal) -lt 0) {
        throw ("{0}{1}Esperado: {2}{1}Saída:{1}{3}" -f $Message, [Environment]::NewLine, $Expected, $Text)
    }
}

Invoke-Cli @("fmt", "--check", $script:projectPath) | Out-Null
$check = Invoke-Cli @("check", "--json", $script:projectPath) | ConvertFrom-Json
if (-not $check.success) {
    throw "spectralang check reportou falha semântica no SpectraGrid."
}
Invoke-Cli @("lint", $script:projectPath) | Out-Null
Invoke-Cli @("compile", "--debug-info=none", "--emit-exe", $script:exePath, $script:projectPath) | Out-Null

$jit = Invoke-Cli @("run", $script:projectPath)
$aot = Invoke-Aot
if ($jit -cne $aot) {
    throw ("saídas JIT e AOT divergiram.{0}JIT:{0}{1}{0}AOT:{0}{2}" -f [Environment]::NewLine, $jit, $aot)
}
foreach ($output in @($jit, $aot)) {
    Assert-Contains $output "SPECTRAGRID ok forecast=mlp-residual-layernorm autodiff=adamw algorithm=merit-order-battery-dispatch" "a rede e o despacho devem concluir"
    Assert-Contains $output "forecast: steps=180" "o modelo deve executar as atualizações planejadas"
    Assert-Contains $output "holdout_mse=" "a avaliação deve relatar o erro no holdout"
    Assert-Contains $output "stress: unserved_kwh=189 status=load-shed" "o cenário isolado deve registrar corte de carga"
    Assert-Contains $output "ensemble: low_cost_cents=7040 high_cost_cents=9890 delta_cents=2850 tasks=2 channel_drained=true" "a análise concorrente deve coletar os dois cenários"
}

"spectragrid JIT + AOT integration: ok"
