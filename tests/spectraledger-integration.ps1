param(
    [string]$Executable = "target\spectraledger-dev.exe",
    [string]$Spectralang = "target\debug\spectralang.exe"
)

$ErrorActionPreference = "Stop"
$root = (Get-Location).Path
$exePath = [System.IO.Path]::GetFullPath((Join-Path $root $Executable))
$script:cliPath = [System.IO.Path]::GetFullPath((Join-Path $root $Spectralang))
$script:projectPath = [System.IO.Path]::GetFullPath((Join-Path $root "examples\complete\20-spectraledger"))
if (-not (Test-Path -LiteralPath $exePath -PathType Leaf)) {
    throw "Executável não encontrado: $exePath. Compile examples/complete/20-spectraledger primeiro."
}
if (-not (Test-Path -LiteralPath $script:cliPath -PathType Leaf)) {
    throw "CLI não encontrada: $script:cliPath. Compile cargo build -p spectra-cli primeiro."
}

function Invoke-SpectraLedger {
    param(
        [string[]]$Arguments,
        [int]$ExpectedExitCode = 0
    )
    $lines = @(& $script:exePath @Arguments 2>&1)
    $exitCode = $LASTEXITCODE
    $output = ($lines | ForEach-Object { [string]$_ }) -join "`n"
    if ($exitCode -ne $ExpectedExitCode) {
        throw "spectraledger $($Arguments -join ' ') retornou $exitCode; esperado $ExpectedExitCode.`n$output"
    }
    return $output
}

function Invoke-SpectraLedgerJit {
    param(
        [string[]]$Arguments,
        [int]$ExpectedExitCode = 0
    )
    $lines = @(& $script:cliPath run $script:projectPath -- @Arguments 2>&1)
    $exitCode = $LASTEXITCODE
    $output = ($lines | ForEach-Object { [string]$_ }) -join "`n"
    if ($exitCode -ne $ExpectedExitCode) {
        throw "spectralang run spectraledger $($Arguments -join ' ') retornou $exitCode; esperado $ExpectedExitCode.`n$output"
    }
    return $output
}

function Assert-True {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) {
        throw $Message
    }
}

function Assert-Contains {
    param([string]$Text, [string]$Expected, [string]$Message)
    if (-not $Text.Contains($Expected, [System.StringComparison]::Ordinal)) {
        throw "$Message`nEsperado: $Expected`nSaída:`n$Text"
    }
}

function Write-Utf8 {
    param([string]$Path, [string]$Text)
    $absolutePath = [System.IO.Path]::GetFullPath((Join-Path (Get-Location).Path $Path))
    $parent = Split-Path -Parent $absolutePath
    if ($parent -and -not (Test-Path -LiteralPath $parent -PathType Container)) {
        New-Item -ItemType Directory -Path $parent | Out-Null
    }
    [System.IO.File]::WriteAllText($absolutePath, $Text, [System.Text.UTF8Encoding]::new($false))
}

$tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$workspace = Join-Path $tempRoot ("spectraledger-test-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $workspace | Out-Null
Push-Location $workspace
try {
    Assert-Contains (Invoke-SpectraLedger @("--help")) "spectraledger init" "help deve listar init"
    Assert-Contains (Invoke-SpectraLedger @("unknown") 64) "comando desconhecido" "comando desconhecido deve retornar erro de uso"
    Assert-Contains (Invoke-SpectraLedger @("list") 1) "ledger ausente" "list deve explicar como inicializar o ledger"
    Assert-Contains (Invoke-SpectraLedger @("add", "expense", "2026-09-01", "1.005", "food", "invalid") 64) "valor inválido" "mais de duas casas decimais devem ser recusadas"
    Assert-Contains (Invoke-SpectraLedger @("init")) "inicializado" "init deve criar o ledger"
    Assert-Contains (Invoke-SpectraLedger @("init") 1) "já foi inicializado" "init repetido deve preservar os dados"
    Assert-Contains (Invoke-SpectraLedger @("self-test")) "10 verificações passaram" "self-test deve cobrir regras e serialização"

    Assert-Contains (Invoke-SpectraLedger @("add", "expense", "2026-09-03", "25.90", "groceries", "Mercado Ω 100%")) "Transação #1 registrada" "add deve criar despesa com ID estável"
    Assert-Contains (Invoke-SpectraLedger @("add", "income", "2026-09-05", "2500.00", "salary", "Salário mensal")) "Transação #2 registrada" "add deve criar receita em outro processo"
    Assert-Contains (Invoke-SpectraLedger @("add", "expense", "2026-09-06", "7.99", "groceries", "Padaria")) "Transação #3 registrada" "add deve acrescentar uma segunda despesa"
    Assert-Contains (Invoke-SpectraLedger @("add", "expense", "2026-08-31", "12.00", "transport", "Ônibus")) "Transação #4 registrada" "add deve persistir uma data de outro mês"

    $filtered = Invoke-SpectraLedger @("list", "--month", "2026-09", "--kind", "expense", "--category", "groceries")
    Assert-Contains $filtered "Mercado Ω 100%" "filtros combinados devem carregar texto UTF-8 escapado"
    Assert-Contains $filtered "Padaria" "filtros devem manter as despesas correspondentes"
    Assert-True (-not $filtered.Contains("Salário mensal")) "filtro expense não deve retornar receitas"
    Assert-True (-not $filtered.Contains("Ônibus")) "filtro mensal deve excluir transações de outro mês"

    Assert-Contains (Invoke-SpectraLedger @("budget", "set", "2026-09", "groceries", "30.00")) "Orçamento de groceries" "budget set deve criar um limite"
    $summary = Invoke-SpectraLedger @("summary", "2026-09")
    Assert-Contains $summary "Receitas: R$ 2500,00" "summary deve somar receitas em centavos"
    Assert-Contains $summary "Despesas: R$ 33,89" "summary deve somar despesas sem ponto flutuante"
    Assert-Contains $summary "Saldo: R$ 2466,11" "summary deve calcular saldo"
    Assert-Contains $summary "Orçamentos excedidos: 1" "summary deve sinalizar limite excedido"
    Assert-Contains (Invoke-SpectraLedger @("budget", "list", "2026-09")) "gasto=R$ 33,89" "budget list deve calcular gasto por categoria"
    Assert-Contains (Invoke-SpectraLedger @("budget", "set", "2026-09", "groceries", "40.00")) "R$ 40,00" "budget set deve substituir o limite anterior"
    Assert-Contains (Invoke-SpectraLedger @("budget", "list", "2026-09")) "[dentro do limite]" "limite atualizado deve refletir o novo estado"

    Assert-Contains (Invoke-SpectraLedger @("remove", "3")) "Transação #3 removida" "remove deve excluir apenas a transação escolhida"
    Assert-Contains (Invoke-SpectraLedger @("add", "income", "2026-09-15", "0.01", "refund", "Ajuste")) "Transação #5 registrada" "IDs removidos não devem ser reutilizados"

    $databasePath = Join-Path $workspace ".spectraledger\ledger.db"
    $backupPath = $databasePath + ".bak"
    Move-Item -LiteralPath $databasePath -Destination $backupPath
    Assert-Contains (Invoke-SpectraLedger @("list")) "Mercado Ω 100%" "próxima execução deve restaurar o backup"
    Assert-True (Test-Path -LiteralPath $databasePath -PathType Leaf) "recuperação deve republicar ledger.db"
    Assert-True (-not (Test-Path -LiteralPath $backupPath)) "recuperação deve remover backup restaurado"

    $validDatabase = [System.IO.File]::ReadAllText($databasePath)
    Write-Utf8 ".spectraledger\ledger.db" "conteúdo corrompido`n"
    Assert-Contains (Invoke-SpectraLedger @("list") 1) "cabeçalho do ledger" "ledger corrompido deve ser recusado"
    [System.IO.File]::WriteAllText($databasePath, $validDatabase, [System.Text.UTF8Encoding]::new($false))
    Assert-Contains (Invoke-SpectraLedger @("summary", "2026-09")) "Orçamentos excedidos: 0" "restaurar o banco válido deve permitir nova execução"

}
finally {
    Pop-Location
    $resolvedWorkspace = [System.IO.Path]::GetFullPath($workspace)
    $resolvedTempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    $safePrefix = $resolvedTempRoot.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
    if ($resolvedWorkspace.StartsWith($safePrefix, [System.StringComparison]::OrdinalIgnoreCase) -and (Split-Path -Leaf $resolvedWorkspace).StartsWith("spectraledger-test-", [System.StringComparison]::Ordinal)) {
        Remove-Item -LiteralPath $resolvedWorkspace -Recurse -Force
    } else {
        throw "Recusa de limpeza fora da pasta temporária controlada: $resolvedWorkspace"
    }
}

$jitWorkspace = Join-Path $tempRoot ("spectraledger-jit-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $jitWorkspace | Out-Null
Push-Location $jitWorkspace
try {
    Assert-Contains (Invoke-SpectraLedgerJit @("init")) "inicializado" "JIT deve inicializar o ledger em outro diretório"
    Assert-Contains (Invoke-SpectraLedgerJit @("add", "expense", "2026-09-10", "42.01", "groceries", "Almoço de teste")) "Transação #1 registrada" "JIT deve gravar uma transação"
    $jitSummary = Invoke-SpectraLedgerJit @("summary", "2026-09")
    Assert-Contains $jitSummary "Despesas: R$ 42,01" "JIT deve ler e somar a transação gravada"
    Assert-Contains $jitSummary "Saldo: R$ -42,01" "JIT deve calcular saldo negativo"
}
finally {
    Pop-Location
    $resolvedJitWorkspace = [System.IO.Path]::GetFullPath($jitWorkspace)
    $resolvedTempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    $safePrefix = $resolvedTempRoot.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
    if ($resolvedJitWorkspace.StartsWith($safePrefix, [System.StringComparison]::OrdinalIgnoreCase) -and (Split-Path -Leaf $resolvedJitWorkspace).StartsWith("spectraledger-jit-", [System.StringComparison]::Ordinal)) {
        Remove-Item -LiteralPath $resolvedJitWorkspace -Recurse -Force
    } else {
        throw "Recusa de limpeza fora da pasta temporária controlada: $resolvedJitWorkspace"
    }
}

"spectraledger JIT + AOT integration: ok"
