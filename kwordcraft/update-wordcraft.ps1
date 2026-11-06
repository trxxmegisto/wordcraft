<#
.SYNOPSIS
    KWordCraft: WordCraft mit Assistent aktualisieren, bauen und installieren.

.DESCRIPTION
    1. Holt die neuen Änderungen von upstream (storytold/wordcraft) und führt sie in den
       Branch "assistant" zusammen.
    2. Bringt die Sprachdaten (craft-i18n) auf die Revision, die WordCraft verlangt.
    3. Prüft den Assistenten mit seinen Tests und baut die Release-Version.
    4. Installiert sie nach D:\KWordCraft\bin\wordcraft.exe (die alte bleibt als
       wordcraft.prev.exe) und legt beim ersten Mal die Desktop-Verknüpfungen an.
    5. Schiebt "assistant" und "main" auf deinen Fork (origin), außer mit -NoPush.

    Bricht bei jedem Fehler ab, bevor etwas installiert oder gepusht wird. Gibt es beim
    Zusammenführen einen Konflikt, wird das Zusammenführen zurückgenommen.

.PARAMETER NoPush
    Nichts zu GitHub schieben.

.PARAMETER NoMerge
    Kein Update von upstream holen, nur den aktuellen Stand bauen und installieren.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File D:\KWordCraft\wordcraft\kwordcraft\update-wordcraft.ps1
#>
[CmdletBinding()]
param(
    [switch]$NoPush,
    [switch]$NoMerge
)

$ErrorActionPreference = 'Stop'
$Repo    = Split-Path -Parent $PSScriptRoot           # D:\KWordCraft\wordcraft
$Root    = Split-Path -Parent $Repo                   # D:\KWordCraft
$I18n    = Join-Path $Root 'craft-i18n'
$Bin     = Join-Path $Root 'bin'
$Branch  = 'assistant'

function Step($text) { Write-Host ''; Write-Host "==> $text" -ForegroundColor Cyan }
function Fail($text) { Write-Host ''; Write-Host "ABGEBROCHEN: $text" -ForegroundColor Red; exit 1 }

# A native command; stops the script when it fails.
function Run {
    param([string]$Exe, [string[]]$Arguments, [string]$What)
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) { Fail "$What (Exit-Code $LASTEXITCODE)" }
}

Set-Location $Repo

Step 'Ausgangslage prüfen'
$dirty = git status --porcelain
if ($dirty) { Fail "Im Repo gibt es nicht committete Änderungen:`n$dirty`nErst committen oder verwerfen." }
Run git @('switch', $Branch) "Wechsel auf den Branch $Branch"

if (-not $NoMerge) {
    Step 'Neues von upstream holen'
    Run git @('fetch', 'upstream') 'git fetch upstream'
    Run git @('fetch', 'origin') 'git fetch origin'
    $behind = (git rev-list --count "HEAD..upstream/main").Trim()
    if ($behind -eq '0') {
        Write-Host 'Schon auf dem neuesten Stand von upstream.'
    } else {
        Write-Host "$behind neue Commits von upstream werden zusammengeführt."
        git merge --no-edit upstream/main
        if ($LASTEXITCODE -ne 0) {
            git merge --abort
            Fail ("Konflikt beim Zusammenführen mit upstream. Nichts wurde geändert.`n" +
                  'Meist an einer Zeile mit "assistant-hook (KWordCraft)". Bitte Claude: "Update WordCraft".')
        }
    }
}

Step 'Sprachdaten (craft-i18n) prüfen'
$rev = (Get-Content (Join-Path $Repo 'craft-i18n.rev') -Raw).Trim()
if (-not (Test-Path $I18n)) {
    Run git @('clone', '--filter=blob:none', '--no-checkout', 'https://github.com/storytold/craft-i18n.git', $I18n) 'craft-i18n klonen'
}
$have = (git -C $I18n rev-parse HEAD)
if ($have -ne $rev) {
    Run git @('-C', $I18n, 'fetch', 'origin') 'craft-i18n holen'
    Run git @('-C', $I18n, 'checkout', '-q', $rev) "craft-i18n auf $rev setzen"
}
Write-Host "craft-i18n: $rev"
$env:CRAFT_I18N_DIR = $I18n

Step 'Assistent testen'
Run cargo @('test', '-q', '-p', 'wordcraft-assistant', '-p', 'wordcraft-assistant-ui') 'Tests des Assistenten'

Step 'Release-Version bauen (das dauert einige Minuten)'
Run cargo @('build', '--release', '-p', 'wordcraft') 'Release-Build'

Step "Installieren nach $Bin"
$exe = Join-Path $Bin 'wordcraft.exe'
$running = Get-Process -Name 'wordcraft' -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe }
if ($running) { Fail 'WordCraft läuft noch. Bitte schließen und das Skript noch einmal starten.' }
New-Item -ItemType Directory -Force $Bin | Out-Null
if (Test-Path $exe) { Copy-Item $exe (Join-Path $Bin 'wordcraft.prev.exe') -Force }
Copy-Item (Join-Path $Repo 'target\release\wordcraft.exe') $exe -Force
Write-Host "Installiert: $exe"

$desktop = [Environment]::GetFolderPath('Desktop')
$shell = New-Object -ComObject WScript.Shell
$app = Join-Path $desktop 'WordCraft.lnk'
if (-not (Test-Path $app)) {
    $lnk = $shell.CreateShortcut($app)
    $lnk.TargetPath = $exe
    $lnk.WorkingDirectory = $Bin
    $lnk.Save()
    Write-Host "Verknüpfung angelegt: $app"
}
$upd = Join-Path $desktop 'WordCraft aktualisieren.lnk'
if (-not (Test-Path $upd)) {
    $lnk = $shell.CreateShortcut($upd)
    $lnk.TargetPath = 'powershell.exe'
    $lnk.Arguments = "-NoExit -ExecutionPolicy Bypass -File `"$PSCommandPath`""
    $lnk.WorkingDirectory = $Repo
    $lnk.Save()
    Write-Host "Verknüpfung angelegt: $upd"
}

if ($NoPush) {
    Step 'GitHub: übersprungen (-NoPush)'
} else {
    Step 'Fork auf GitHub (origin) aktualisieren'
    Run git @('push', 'origin', $Branch) "git push origin $Branch"
    if (-not $NoMerge) {
        # Only a fast-forward: the fork's main is never overwritten.
        git push origin 'upstream/main:refs/heads/main'
        if ($LASTEXITCODE -ne 0) {
            Write-Host 'Hinweis: main auf GitHub weicht von upstream ab und wurde nicht verändert.' -ForegroundColor Yellow
        }
    }
}

Step 'Fertig'
Write-Host 'WordCraft ist aktuell. Starten über die Desktop-Verknüpfung "WordCraft".' -ForegroundColor Green
