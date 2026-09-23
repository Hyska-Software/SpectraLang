param(
    [string]$Executable = "target\spectraboard-dev.exe"
)

$ErrorActionPreference = "Stop"
$root = (Get-Location).Path
$exePath = [System.IO.Path]::GetFullPath((Join-Path $root $Executable))
if (-not (Test-Path -LiteralPath $exePath -PathType Leaf)) {
    throw "Executável não encontrado: $exePath. Compile examples/complete/19-spectraboard primeiro."
}

function Invoke-SpectraBoard {
    param(
        [string[]]$Arguments,
        [int]$ExpectedExitCode = 0
    )
    $lines = @(& $script:exePath @Arguments 2>&1)
    $exitCode = $LASTEXITCODE
    $output = ($lines | ForEach-Object { [string]$_ }) -join "`n"
    if ($exitCode -ne $ExpectedExitCode) {
        throw "spectraboard $($Arguments -join ' ') retornou $exitCode; esperado $ExpectedExitCode.`n$output"
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
$workspace = Join-Path $tempRoot ("spectraboard-test-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $workspace | Out-Null
Push-Location $workspace
try {
    Assert-Contains (Invoke-SpectraBoard @("--help")) "spectraboard init" "help deve listar o comando init"
    Assert-Contains (Invoke-SpectraBoard @("unknown") 64) "comando desconhecido" "comando inválido deve retornar erro de uso"
    Assert-Contains (Invoke-SpectraBoard @("list") 1) "banco ausente" "list deve explicar como inicializar o banco"
    Assert-Contains (Invoke-SpectraBoard @("init")) "inicializado" "init deve criar o banco local"
    Assert-Contains (Invoke-SpectraBoard @("init") 1) "já foi inicializado" "init repetido deve preservar o banco"
    Assert-Contains (Invoke-SpectraBoard @("self-test")) "9 verificações do domínio" "self-test deve cobrir domínio e serialização"

    $created = Invoke-SpectraBoard @("add", "--title", "Corrigir resolução de módulos / Ω", "--priority", "high", "--due", "2026-10-01", "--tag", "compiler")
    Assert-Contains $created "Tarefa #1 criada" "add deve criar uma tarefa com ID estável"
    $details = Invoke-SpectraBoard @("show", "1")
    Assert-Contains $details "Corrigir resolução de módulos / Ω" "show deve carregar UTF-8 persistido"
    Assert-Contains $details "Prazo: 2026-10-01" "show deve exibir o prazo"
    Assert-Contains (Invoke-SpectraBoard @("list", "--status", "open", "--priority", "high", "--tag", "compiler")) "#1 [ ] [high]" "list deve combinar filtros"

    $databasePath = Join-Path $workspace ".spectraboard\tasks.db"
    $backupPath = $databasePath + ".bak"
    Move-Item -LiteralPath $databasePath -Destination $backupPath
    $recovered = Invoke-SpectraBoard @("list")
    Assert-Contains $recovered "Corrigir resolução de módulos / Ω" "a próxima execução deve recuperar backup após interrupção"
    Assert-True (Test-Path -LiteralPath $databasePath -PathType Leaf) "recuperação deve republicar tasks.db"
    Assert-True (-not (Test-Path -LiteralPath $backupPath)) "recuperação deve remover o backup já restaurado"

    Assert-Contains (Invoke-SpectraBoard @("edit", "1", "--title", "Revisar imports aninhados", "--priority", "normal", "--due", "none", "--tag", "language")) "Tarefa #1 atualizada" "edit deve persistir campos alterados"
    $filtered = Invoke-SpectraBoard @("list", "--status", "open", "--priority", "normal", "--tag", "language")
    Assert-Contains $filtered "Revisar imports aninhados" "list deve refletir os campos editados"
    Assert-Contains $filtered "sem prazo @language" "edit --due none deve limpar o prazo"
    Assert-Contains (Invoke-SpectraBoard @("summary")) "Alta prioridade abertas: 0" "summary deve contar estado e prioridade atuais"

    Assert-Contains (Invoke-SpectraBoard @("complete", "1")) "marcada como done" "complete deve atualizar o estado"
    Assert-Contains (Invoke-SpectraBoard @("list", "--status", "open")) "Nenhuma tarefa encontrada" "filtro open deve ocultar tarefa concluída"
    Assert-Contains (Invoke-SpectraBoard @("show", "1")) "Estado: done" "show deve sobreviver a outro processo"
    Assert-Contains (Invoke-SpectraBoard @("complete", "1")) "já está done" "complete repetido deve ser idempotente"
    Assert-Contains (Invoke-SpectraBoard @("reopen", "1")) "marcada como open" "reopen deve devolver a tarefa ao fluxo"

    Assert-Contains (Invoke-SpectraBoard @("add", "--title", "Tarefa auxiliar 100%", "--tag", "cleanup")) "Tarefa #2 criada" "add deve persistir o próximo ID"
    Assert-Contains (Invoke-SpectraBoard @("remove", "2")) "Tarefa #2 removida" "remove deve excluir somente o registro indicado"
    Assert-Contains (Invoke-SpectraBoard @("add", "--title", "ID não reutilizado")) "Tarefa #3 criada" "IDs removidos não devem ser reutilizados"
    Assert-Contains (Invoke-SpectraBoard @("add", "--title", "Data impossível", "--due", "2026-02-30") 1) "prazo inválido" "add deve rejeitar datas que não existem"
    Assert-Contains (Invoke-SpectraBoard @("list", "--status", "finished") 64) "status inválido" "parser deve rejeitar filtros desconhecidos"
    Assert-Contains (Invoke-SpectraBoard @("show", "999") 1) "tarefa não encontrada" "show deve explicar IDs ausentes"

    $validDatabase = [System.IO.File]::ReadAllText($databasePath)
    Write-Utf8 ".spectraboard\tasks.db" "conteúdo corrompido`n"
    Assert-Contains (Invoke-SpectraBoard @("list") 1) "cabeçalho do banco" "banco corrompido deve ser rejeitado sem apagar dados silenciosamente"
    [System.IO.File]::WriteAllText($databasePath, $validDatabase, [System.Text.UTF8Encoding]::new($false))
    Assert-Contains (Invoke-SpectraBoard @("list")) "ID não reutilizado" "dados restaurados devem continuar legíveis"

    "spectraboard integration: ok"
}
finally {
    Pop-Location
    $resolvedWorkspace = [System.IO.Path]::GetFullPath($workspace)
    $resolvedTempRoot = [System.IO.Path]::GetFullPath($tempRoot)
    $safePrefix = $resolvedTempRoot.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
    if ($resolvedWorkspace.StartsWith($safePrefix, [System.StringComparison]::OrdinalIgnoreCase) -and (Split-Path -Leaf $resolvedWorkspace).StartsWith("spectraboard-test-", [System.StringComparison]::Ordinal)) {
        Remove-Item -LiteralPath $resolvedWorkspace -Recurse -Force
    } else {
        throw "Recusa de limpeza fora da pasta temporária controlada: $resolvedWorkspace"
    }
}
