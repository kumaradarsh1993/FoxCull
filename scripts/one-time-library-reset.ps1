param(
  [string]$BackupRoot = ""
)

$ErrorActionPreference = "Stop"

if (-not $BackupRoot) {
  $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
  $BackupRoot = "D:\FoxCullLibraryResetBackup-$stamp"
}

$sqlite = "D:\android-dev\sdk\platform-tools\sqlite3.exe"
$libraryNames = @("_FoxCull", "_FoxCullCodex")

New-Item -ItemType Directory -Force -Path $BackupRoot | Out-Null

function Assert-RootLibrary {
  param(
    [string]$DriveRoot,
    [System.IO.DirectoryInfo]$Dir
  )

  $leafOk = $libraryNames -contains $Dir.Name
  $parentOk = $Dir.Parent -and ($Dir.Parent.FullName.TrimEnd("\") -eq $DriveRoot.TrimEnd("\"))
  if (-not ($leafOk -and $parentOk)) {
    throw "Refusing to touch unexpected path: $($Dir.FullName)"
  }
}

function Copy-CatalogSidecars {
  param(
    [string]$Catalog,
    [string]$Dest
  )

  foreach ($suffix in @("", "-wal", "-shm")) {
    $src = "$Catalog$suffix"
    if (Test-Path -LiteralPath $src -PathType Leaf) {
      Copy-Item -LiteralPath $src -Destination (Join-Path $Dest ("catalog.sqlite$suffix")) -Force
    }
  }
}

function Get-TrashRows {
  param([string]$Catalog)

  if (-not (Test-Path -LiteralPath $sqlite -PathType Leaf)) { return @() }
  if (-not (Test-Path -LiteralPath $Catalog -PathType Leaf)) { return @() }

  $rows = & $sqlite $Catalog "SELECT stored || char(9) || orig FROM trash;"
  if ($LASTEXITCODE -ne 0 -or -not $rows) { return @() }
  return @($rows | ForEach-Object {
    $parts = $_ -split "`t", 2
    if ($parts.Count -eq 2) {
      [pscustomobject]@{ Stored = $parts[0]; Orig = $parts[1] }
    }
  } | Where-Object { $_ })
}

function Resolve-LibraryChild {
  param(
    [string]$Base,
    [string]$Relative
  )

  $candidate = Join-Path $Base ($Relative -replace "/", "\")
  $fullBase = [System.IO.Path]::GetFullPath($Base).TrimEnd("\") + "\"
  $fullCandidate = [System.IO.Path]::GetFullPath($candidate)
  if (-not $fullCandidate.StartsWith($fullBase, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing path traversal: $Relative"
  }
  return $fullCandidate
}

$summary = [System.Collections.Generic.List[object]]::new()
$roots = Get-PSDrive -PSProvider FileSystem |
  Where-Object { $_.Root -and (Test-Path -LiteralPath $_.Root) } |
  Select-Object -ExpandProperty Root

foreach ($root in $roots) {
  foreach ($name in $libraryNames) {
    $path = Join-Path $root $name
    if (-not (Test-Path -LiteralPath $path -PathType Container)) { continue }

    $dir = Get-Item -LiteralPath $path -Force
    Assert-RootLibrary -DriveRoot $root -Dir $dir

    $backupName = (($root.TrimEnd("\") -replace ":", "") + "__" + $name)
    $backupDir = Join-Path $BackupRoot $backupName
    New-Item -ItemType Directory -Force -Path $backupDir | Out-Null

    $catalog = Join-Path $path "catalog.sqlite"
    Copy-CatalogSidecars -Catalog $catalog -Dest $backupDir

    $restoreRows = Get-TrashRows -Catalog $catalog
    $restored = 0
    $leftInTrash = 0
    foreach ($row in $restoreRows) {
      $src = Resolve-LibraryChild -Base (Join-Path $path "recycle") -Relative $row.Stored
      $dst = Resolve-LibraryChild -Base $root -Relative $row.Orig
      if ((Test-Path -LiteralPath $src) -and -not (Test-Path -LiteralPath $dst)) {
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $dst) | Out-Null
        Move-Item -LiteralPath $src -Destination $dst
        $restored += 1
      } elseif (Test-Path -LiteralPath $src) {
        $leftInTrash += 1
      }
    }

    $files = Get-ChildItem -LiteralPath $path -Recurse -Force -File -ErrorAction SilentlyContinue
    $manifest = [pscustomobject]@{
      drive = $root
      library = $name
      path = $path
      backedUpTo = $backupDir
      filesBeforeDelete = @($files).Count
      bytesBeforeDelete = (($files | Measure-Object Length -Sum).Sum)
      restoredTrashItems = $restored
      trashItemsLeftDueToCollision = $leftInTrash
      removedAt = (Get-Date).ToString("o")
    }
    $manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $backupDir "manifest.json") -Encoding UTF8

    Remove-Item -LiteralPath $path -Recurse -Force
    $summary.Add($manifest)
  }
}

$summary | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $BackupRoot "summary.json") -Encoding UTF8
$summary | Format-Table drive, library, filesBeforeDelete, restoredTrashItems, trashItemsLeftDueToCollision, backedUpTo -AutoSize
