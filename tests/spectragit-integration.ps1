param(
    [string]$Executable = "target\spectragit-dev.exe"
)

$ErrorActionPreference = "Stop"
$root = (Get-Location).Path
$exePath = [System.IO.Path]::GetFullPath((Join-Path $root $Executable))
if (-not (Test-Path -LiteralPath $exePath -PathType Leaf)) {
    throw "Executável não encontrado: $exePath. Compile examples/complete/18-spectragit primeiro."
}

function Invoke-Spectragit {
    param(
        [string[]]$Arguments,
        [int]$ExpectedExitCode = 0
    )
    $lines = @(& $script:exePath @Arguments 2>&1)
    $exitCode = $LASTEXITCODE
    $output = ($lines | ForEach-Object { [string]$_ }) -join "`n"
    if ($exitCode -ne $ExpectedExitCode) {
        throw "spectragit $($Arguments -join ' ') retornou $exitCode; esperado $ExpectedExitCode.`n$output"
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
$workspace = Join-Path $tempRoot ("spectragit-test-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $workspace | Out-Null
Push-Location $workspace
try {
    Assert-Contains (Invoke-Spectragit @("--help")) "spectragit init" "help deve listar os comandos com o nome spectragit"
    Assert-Contains (Invoke-Spectragit @("unknown-command") 64) "comando desconhecido" "comando desconhecido deve falhar com uso"
    Invoke-Spectragit @("status") 1 | Out-Null
    Assert-Contains (Invoke-Spectragit @("init")) "inicializado" "init deve criar o repositório"
    $configPath = Join-Path $workspace ".spectragit\config"
    [System.IO.File]::WriteAllText($configPath, "format`t999`n", [System.Text.UTF8Encoding]::new($false))
    Assert-Contains (Invoke-Spectragit @("status") 1) "incompatível" "formato desconhecido deve ser recusado"
    [System.IO.File]::WriteAllText($configPath, "format`t1`n", [System.Text.UTF8Encoding]::new($false))
    Assert-Contains (Invoke-Spectragit @("init") 1) "já contém" "init repetido deve falhar claramente"
    Assert-Contains (Invoke-Spectragit @("status")) "árvore de trabalho limpa" "repositório novo deve estar limpo"
    Assert-Contains (Invoke-Spectragit @("self-test")) "14 verificações passaram" "self-test Spectra deve verificar hash, caminhos, serialização e diff por linhas"

    $sameText = "linha um`nlinha dois`n"
    Write-Utf8 "src\a.spectra" $sameText
    Write-Utf8 "docs\nested\b.spectra" $sameText
    Invoke-Spectragit @("add", "docs\nested\") | Out-Null
    Invoke-Spectragit @("add", "src\a.spectra") | Out-Null
    $status = Invoke-Spectragit @("status")
    Assert-Contains $status "A docs/nested/b.spectra" "status deve listar arquivo staged novo em pasta aninhada"
    Assert-Contains $status "A src/a.spectra" "status deve listar arquivo staged novo"
    $diff = Invoke-Spectragit @("diff", "--staged")
    Assert-Contains $diff "+ linha um" "diff staged deve exibir linhas adicionadas"

    $firstCommitOutput = Invoke-Spectragit @("commit", "-m", "Initial commit")
    $firstCommit = [regex]::Match($firstCommitOutput, "\[(?:main) (sg1-[0-9]+-[0-9]+-[0-9]+)\]").Groups[1].Value
    Assert-True ($firstCommit -ne "") "commit inicial deve imprimir seu identificador"
    Assert-Contains (Invoke-Spectragit @("log")) "Initial commit" "log deve reconstruir o histórico"
    $firstCommitInfo = Invoke-Spectragit @("inspect", $firstCommit)
    $firstTree = [regex]::Match($firstCommitInfo, "Tree: (sg1-[0-9]+-[0-9]+-[0-9]+)").Groups[1].Value
    Assert-True ($firstTree -ne "") "inspect commit deve mostrar a tree"
    $firstTreeInfo = Invoke-Spectragit @("inspect", $firstTree)
    $blobRows = [regex]::Matches($firstTreeInfo, "(?m)^\s+(sg1-[0-9]+-[0-9]+-[0-9]+) (.+)$")
    Assert-True ($blobRows.Count -eq 2) "tree deve conter os dois caminhos distintos"
    Assert-True ($blobRows[0].Groups[1].Value -eq $blobRows[1].Groups[1].Value) "conteúdo idêntico deve reutilizar o mesmo blob"
    $sameBlob = $blobRows[0].Groups[1].Value
    Assert-Contains (Invoke-Spectragit @("inspect", $sameBlob)) "linha dois" "inspect blob deve carregar e validar seu conteúdo"

    $snapshotOne = Invoke-Spectragit @("snapshot", "create", "baseline")
    $snapshotTwo = Invoke-Spectragit @("snapshot", "create", "repeat")
    $baselineTree = [regex]::Match($snapshotOne, "\((sg1-[0-9]+-[0-9]+-[0-9]+)\)").Groups[1].Value
    $repeatTree = [regex]::Match($snapshotTwo, "\((sg1-[0-9]+-[0-9]+-[0-9]+)\)").Groups[1].Value
    Assert-True ($baselineTree -eq $repeatTree -and $baselineTree -eq $firstTree) "trees iguais devem produzir serialização e hash idênticos"
    Assert-Contains (Invoke-Spectragit @("snapshot", "list")) "baseline" "snapshot list deve manter nomes após reiniciar o processo"
    Write-Utf8 "snapshot-note.txt" "conteúdo salvo no snapshot`n"
    Invoke-Spectragit @("snapshot", "create", "with-note") | Out-Null
    [System.IO.File]::Delete((Join-Path $workspace "snapshot-note.txt"))
    Invoke-Spectragit @("snapshot", "restore", "with-note") | Out-Null
    Assert-True ([System.IO.File]::ReadAllText((Join-Path $workspace "snapshot-note.txt")) -eq "conteúdo salvo no snapshot`n") "snapshot deve restaurar arquivo não rastreado salvo"
    Invoke-Spectragit @("snapshot", "restore", "with-note") | Out-Null
    [System.IO.File]::Delete((Join-Path $workspace "snapshot-note.txt"))

    Write-Utf8 "!SG_SCAN_ERROR!" "sentinel filename`n"
    Invoke-Spectragit @("add", "!SG_SCAN_ERROR!") | Out-Null
    Assert-Contains (Invoke-Spectragit @("status")) "A !SG_SCAN_ERROR!" "nome de arquivo parecido com sentinela não deve falhar na varredura"
    Write-Utf8 "!SG_SCAN_ERROR!" "`t!SG_SCAN_ERROR! conteúdo válido`n"
    Assert-Contains (Invoke-Spectragit @("diff")) "conteúdo válido" "conteúdo parecido com sentinela deve permanecer texto normal"
    Invoke-Spectragit @("restore", "!SG_SCAN_ERROR!") | Out-Null
    Assert-True (-not (Test-Path -LiteralPath "!SG_SCAN_ERROR!")) "restore deve remover o arquivo de teste staged"

    Invoke-Spectragit @("branch", "dev") | Out-Null
    Assert-Contains (Invoke-Spectragit @("branch")) "dev" "branch deve criar e listar uma branch"
    Invoke-Spectragit @("checkout", "dev") | Out-Null
    Write-Utf8 "src\a.spectra" "linha um`nlinha três`nlinha dois`n"
    Write-Utf8 "extra.txt" "arquivo local não staged"
    $status = Invoke-Spectragit @("status")
    Assert-Contains $status "M src/a.spectra" "status deve detectar modificação fora do staging"
    Assert-Contains $status "? extra.txt" "status deve detectar arquivo não rastreado"
    $diff = Invoke-Spectragit @("diff")
    Assert-Contains $diff "+ linha três" "diff deve detectar linha adicionada"
    $blockedCheckout = Invoke-Spectragit @("checkout", "main") 1
    Assert-Contains $blockedCheckout "arquivo rastreado modificado" "checkout deve proteger alterações locais"
    Assert-True (Test-Path -LiteralPath "extra.txt" -PathType Leaf) "checkout bloqueado deve preservar arquivo não rastreado"
    [System.IO.File]::Delete((Join-Path $workspace "extra.txt"))
    Invoke-Spectragit @("add", "src\a.spectra") | Out-Null
    Assert-Contains (Invoke-Spectragit @("status")) "M src/a.spectra" "add específico deve atualizar o staging"
    Assert-Contains (Invoke-Spectragit @("diff", "--staged")) "+ linha três" "diff --staged deve comparar o índice com HEAD"
    Assert-Contains (Invoke-Spectragit @("diff")) "Nenhuma diferença." "diff padrão não deve repetir alterações que já estão staged"
    Write-Utf8 "src\a.spectra" "linha zero`nlinha um`nlinha três`nlinha dois`n"
    Assert-Contains (Invoke-Spectragit @("diff")) "+ linha zero" "diff padrão deve comparar o worktree com o staging"
    Assert-True (-not (Invoke-Spectragit @("diff", "--staged")).Contains("linha zero")) "diff --staged não deve incluir alterações ainda fora do staging"
    Write-Utf8 "src\a.spectra" "linha um`nlinha três`nlinha dois`n"
    $devCommitOutput = Invoke-Spectragit @("commit", "-m", "Update on dev")
    $devCommit = [regex]::Match($devCommitOutput, "\[dev (sg1-[0-9]+-[0-9]+-[0-9]+)\]").Groups[1].Value
    Assert-True ($devCommit -ne "") "commit em dev deve atualizar seu ponteiro"
    Assert-Contains (Invoke-Spectragit @("explain", $devCommit)) "Linhas aproximadas: +1 -0" "explain deve contar as linhas alteradas"
    Assert-Contains (Invoke-Spectragit @("timeline")) "Histórico de dev" "timeline deve mostrar o histórico da branch atual"

    Invoke-Spectragit @("checkout", "main") | Out-Null
    Assert-True ([System.IO.File]::ReadAllText((Join-Path $workspace "src\a.spectra")) -eq $sameText) "checkout deve restaurar o snapshot de main"
    Assert-Contains (Invoke-Spectragit @("log")) "Initial commit" "main deve manter seu histórico"
    Assert-True (-not (Invoke-Spectragit @("log")).Contains("Update on dev")) "históricos de main e dev devem permanecer independentes"

    Write-Utf8 "src\a.spectra" "main independente`n"
    Invoke-Spectragit @("add", ".") | Out-Null
    $mainCommitOutput = Invoke-Spectragit @("commit", "-m", "Main change")
    $mainCommit = [regex]::Match($mainCommitOutput, "\[main (sg1-[0-9]+-[0-9]+-[0-9]+)\]").Groups[1].Value
    Assert-True ($mainCommit -ne "" -and $mainCommit -ne $devCommit) "branches devem ter ponteiros distintos"
    Write-Utf8 "src\a.spectra" "edição local que será restaurada`n"
    Invoke-Spectragit @("restore", "src\a.spectra") | Out-Null
    Assert-True ([System.IO.File]::ReadAllText((Join-Path $workspace "src\a.spectra")) -eq "main independente`n") "restore deve recuperar a versão registrada"
    Assert-Contains (Invoke-Spectragit @("status")) "árvore de trabalho limpa" "restore deve limpar a alteração do arquivo"

    [System.IO.File]::Delete((Join-Path $workspace "docs\nested\b.spectra"))
    Assert-Contains (Invoke-Spectragit @("status")) "D docs/nested/b.spectra" "status deve detectar arquivo removido"
    Assert-Contains (Invoke-Spectragit @("diff")) "/dev/null" "diff deve apresentar arquivo removido"
    Invoke-Spectragit @("add", ".") | Out-Null
    Assert-Contains (Invoke-Spectragit @("status")) "D docs/nested/b.spectra" "add . deve stagear remoções"
    Assert-Contains (Invoke-Spectragit @("diff", "--staged")) "/dev/null" "diff --staged deve mostrar remoção preparada"
    Write-Utf8 "docs\nested\b.spectra" "restaurado fora do staging`n"
    Assert-Contains (Invoke-Spectragit @("diff")) "+ restaurado fora do staging" "diff deve mostrar recriação local após remoção staged"
    [System.IO.File]::Delete((Join-Path $workspace "docs\nested\b.spectra"))
    $removeCommitOutput = Invoke-Spectragit @("commit", "-m", "Remove nested file")
    $removeCommit = [regex]::Match($removeCommitOutput, "\[main (sg1-[0-9]+-[0-9]+-[0-9]+)\]").Groups[1].Value
    Assert-True ($removeCommit -ne "") "commit deve registrar a remoção"
    Assert-Contains (Invoke-Spectragit @("explain", $removeCommit)) "1 removidos" "explain deve identificar arquivo removido"

    Invoke-Spectragit @("checkout", "dev") | Out-Null
    Assert-True (Test-Path -LiteralPath "docs\nested\b.spectra" -PathType Leaf) "checkout deve restaurar arquivo existente na branch dev"
    Assert-True ([System.IO.File]::ReadAllText((Join-Path $workspace "src\a.spectra")).Contains("linha três")) "dev deve preservar seu próprio conteúdo"
    Invoke-Spectragit @("checkout", "main") | Out-Null
    Assert-True (-not (Test-Path -LiteralPath "docs\nested\b.spectra")) "checkout deve remover arquivo ausente no snapshot de main"

    Write-Utf8 "notes-local.txt" "arquivo local sem colisão`n"
    Invoke-Spectragit @("checkout", "dev") | Out-Null
    Assert-True (Test-Path -LiteralPath "notes-local.txt" -PathType Leaf) "checkout deve preservar arquivo não rastreado sem colisão"
    Invoke-Spectragit @("checkout", "main") | Out-Null
    Assert-True (Test-Path -LiteralPath "notes-local.txt" -PathType Leaf) "checkout de volta deve preservar arquivo não rastreado sem colisão"
    [System.IO.File]::Delete((Join-Path $workspace "notes-local.txt"))

    Write-Utf8 "docs\nested\b.spectra" "arquivo não rastreado protegido`n"
    $blockedCollision = Invoke-Spectragit @("checkout", "dev") 1
    Assert-Contains $blockedCollision "conteúdo não rastreado" "checkout deve bloquear colisão com arquivo local não rastreado"
    Assert-True ([System.IO.File]::ReadAllText((Join-Path $workspace "docs\nested\b.spectra")) -eq "arquivo não rastreado protegido`n") "checkout bloqueado deve preservar conteúdo não rastreado em colisão"
    [System.IO.File]::Delete((Join-Path $workspace "docs\nested\b.spectra"))
    Invoke-Spectragit @("checkout", "dev") | Out-Null
    Invoke-Spectragit @("checkout", "main") | Out-Null

    Invoke-Spectragit @("snapshot", "restore", "baseline") | Out-Null
    Assert-True ([System.IO.File]::ReadAllText((Join-Path $workspace "src\a.spectra")) -eq $sameText) "snapshot restore deve recuperar todo o snapshot"
    Assert-True (Test-Path -LiteralPath "docs\nested\b.spectra" -PathType Leaf) "snapshot restore deve recriar arquivos removidos"
    Invoke-Spectragit @("restore", "src\a.spectra", "--source", $devCommit) | Out-Null
    Assert-True ([System.IO.File]::ReadAllText((Join-Path $workspace "src\a.spectra")).Contains("linha três")) "restore --source deve carregar a versão solicitada"
    Assert-Contains (Invoke-Spectragit @("inspect", $baselineTree)) "Tipo: tree" "inspect deve aceitar a tree de um snapshot"

    $blobPath = Join-Path $workspace (".spectragit\objects\" + $sameBlob)
    [System.IO.File]::WriteAllText($blobPath, "conteúdo corrompido", [System.Text.UTF8Encoding]::new($false))
    Assert-Contains (Invoke-Spectragit @("inspect", $sameBlob) 1) "inexistente ou corrompido" "inspect deve detectar objeto adulterado pelo hash"

    "spectragit integration: ok"
}
finally {
    Pop-Location
    $resolvedWorkspace = [System.IO.Path]::GetFullPath($workspace)
    $resolvedTempRoot = [System.IO.Path]::GetFullPath($tempRoot)
    $safePrefix = $resolvedTempRoot.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
    if ($resolvedWorkspace.StartsWith($safePrefix, [System.StringComparison]::OrdinalIgnoreCase) -and (Split-Path -Leaf $resolvedWorkspace).StartsWith("spectragit-test-", [System.StringComparison]::Ordinal)) {
        Remove-Item -LiteralPath $resolvedWorkspace -Recurse -Force
    } else {
        throw "Recusa de limpeza fora da pasta temporária controlada: $resolvedWorkspace"
    }
}
