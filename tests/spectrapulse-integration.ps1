param(
    [string]$Executable = "target\spectrapulse-integration.exe",
    [string]$Spectralang = "target\debug\spectralang.exe"
)

$ErrorActionPreference = "Stop"
$root = (Get-Location).Path
$script:cliPath = [System.IO.Path]::GetFullPath((Join-Path $root $Spectralang))
$script:exePath = [System.IO.Path]::GetFullPath((Join-Path $root $Executable))
$script:projectPath = [System.IO.Path]::GetFullPath((Join-Path $root "examples\complete\23-spectrapulse"))

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
        throw ("SpectraPulse AOT retornou {0}.{1}{2}" -f $exitCode, [Environment]::NewLine, $output)
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
Invoke-Cli @("check", "--json", $script:projectPath) | Out-Null
Invoke-Cli @("compile", "--debug-info=none", "--emit-exe", $script:exePath, $script:projectPath) | Out-Null

$jit = Invoke-Cli @("run", $script:projectPath)
$aot = Invoke-Aot
foreach ($output in @($jit, $aot)) {
    Assert-Contains $output "SPECTRAPULSE ok" "execução deve completar previsão multivariada"
    Assert-Contains $output '"rmse"' "avaliação deve informar RMSE"
    Assert-Contains $output "examples=6" "holdout deve conter seis exemplos"
}

"spectrapulse JIT + AOT integration: ok"
