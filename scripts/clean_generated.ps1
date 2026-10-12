[CmdletBinding()]
param(
    [switch]$IncludeBuildCache,
    [switch]$DryRun
)
$ErrorActionPreference = 'Stop'
$workspaceRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path.TrimEnd('\')
$rootPrefix = $workspaceRoot + '\'
$cleanupTargets = [System.Collections.Generic.List[string]]::new()
$qaRoot = Join-Path $workspaceRoot 'qa-output'
if (Test-Path -LiteralPath $qaRoot) {
    foreach ($item in Get-ChildItem -LiteralPath $qaRoot -Force) {
        if ($item.Name -eq 'sequential') {
            foreach ($child in Get-ChildItem -LiteralPath $item.FullName -Force) {
                if ($child.Name -ne 'verification.md') { $cleanupTargets.Add($child.FullName) }
            }
        } else { $cleanupTargets.Add($item.FullName) }
    }
}
if ($IncludeBuildCache -and (Test-Path -LiteralPath (Join-Path $workspaceRoot 'target'))) {
    foreach ($item in Get-ChildItem -LiteralPath (Join-Path $workspaceRoot 'target') -Force) {
        if ($item.Name -in @('debug', 'release')) {
            foreach ($child in Get-ChildItem -LiteralPath $item.FullName -Force) {
                if ($child.Name -ne 'photo_editor.exe') { $cleanupTargets.Add($child.FullName) }
            }
        } else { $cleanupTargets.Add($item.FullName) }
    }
}
foreach ($name in @('tests', 'scripts')) {
    foreach ($item in Get-ChildItem -LiteralPath (Join-Path $workspaceRoot $name) -Directory -Recurse -Force) {
        if ($item.Name -eq '__pycache__') { $cleanupTargets.Add($item.FullName) }
    }
}
foreach ($targetPath in $cleanupTargets) {
    $resolvedTarget = (Resolve-Path -LiteralPath $targetPath).Path
    if (-not $resolvedTarget.StartsWith($rootPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw 'Cleanup target escaped the workspace.'
    }
    $cursor = Get-Item -LiteralPath $resolvedTarget -Force
    while ($null -ne $cursor -and $cursor.FullName -ne $workspaceRoot) {
        if ($cursor.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
            throw 'Cleanup refuses paths through a symbolic link or junction.'
        }
        if ($cursor.PSIsContainer) { $cursor = $cursor.Parent } else { $cursor = $cursor.Directory }
    }
    if ((Get-Item -LiteralPath $resolvedTarget).PSIsContainer) {
        $links = @(Get-ChildItem -LiteralPath $resolvedTarget -Recurse -Force |
            Where-Object { $_.Attributes -band [System.IO.FileAttributes]::ReparsePoint })
        if ($links.Count -gt 0) { throw 'Cleanup refuses a directory containing links or junctions.' }
    }
}
$fileCount = 0
$byteCount = 0L
foreach ($targetPath in $cleanupTargets) {
    $item = Get-Item -LiteralPath $targetPath -Force
    $files = if ($item.PSIsContainer) { @(Get-ChildItem -LiteralPath $targetPath -File -Recurse -Force) } else { @($item) }
    $fileCount += $files.Count
    foreach ($file in $files) { $byteCount += $file.Length }
    if (-not $DryRun) { Remove-Item -LiteralPath $targetPath -Recurse -Force }
}
[pscustomobject]@{ DryRun = [bool]$DryRun; Files = $fileCount; Bytes = $byteCount; Targets = $cleanupTargets.Count }
