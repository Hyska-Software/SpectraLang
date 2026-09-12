#requires -Version 5.1
<#
.SYNOPSIS
    Interactive version bump for the SpectraLang release surface.

.DESCRIPTION
    Runs as an interactive menu by default (no parameters). Flags exist for CI
    and for scripted use.

    Files kept in sync (the release surface):
      tools/spectra-cli/Cargo.toml              [package] version
      tools/spectra-lsp/Cargo.toml              [package] version
      tools/vscode-extension/package.json       version
      tools/vscode-extension/package-lock.json  version (root + packages[""])
      Cargo.lock                                spectra-cli / spectra-lsp entries

    With -IncludeCore (menu option 7) the internal language crates move to the
    same version as well: compiler, midend, backend, runtime, spectra-contract,
    spectra-db, spectra-api.

    The script never commits, pushes, or tags. Publish with git manually or let
    .github/workflows/release.yml do it.

    When GITHUB_OUTPUT is set, CURRENT and VERSION are appended to it, which is
    how the release workflow reads the resolved version.

.PARAMETER Bump
    patch | minor | major. Ignored when -Version is given.

.PARAMETER Version
    Exact target version: X.Y.Z or X.Y.Z-prerelease, without the leading "v".

.PARAMETER DryRun
    Print the preview and exit without writing anything.

.PARAMETER Yes
    Skip the confirmation prompt. Required when stdin is not interactive.

.PARAMETER IncludeCore
    Also bump the internal language crates.

.EXAMPLE
    .\bump-version.ps1
    # interactive menu

.EXAMPLE
    .\bump-version.ps1 -Bump patch -DryRun
    # preview only, nothing is written

.EXAMPLE
    .\bump-version.ps1 -Version 1.0.0-rc.1 -Yes
    # apply an exact version without prompting
#>
[CmdletBinding()]
param(
    [ValidateSet('patch', 'minor', 'major')]
    [string]$Bump,

    [string]$Version,

    [switch]$DryRun,

    [switch]$Yes,

    [switch]$IncludeCore
)

$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------------------
# Output helpers
# ---------------------------------------------------------------------------
$script:UseColor = -not [bool]$env:NO_COLOR

function Write-Line {
    param([string]$Text = '', [string]$Color = 'Gray')
    if ($script:UseColor) { Write-Host $Text -ForegroundColor $Color } else { Write-Host $Text }
}

function Write-Ok { param([string]$Text) Write-Line "  ok: $Text" 'Green' }
function Write-Warn { param([string]$Text) Write-Line "  aviso: $Text" 'Yellow' }
function Write-Step { param([string]$Text) Write-Line "  $Text" 'Cyan' }

# ---------------------------------------------------------------------------
# Repository discovery and text helpers
# ---------------------------------------------------------------------------
function Resolve-RepoRoot {
    $starts = @($PSScriptRoot, (Join-Path $PSScriptRoot '..'))
    foreach ($start in $starts) {
        if ([string]::IsNullOrWhiteSpace($start)) { continue }
        $candidate = $null
        try { $candidate = (Resolve-Path -LiteralPath $start -ErrorAction Stop).Path } catch { continue }
        $hasCli = Test-Path -LiteralPath (Join-Path $candidate 'tools/spectra-cli/Cargo.toml')
        $hasExt = Test-Path -LiteralPath (Join-Path $candidate 'tools/vscode-extension/package.json')
        if ($hasCli -and $hasExt) { return $candidate }
    }
    throw "Raiz do repositorio nao encontrada a partir de '$PSScriptRoot'."
}

function Read-Text { param([string]$Path) return [System.IO.File]::ReadAllText($Path) }

function Write-Text {
    param([string]$Path, [string]$Text)
    # UTF-8 without BOM, byte-exact apart from the replaced fields.
    $encoding = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($Path, $Text, $encoding)
}

function Split-Semver {
    param([string]$Value)
    $match = [regex]::Match($Value, '^(\d+)\.(\d+)\.(\d+)(?:-(.+))?$')
    if (-not $match.Success) { throw "versao invalida: '$Value' (use X.Y.Z ou X.Y.Z-pre)" }
    return [pscustomobject]@{
        Major = [int]$match.Groups[1].Value
        Minor = [int]$match.Groups[2].Value
        Patch = [int]$match.Groups[3].Value
        Pre   = $match.Groups[4].Value
    }
}

function Get-NextVersion {
    param([string]$Current, [string]$Mode)
    $v = Split-Semver $Current
    switch ($Mode) {
        'major' { return ('{0}.0.0' -f ($v.Major + 1)) }
        'minor' { return ('{0}.{1}.0' -f $v.Major, ($v.Minor + 1)) }
        'patch' {
            if ($v.Pre) { return ('{0}.{1}.{2}' -f $v.Major, $v.Minor, $v.Patch) }
            return ('{0}.{1}.{2}' -f $v.Major, $v.Minor, ($v.Patch + 1))
        }
        default { throw "modo de bump desconhecido: $Mode" }
    }
}

function Get-CargoPackageBlock {
    param([string]$Text)
    $match = [regex]::Match($Text, '(?s)^\[package\][ \t]*\r?\n(.*?)(?=\r?\n\[|\z)')
    if (-not $match.Success) { throw 'bloco [package] nao encontrado' }
    return $match
}

function Get-CargoPackageVersion {
    param([string]$Text)
    $block = (Get-CargoPackageBlock $Text).Groups[1].Value
    $match = [regex]::Match($block, '(?m)^version\s*=\s*"([^"]+)"')
    if (-not $match.Success) { throw 'campo version nao encontrado no bloco [package]' }
    return $match.Groups[1].Value
}

function Get-CargoCrateName {
    param([string]$Text)
    $block = (Get-CargoPackageBlock $Text).Groups[1].Value
    $match = [regex]::Match($block, '(?m)^name\s*=\s*"([^"]+)"')
    if (-not $match.Success) { throw 'campo name nao encontrado no bloco [package]' }
    return $match.Groups[1].Value
}

function Get-JsonVersion {
    param([string]$Text)
    $match = [regex]::Match($Text, '"version"\s*:\s*"([^"]+)"')
    if (-not $match.Success) { throw 'campo "version" nao encontrado' }
    return $match.Groups[1].Value
}

function Get-LockEntryVersion {
    param([string]$Text, [string]$Crate)
    if ([string]::IsNullOrWhiteSpace($Crate)) { return $null }
    $pattern = '(?ms)^\[\[package\]\][ \t]*\r?\nname = "' + [regex]::Escape($Crate) + '"\r?\nversion = "([^"]*)"'
    $match = [regex]::Match($Text, $pattern)
    if (-not $match.Success) { return $null }
    return $match.Groups[1].Value
}

function Set-CargoPackageVersion {
    param([string]$Text, [string]$Version)
    $block = Get-CargoPackageBlock $Text
    if ($block.Groups[1].Value -notmatch '(?m)^version\s*=\s*"') {
        throw 'campo version nao encontrado no bloco [package]'
    }
    $newBlock = [regex]::Replace($block.Groups[1].Value, '(?m)^version\s*=\s*"[^"]*"', ('version = "{0}"' -f $Version))
    $index = $block.Groups[1].Index
    return $Text.Substring(0, $index) + $newBlock + $Text.Substring($index + $block.Groups[1].Length)
}

function Set-FirstJsonVersion {
    param([string]$Text, [string]$Version)
    $regex = New-Object System.Text.RegularExpressions.Regex('"version"\s*:\s*"[^"]+"')
    return $regex.Replace($Text, ('"version": "{0}"' -f $Version), 1)
}

function Set-JsonLockVersions {
    param([string]$Text, [string]$Version)
    # Root "version" is the first occurrence in the file.
    $text = Set-FirstJsonVersion -Text $Text -Version $Version
    # packages[""] .version is the first occurrence after the "packages" object.
    $index = $text.IndexOf('"packages"')
    if ($index -lt 0) {
        Write-Warn 'package-lock.json sem bloco "packages"; apenas o campo raiz foi atualizado.'
        return $text
    }
    $head = $text.Substring(0, $index)
    $tail = $text.Substring($index)
    $tail = (Set-FirstJsonVersion -Text $tail -Version $Version)
    return $head + $tail
}

function Set-LockEntryVersion {
    param([string]$Text, [string]$Crate, [string]$Version)
    if ([string]::IsNullOrWhiteSpace($Crate)) { return $null }
    $pattern = '(?ms)(^\[\[package\]\][ \t]*\r?\nname = "' + [regex]::Escape($Crate) + '"\r?\nversion = )"[^"]*"'
    $match = [regex]::Match($Text, $pattern)
    if (-not $match.Success) { return $null }
    $index = $match.Index
    return $Text.Substring(0, $index) + $match.Groups[1].Value + '"' + $Version + '"' + $Text.Substring($index + $match.Length)
}

# ---------------------------------------------------------------------------
# Target model
# ---------------------------------------------------------------------------
$script:CoreCrates = @(
    @{ Id = 'compiler'; Label = 'compiler (core)';           Rel = 'compiler/Cargo.toml' },
    @{ Id = 'midend';   Label = 'midend (core)';             Rel = 'midend/Cargo.toml' },
    @{ Id = 'backend';  Label = 'backend (core)';            Rel = 'backend/Cargo.toml' },
    @{ Id = 'runtime';  Label = 'runtime (core)';            Rel = 'runtime/Cargo.toml' },
    @{ Id = 'contract'; Label = 'spectra-contract (core)';   Rel = 'packages/spectra-contract/Cargo.toml' },
    @{ Id = 'db';       Label = 'spectra-db (core)';         Rel = 'packages/spectra-db/Cargo.toml' },
    @{ Id = 'api';      Label = 'spectra-api (core)';        Rel = 'packages/spectra-api/Cargo.toml' }
)

function Get-BumpTargets {
    param([bool]$WithCore)

    $targets = New-Object System.Collections.ArrayList
    [void]$targets.Add([pscustomobject]@{ Id = 'cli';     Label = 'spectralang (CLI)';       Rel = 'tools/spectra-cli/Cargo.toml';              Kind = 'cargo' })
    [void]$targets.Add([pscustomobject]@{ Id = 'lsp';     Label = 'spectra-lsp';             Rel = 'tools/spectra-lsp/Cargo.toml';              Kind = 'cargo' })
    [void]$targets.Add([pscustomobject]@{ Id = 'ext';     Label = 'Extensao VS Code';        Rel = 'tools/vscode-extension/package.json';       Kind = 'json' })
    [void]$targets.Add([pscustomobject]@{ Id = 'extlock'; Label = 'Extensao (package-lock)'; Rel = 'tools/vscode-extension/package-lock.json';  Kind = 'jsonlock' })

    if ($WithCore) {
        foreach ($core in $script:CoreCrates) {
            [void]$targets.Add([pscustomobject]@{ Id = $core.Id; Label = $core.Label; Rel = $core.Rel; Kind = 'cargo' })
        }
    }
    return $targets
}

# ---------------------------------------------------------------------------
# Plan, preview, apply
# ---------------------------------------------------------------------------
function New-BumpPlan {
    param([object[]]$Targets, [string]$TargetVersion)

    $plan = New-Object System.Collections.ArrayList
    foreach ($target in $Targets) {
        $path = Join-Path $script:Root $target.Rel
        if (-not (Test-Path -LiteralPath $path)) { throw "arquivo nao encontrado: $($target.Rel)" }
        $text = Read-Text $path

        $old = $null
        $crate = $null
        switch ($target.Kind) {
            'cargo' { $old = Get-CargoPackageVersion $text; $crate = Get-CargoCrateName $text }
            'json' { $old = Get-JsonVersion $text }
            'jsonlock' { $old = Get-JsonVersion $text }
            default { throw "tipo de alvo desconhecido: $($target.Kind)" }
        }

        [void]$plan.Add([pscustomobject]@{
            Target = $target
            Path   = $path
            Old    = $old
            New    = $TargetVersion
            Crate  = $crate
            Text   = $text
        })
    }
    return $plan
}

function Get-CurrentVersion {
    param([object[]]$Plan)
    $cli = $Plan | Where-Object { $_.Target.Id -eq 'cli' } | Select-Object -First 1
    if (-not $cli) { throw 'alvo cli ausente do plano' }
    return $cli.Old
}

function Show-PlanPreview {
    param([object[]]$Plan, [string]$TargetVersion)

    Write-Line ''
    Write-Step 'Previa (nada foi gravado ainda):'
    Write-Line ''
    foreach ($item in $Plan) {
        $changed = $item.Old -ne $item.New
        $color = 'DarkGray'
        if ($changed) { $color = 'White' }
        $line = '  {0,-46} {1} -> {2}' -f $item.Target.Rel, $item.Old, $item.New
        if (-not $changed) { $line = $line + '  (ja na versao alvo)' }
        Write-Line $line $color
    }

    $lockPath = Join-Path $script:Root 'Cargo.lock'
    if (Test-Path -LiteralPath $lockPath) {
        $lockText = Read-Text $lockPath
        foreach ($item in ($Plan | Where-Object { $_.Target.Kind -eq 'cargo' })) {
            $locked = Get-LockEntryVersion -Text $lockText -Crate $item.Crate
            if ($null -ne $locked -and $locked -ne $item.New) {
                Write-Line ('  {0,-46} {1} -> {2}' -f ('Cargo.lock :: ' + $item.Crate), $locked, $item.New) 'White'
            }
        }
    }
    Write-Line ''
}

function Sync-CargoLock {
    param([object[]]$Plan)

    $lockPath = Join-Path $script:Root 'Cargo.lock'
    if (-not (Test-Path -LiteralPath $lockPath)) {
        Write-Warn 'Cargo.lock nao encontrado; lockfile nao atualizado.'
        return 'missing'
    }

    $cargo = Get-Command cargo -ErrorAction SilentlyContinue
    if ($cargo) {
        # Native stderr must not become a terminating error under ErrorActionPreference=Stop.
        $previous = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        try {
            foreach ($extra in @(@('--offline'), @())) {
                $arguments = @('metadata', '--format-version', '1') + $extra
                Push-Location $script:Root
                try { & $cargo.Source @arguments *> $null } finally { Pop-Location }
                if ($LASTEXITCODE -eq 0) { return 'cargo' }
            }
        } finally {
            $ErrorActionPreference = $previous
        }
        Write-Warn 'cargo metadata falhou; aplicando atualizacao textual do Cargo.lock.'
    } else {
        Write-Warn 'cargo nao encontrado; aplicando atualizacao textual do Cargo.lock.'
    }

    $text = Read-Text $lockPath
    $updated = $false
    foreach ($item in ($Plan | Where-Object { $_.Target.Kind -eq 'cargo' })) {
        $newText = Set-LockEntryVersion -Text $text -Crate $item.Crate -Version $item.New
        if ($null -eq $newText) {
            Write-Warn ("Cargo.lock: entrada '{0}' nao encontrada." -f $item.Crate)
            continue
        }
        $text = $newText
        $updated = $true
    }
    if ($updated) { Write-Text -Path $lockPath -Text $text }
    return 'text'
}

function Invoke-BumpPlan {
    param([object[]]$Plan)

    $written = New-Object System.Collections.ArrayList
    foreach ($item in $Plan) {
        $newText = $null
        switch ($item.Target.Kind) {
            'cargo' { $newText = Set-CargoPackageVersion -Text $item.Text -Version $item.New }
            'json' { $newText = Set-FirstJsonVersion -Text $item.Text -Version $item.New }
            'jsonlock' { $newText = Set-JsonLockVersions -Text $item.Text -Version $item.New }
            default { throw "tipo de alvo desconhecido: $($item.Target.Kind)" }
        }
        if ($newText -ne $item.Text) {
            Write-Text -Path $item.Path -Text $newText
            [void]$written.Add($item.Target.Rel)
        }
    }

    $lockMethod = Sync-CargoLock -Plan $Plan

    Write-Line ''
    if ($written.Count -gt 0) {
        foreach ($rel in $written) { Write-Ok $rel }
    } else {
        Write-Step 'Nenhum manifesto precisou de alteracao.'
    }
    switch ($lockMethod) {
        'cargo' { Write-Ok 'Cargo.lock regenerado via cargo metadata' }
        'text' { Write-Ok 'Cargo.lock atualizado (modo textual)' }
        'missing' { Write-Warn 'Cargo.lock ausente' }
    }
    return $written
}

function Get-LocalVersions {
    param([bool]$WithCore)
    $targets = Get-BumpTargets -WithCore $WithCore
    $rows = New-Object System.Collections.ArrayList
    foreach ($target in $targets) {
        $path = Join-Path $script:Root $target.Rel
        $value = '(ausente)'
        if (Test-Path -LiteralPath $path) {
            $text = Read-Text $path
            switch ($target.Kind) {
                'cargo' { $value = Get-CargoPackageVersion $text }
                'json' { $value = Get-JsonVersion $text }
                'jsonlock' { $value = Get-JsonVersion $text }
            }
        }
        [void]$rows.Add([pscustomobject]@{ Target = $target; Version = $value })
    }
    return $rows
}

function Show-VersionStatus {
    param([bool]$WithCore)

    $rows = Get-LocalVersions -WithCore $WithCore
    $cli = ($rows | Where-Object { $_.Target.Id -eq 'cli' } | Select-Object -First 1).Version
    $lockPath = Join-Path $script:Root 'Cargo.lock'
    $lockText = $null
    if (Test-Path -LiteralPath $lockPath) { $lockText = Read-Text $lockPath }

    Write-Line ''
    Write-Step 'Versoes por arquivo:'
    Write-Line ''
    foreach ($row in $rows) {
        $color = 'DarkGray'
        $suffix = ''
        if ($row.Target.Id -ne 'cli' -and $row.Version -ne $cli) { $color = 'Yellow'; $suffix = '  (difere do CLI)' }
        Write-Line ('  {0,-46} {1}{2}' -f $row.Target.Rel, $row.Version, $suffix) $color
    }
    if ($lockText) {
        foreach ($row in ($rows | Where-Object { $_.Target.Kind -eq 'cargo' })) {
            $manifestPath = Join-Path $script:Root $row.Target.Rel
            $crate = $null
            if (Test-Path -LiteralPath $manifestPath) {
                try { $crate = Get-CargoCrateName (Read-Text $manifestPath) } catch { $crate = $null }
            }
            $locked = Get-LockEntryVersion -Text $lockText -Crate $crate
            if ($null -eq $locked) { continue }
            $color = 'DarkGray'
            $suffix = ''
            if ($locked -ne $row.Version) { $color = 'Yellow'; $suffix = '  (lockfile dessincronizado)' }
            Write-Line ('  {0,-46} {1}{2}' -f ('Cargo.lock :: ' + $crate), $locked, $suffix) $color
        }
    }
    Write-Line ''
}

# ---------------------------------------------------------------------------
# Shared run flow
# ---------------------------------------------------------------------------
function Resolve-TargetVersion {
    param([string]$Current, [string]$Mode, [string]$Exact)
    if ($Exact) {
        $null = Split-Semver $Exact
        return $Exact
    }
    return Get-NextVersion -Current $Current -Mode $Mode
}

function Write-GitHubOutputs {
    param([string]$Current, [string]$TargetVersion)
    Write-Line ('  CURRENT={0}' -f $Current) 'Gray'
    Write-Line ('  VERSION={0}' -f $TargetVersion) 'Gray'
    if ($env:GITHUB_OUTPUT) {
        Add-Content -LiteralPath $env:GITHUB_OUTPUT -Value ('CURRENT={0}' -f $Current)
        Add-Content -LiteralPath $env:GITHUB_OUTPUT -Value ('VERSION={0}' -f $TargetVersion)
    }
}

function Get-SummaryLine {
    param([object[]]$Plan, [string]$TargetVersion)
    $bits = New-Object System.Collections.ArrayList
    foreach ($item in $Plan) {
        if ($item.Target.Id -eq 'extlock') { continue }
        [void]$bits.Add(('{0} {1}->{2}' -f $item.Target.Id, $item.Old, $item.New))
    }
    return ('alvo {0} | {1}' -f $TargetVersion, ($bits -join '  '))
}

function Show-NextSteps {
    param([string]$TargetVersion)
    Write-Line ''
    Write-Step 'Proximos passos (o script nao faz commit/tag):'
    Write-Line '    git add -u' 'Gray'
    Write-Line ('    git commit -m "chore: bump version to {0}"' -f $TargetVersion) 'Gray'
    Write-Line '    git push origin main   # release.yml cria a tag e o release' 'Gray'
    Write-Line ''
}

function Invoke-BumpRun {
    param([string]$Mode, [string]$Exact, [bool]$PreviewOnly, [bool]$SkipConfirm, [bool]$WithCore)

    $plan = New-BumpPlan -Targets (Get-BumpTargets -WithCore $WithCore) -TargetVersion '0.0.0'
    $current = Get-CurrentVersion -Plan $plan
    $target = Resolve-TargetVersion -Current $current -Mode $Mode -Exact $Exact

    if ($target -eq $current -and ($plan | Where-Object { $_.Old -ne $target }).Count -eq 0) {
        Write-Line ''
        Write-Step ('Nada a fazer: todos os arquivos ja estao em {0}.' -f $target)
        Write-GitHubOutputs -Current $current -TargetVersion $target
        return
    }

    $plan = New-BumpPlan -Targets (Get-BumpTargets -WithCore $WithCore) -TargetVersion $target

    if ([int]((Split-Semver $target).Major) -lt [int]((Split-Semver $current).Major)) {
        Write-Warn ('a versao alvo ({0}) e menor que a atual ({1}).' -f $target, $current)
    }

    Show-PlanPreview -Plan $plan -TargetVersion $target

    if ($PreviewOnly) {
        Write-Step 'dry bump: nada foi gravado.'
        Write-GitHubOutputs -Current $current -TargetVersion $target
        return
    }

    if (-not $SkipConfirm) {
        $answer = Read-Host '  Aplicar as alteracoes? (s/N)'
        if ($answer -notmatch '^(s|sim|y|yes)$') {
            Write-Step 'cancelado; nada foi gravado.'
            return
        }
    }

    $null = Invoke-BumpPlan -Plan $plan
    Write-Line ''
    Write-Step (Get-SummaryLine -Plan $plan -TargetVersion $target)
    Write-GitHubOutputs -Current $current -TargetVersion $target
    Show-NextSteps -TargetVersion $target
}

# ---------------------------------------------------------------------------
# Non-interactive entry point
# ---------------------------------------------------------------------------
function Test-InteractiveInput {
    if ([Console]::IsInputRedirected) { return $false }
    try { $null = $Host.UI.RawUI.WindowSize; return $true } catch { return $false }
}

function Invoke-NonInteractive {
    $mode = $Bump
    if (-not $Version -and -not $mode) { $mode = 'patch' }
    if ($DryRun) {
        Invoke-BumpRun -Mode $mode -Exact $Version -PreviewOnly $true -SkipConfirm $true -WithCore ([bool]$IncludeCore)
        return
    }
    if (-not $Yes -and -not (Test-InteractiveInput)) {
        throw 'stdin nao e interativo: use -Yes para aplicar sem confirmacao (ou -DryRun para apenas ver a previa).'
    }
    Invoke-BumpRun -Mode $mode -Exact $Version -PreviewOnly $false -SkipConfirm ([bool]$Yes) -WithCore ([bool]$IncludeCore)
}

# ---------------------------------------------------------------------------
# Interactive menu
# ---------------------------------------------------------------------------
function Show-Menu {
    param([bool]$WithCore)

    $rows = Get-LocalVersions -WithCore $WithCore
    $cli = ($rows | Where-Object { $_.Target.Id -eq 'cli' } | Select-Object -First 1).Version
    $lsp = ($rows | Where-Object { $_.Target.Id -eq 'lsp' } | Select-Object -First 1).Version
    $ext = ($rows | Where-Object { $_.Target.Id -eq 'ext' } | Select-Object -First 1).Version

    Write-Line ''
    Write-Line '  ==========================================================================' 'Cyan'
    Write-Line '   SpectraLang - bump de versao' 'Cyan'
    Write-Line '  ==========================================================================' 'Cyan'
    Write-Line ''
    Write-Line ('   CLI {0}    LSP {1}    extensao {2}    crates internos: {3}' -f $cli, $lsp, $ext, $(if ($WithCore) { 'ligados' } else { 'desligados' })) 'White'
    Write-Line ''
    Write-Line ('  [1] Bump patch   {0} -> {1}' -f $cli, (Get-NextVersion -Current $cli -Mode patch)) 'Gray'
    Write-Line ('  [2] Bump minor   {0} -> {1}' -f $cli, (Get-NextVersion -Current $cli -Mode minor)) 'Gray'
    Write-Line ('  [3] Bump major   {0} -> {1}' -f $cli, (Get-NextVersion -Current $cli -Mode major)) 'Gray'
    Write-Line '  [4] Definir versao exata' 'Gray'
    Write-Line '  [5] Dry bump (previa sem gravar)' 'Gray'
    Write-Line '  [6] Ver versoes por arquivo' 'Gray'
    Write-Line ('  [7] Crates internos do core: {0}  (alternar)' -f $(if ($WithCore) { 'ligados' } else { 'desligados' })) 'Gray'
    Write-Line '  [0] Sair' 'Gray'
    Write-Line ''
}

function Start-Menu {
    $withCore = [bool]$IncludeCore

    while ($true) {
        Show-Menu -WithCore $withCore
        $choice = Read-Host '  Escolha'
        try {
            switch ($choice) {
            '1' { Invoke-BumpRun -Mode 'patch' -Exact '' -PreviewOnly $false -SkipConfirm $false -WithCore $withCore }
            '2' { Invoke-BumpRun -Mode 'minor' -Exact '' -PreviewOnly $false -SkipConfirm $false -WithCore $withCore }
            '3' { Invoke-BumpRun -Mode 'major' -Exact '' -PreviewOnly $false -SkipConfirm $false -WithCore $withCore }
            '4' {
                $exact = Read-Host '  Versao alvo (ex: 1.0.0 ou 1.0.0-rc.1)'
                if ([string]::IsNullOrWhiteSpace($exact)) { continue }
                Invoke-BumpRun -Mode '' -Exact $exact.Trim() -PreviewOnly $false -SkipConfirm $false -WithCore $withCore
            }
            '5' {
                $kind = Read-Host '  Dry bump: [1] patch  [2] minor  [3] major  [4] exata'
                switch ($kind) {
                    '1' { Invoke-BumpRun -Mode 'patch' -Exact '' -PreviewOnly $false -SkipConfirm $false -WithCore $withCore }
                    '2' { Invoke-BumpRun -Mode 'minor' -Exact '' -PreviewOnly $false -SkipConfirm $false -WithCore $withCore }
                    '3' { Invoke-BumpRun -Mode 'major' -Exact '' -PreviewOnly $false -SkipConfirm $false -WithCore $withCore }
                    '4' {
                        $exact = Read-Host '  Versao alvo (ex: 1.0.0 ou 1.0.0-rc.1)'
                        if ([string]::IsNullOrWhiteSpace($exact)) { continue }
                        Invoke-BumpRun -Mode '' -Exact $exact.Trim() -PreviewOnly $false -SkipConfirm $false -WithCore $withCore
                    }
                    default { Write-Warn 'opcao invalida.' }
                }
            }
            '6' { Show-VersionStatus -WithCore $withCore }
            '7' { $withCore = -not $withCore }
            '0' { return }
            default { Write-Warn 'opcao invalida.' }
            }
        } catch {
            Write-Line ''
            Write-Warn ('falhou: {0}' -f $_.Exception.Message)
        }
    }
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
$script:Root = Resolve-RepoRoot

if ($Bump -or $Version) {
    Invoke-NonInteractive
} else {
    Start-Menu
}
