param(
    [string]$Executable = "target\spectrahabit-dev.exe",
    [string]$Spectralang = "target\debug\spectralang.exe"
)

$ErrorActionPreference = "Stop"
$root = (Get-Location).Path
$exePath = [System.IO.Path]::GetFullPath((Join-Path $root $Executable))
$script:cliPath = [System.IO.Path]::GetFullPath((Join-Path $root $Spectralang))
$script:projectPath = [System.IO.Path]::GetFullPath((Join-Path $root "examples\complete\21-spectrahabit"))
if (-not (Test-Path -LiteralPath $exePath -PathType Leaf)) {
    throw "Executável não encontrado: $exePath. Compile examples/complete/21-spectrahabit primeiro."
}
if (-not (Test-Path -LiteralPath $script:cliPath -PathType Leaf)) {
    throw "CLI não encontrada: $script:cliPath. Compile cargo build -p spectra-cli primeiro."
}

function Invoke-SpectraHabit {
    param([string[]]$Arguments, [int]$ExpectedExitCode = 0)
    $lines = @(& $script:exePath @Arguments 2>&1)
    $exitCode = $LASTEXITCODE
    $output = ($lines | ForEach-Object { [string]$_ }) -join "`n"
    if ($exitCode -ne $ExpectedExitCode) {
        throw "spectrahabit $($Arguments -join ' ') retornou $exitCode; esperado $ExpectedExitCode.`n$output"
    }
    return $output
}

function Invoke-SpectraHabitJit {
    param([string[]]$Arguments, [int]$ExpectedExitCode = 0)
    $lines = @(& $script:cliPath run $script:projectPath -- @Arguments 2>&1)
    $exitCode = $LASTEXITCODE
    $output = ($lines | ForEach-Object { [string]$_ }) -join "`n"
    if ($exitCode -ne $ExpectedExitCode) {
        throw "spectralang run spectrahabit $($Arguments -join ' ') retornou $exitCode; esperado $ExpectedExitCode.`n$output"
    }
    return $output
}

function Assert-True {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
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
    [System.IO.File]::WriteAllText($absolutePath, $Text, [System.Text.UTF8Encoding]::new($false))
}

$tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$workspace = Join-Path $tempRoot ("spectrahabit-test-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $workspace | Out-Null
Push-Location $workspace
try {
    Assert-Contains (Invoke-SpectraHabit @("--help")) "spectrahabit init" "help deve listar init"
    Assert-Contains (Invoke-SpectraHabit @("unknown") 64) "comando desconhecido" "comando desconhecido deve retornar erro de uso"
    Assert-Contains (Invoke-SpectraHabit @("list") 1) "arquivo de hábitos ausente" "list deve orientar a inicialização"
    Assert-Contains (Invoke-SpectraHabit @("init")) "inicializado" "init deve criar o arquivo"
    Assert-Contains (Invoke-SpectraHabit @("init") 1) "já foi inicializado" "init repetido deve preservar os dados"
    Assert-Contains (Invoke-SpectraHabit @("self-test")) "10 verificações passaram" "self-test deve cobrir domínio e serialização"
    Assert-Contains (Invoke-SpectraHabit @("add", "8", "Meta inválida") 64) "entre 1 e 7" "meta semanal fora do intervalo deve ser recusada"

    Assert-Contains (Invoke-SpectraHabit @("add", "5", "Leitura matinal Ω")) "Hábito #1 criado" "add deve criar o primeiro hábito"
    Assert-Contains (Invoke-SpectraHabit @("add", "3", "Alongamento")) "Hábito #2 criado" "add deve persistir outro hábito"
    Assert-Contains (Invoke-SpectraHabit @("checkin", "1", "9999-12-31", "futuro") 64) "não pode estar no futuro" "check-in futuro deve ser recusado"
    Assert-Contains (Invoke-SpectraHabit @("checkin", "1", "2026-09-01", "Capítulo Ω 100%")) "registrado" "check-in deve persistir nota UTF-8"
    Assert-Contains (Invoke-SpectraHabit @("checkin", "1", "2026-09-02")) "registrado" "check-in sem nota deve ser aceito"
    Assert-Contains (Invoke-SpectraHabit @("checkin", "1", "2026-09-03", "Mais leitura")) "registrado" "check-in consecutivo deve ser registrado"
    Assert-Contains (Invoke-SpectraHabit @("checkin", "1", "2026-09-05")) "registrado" "check-in após um dia sem registro deve ser aceito"
    Assert-Contains (Invoke-SpectraHabit @("checkin", "2", "2026-09-03", "Mobilidade")) "registrado" "check-in deve pertencer ao hábito selecionado"
    Assert-Contains (Invoke-SpectraHabit @("checkin", "1", "2026-09-03") 1) "já existe check-in" "duplicata diária deve ser recusada"

    $history = Invoke-SpectraHabit @("history", "1")
    Assert-Contains $history "Capítulo Ω 100%" "histórico deve decodificar nota UTF-8 escapada"
    Assert-Contains $history "2026-09-05" "histórico deve listar datas gravadas"
    Assert-Contains (Invoke-SpectraHabit @("streak", "1", "2026-09-03")) "3 dia(s)" "streak deve contar dias consecutivos até a data"
    Assert-Contains (Invoke-SpectraHabit @("streak", "1", "2026-09-05")) "1 dia(s)" "streak deve parar no primeiro dia sem check-in"
    $week = Invoke-SpectraHabit @("week", "2026-09-01")
    Assert-Contains $week "2026-09-01..2026-09-07" "relatório semanal deve cobrir sete dias inclusivos"
    Assert-Contains $week "4/5 [meta pendente]" "relatório deve comparar check-ins à meta semanal"
    Assert-Contains $week "1/3 [meta pendente]" "relatório deve separar progresso por hábito"

    Assert-Contains (Invoke-SpectraHabit @("goal", "1", "3")) "atualizada para 3 dias" "goal deve atualizar a meta"
    Assert-Contains (Invoke-SpectraHabit @("week", "2026-09-01")) "4/3 [meta cumprida]" "relatório deve refletir nova meta"
    Assert-Contains (Invoke-SpectraHabit @("uncheck", "1", "2026-09-02")) "removido" "uncheck deve apagar apenas aquele dia"
    Assert-Contains (Invoke-SpectraHabit @("streak", "1", "2026-09-03")) "1 dia(s)" "streak deve recalcular após remoção"

    Assert-Contains (Invoke-SpectraHabit @("archive", "2")) "arquivado" "archive deve preservar e arquivar o hábito"
    $active = Invoke-SpectraHabit @("list")
    Assert-True (-not $active.Contains("Alongamento")) "list padrão deve ocultar arquivados"
    Assert-Contains (Invoke-SpectraHabit @("list", "--all")) "[archived] meta=3/7" "list --all deve incluir arquivados"
    Assert-Contains (Invoke-SpectraHabit @("checkin", "2", "2026-09-06") 1) "hábito arquivado" "check-in em arquivado deve ser recusado"
    Assert-Contains (Invoke-SpectraHabit @("remove", "2")) "e seu histórico removidos" "remove deve apagar hábito e check-ins"
    Assert-Contains (Invoke-SpectraHabit @("add", "2", "Hidratação")) "Hábito #3 criado" "IDs removidos não devem ser reutilizados"

    $databasePath = Join-Path $workspace ".spectrahabit\habits.db"
    $backupPath = $databasePath + ".bak"
    Move-Item -LiteralPath $databasePath -Destination $backupPath
    Assert-Contains (Invoke-SpectraHabit @("history", "1")) "Capítulo Ω 100%" "próxima execução deve restaurar o backup"
    Assert-True (Test-Path -LiteralPath $databasePath -PathType Leaf) "recuperação deve republicar habits.db"
    Assert-True (-not (Test-Path -LiteralPath $backupPath)) "recuperação deve remover backup restaurado"
    $validDatabase = [System.IO.File]::ReadAllText($databasePath)
    Write-Utf8 ".spectrahabit\habits.db" "conteúdo corrompido`n"
    Assert-Contains (Invoke-SpectraHabit @("list") 1) "cabeçalho do arquivo" "arquivo corrompido deve ser recusado"
    [System.IO.File]::WriteAllText($databasePath, $validDatabase, [System.Text.UTF8Encoding]::new($false))
    Assert-Contains (Invoke-SpectraHabit @("list")) "Leitura matinal Ω" "arquivo válido restaurado deve continuar legível"
}
finally {
    Pop-Location
    $resolvedWorkspace = [System.IO.Path]::GetFullPath($workspace)
    $safePrefix = $tempRoot.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
    if ($resolvedWorkspace.StartsWith($safePrefix, [System.StringComparison]::OrdinalIgnoreCase) -and (Split-Path -Leaf $resolvedWorkspace).StartsWith("spectrahabit-test-", [System.StringComparison]::Ordinal)) {
        Remove-Item -LiteralPath $resolvedWorkspace -Recurse -Force
    } else {
        throw "Recusa de limpeza fora da pasta temporária controlada: $resolvedWorkspace"
    }
}

$jitWorkspace = Join-Path $tempRoot ("spectrahabit-jit-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $jitWorkspace | Out-Null
Push-Location $jitWorkspace
try {
    Assert-Contains (Invoke-SpectraHabitJit @("init")) "inicializado" "JIT deve inicializar em outro diretório"
    Assert-Contains (Invoke-SpectraHabitJit @("add", "2", "Caminhada")) "Hábito #1 criado" "JIT deve criar um hábito"
    Assert-Contains (Invoke-SpectraHabitJit @("checkin", "1", "2026-09-01", "Parque")) "registrado" "JIT deve gravar check-in"
    Assert-Contains (Invoke-SpectraHabitJit @("streak", "1", "2026-09-01")) "1 dia(s)" "JIT deve calcular sequência persistida"
}
finally {
    Pop-Location
    $resolvedJitWorkspace = [System.IO.Path]::GetFullPath($jitWorkspace)
    $safePrefix = $tempRoot.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
    if ($resolvedJitWorkspace.StartsWith($safePrefix, [System.StringComparison]::OrdinalIgnoreCase) -and (Split-Path -Leaf $resolvedJitWorkspace).StartsWith("spectrahabit-jit-", [System.StringComparison]::Ordinal)) {
        Remove-Item -LiteralPath $resolvedJitWorkspace -Recurse -Force
    } else {
        throw "Recusa de limpeza fora da pasta temporária controlada: $resolvedJitWorkspace"
    }
}

"spectrahabit JIT + AOT integration: ok"
