[CmdletBinding()]
param(
    [switch]$IncludeBuildCache,
    [switch]$KeepLatestBuildCache,
    [switch]$DryRun
)
$ErrorActionPreference = 'Stop'
$workspaceRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path.TrimEnd('\')
$rootPrefix = $workspaceRoot + '\'
$cleanupTargets = [System.Collections.Generic.List[string]]::new()
$keptProfile = $null
if ($IncludeBuildCache -and $KeepLatestBuildCache) { throw 'Select one build cache cleanup mode.' }
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
if ($KeepLatestBuildCache) {
    $manifestPath = Join-Path $workspaceRoot 'target/latest-build.json'
    $messages = @(Get-Content -LiteralPath $manifestPath -Encoding utf8 | ForEach-Object { $_ | ConvertFrom-Json })
    if (-not $messages.Count -or $messages[-1].reason -ne 'build-finished' -or -not $messages[-1].success) {
        throw 'Latest build manifest does not confirm a successful build.'
    }
    $app = $messages | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'photo_editor' -and $_.executable } | Select-Object -Last 1
    if (-not $app) { throw 'Latest application artifact is missing.' }
    $profileRoot = Split-Path -Parent $app.executable
    $keptProfile = Split-Path -Leaf $profileRoot
    if ($keptProfile -notin @('debug','release') -or $profileRoot -ne (Join-Path $workspaceRoot ('target/' + $keptProfile))) {
        throw 'Latest build profile escaped the workspace.'
    }
    $profilePrefix = $profileRoot + '\'
    $hashes = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
    $buildDirs = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
    $artifactNames = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
    foreach ($message in $messages) {
        if ($message.reason -eq 'compiler-artifact') {
            $hashedArtifact = $false
            foreach ($filename in $message.filenames) {
                $absolute = [System.IO.Path]::GetFullPath($filename)
                if (-not $absolute.StartsWith($profilePrefix, [System.StringComparison]::OrdinalIgnoreCase)) { throw 'Build artifact escaped the latest profile.' }
                $name = Split-Path -Leaf $absolute
                [void]$artifactNames.Add($name)
                if ($name -match '-([0-9a-f]{16})(?:\.|$)') { [void]$hashes.Add($Matches[1]); $hashedArtifact = $true }
                $relative = $absolute.Substring($profilePrefix.Length)
                if ($relative -match '^build[\\/]([^\\/]+)') {
                    [void]$buildDirs.Add($Matches[1])
                    if ($Matches[1] -match '-([0-9a-f]{16})$') { [void]$hashes.Add($Matches[1]) }
                }
            }
            # Libraries with cdylib/staticlib outputs can have no hash in the
            # filenames. Their Cargo fingerprint and dep-info are still needed.
            if (-not $hashedArtifact -and -not $message.executable -and $message.target.kind -notcontains 'custom-build') {
                $marker = 'lib-' + $message.target.name
                $artifactTime = (Get-Item -LiteralPath $message.filenames[0]).LastWriteTimeUtc
                $fingerprint = Get-ChildItem -LiteralPath (Join-Path $profileRoot '.fingerprint') -Directory |
                    ForEach-Object { Get-Item -LiteralPath (Join-Path $_.FullName $marker) -ErrorAction SilentlyContinue } |
                    Sort-Object { [Math]::Abs(($_.LastWriteTimeUtc - $artifactTime).TotalSeconds) } | Select-Object -First 1
                if (-not $fingerprint) { throw 'Unhashed library fingerprint is missing.' }
                if ($fingerprint.Directory.Name -match '-([0-9a-f]{16})$') { [void]$hashes.Add($Matches[1]) }
                [void]$artifactNames.Add($message.target.name + '.d')
            }
        } elseif ($message.reason -eq 'build-script-executed') {
            $absolute = [System.IO.Path]::GetFullPath($message.out_dir)
            if (-not $absolute.StartsWith($profilePrefix, [System.StringComparison]::OrdinalIgnoreCase)) { throw 'Build output escaped the latest profile.' }
            $relative = $absolute.Substring($profilePrefix.Length)
            if ($relative -match '^build[\\/]([^\\/]+)') {
                [void]$buildDirs.Add($Matches[1])
                if ($Matches[1] -match '-([0-9a-f]{16})$') { [void]$hashes.Add($Matches[1]) }
            }
        }
    }
    # Cargo reports the root executable rather than its hashed bin artifact.
    $appTime = (Get-Item -LiteralPath $app.executable).LastWriteTimeUtc
    $bin = Get-ChildItem -LiteralPath (Join-Path $profileRoot '.fingerprint') -Directory -Filter 'photo_editor-*' |
        ForEach-Object { Get-Item -LiteralPath (Join-Path $_.FullName 'bin-photo_editor') -ErrorAction SilentlyContinue } |
        Sort-Object { [Math]::Abs(($_.LastWriteTimeUtc - $appTime).TotalSeconds) } | Select-Object -First 1
    if (-not $bin) { throw 'Latest application fingerprint is missing.' }
    if ($bin.Directory.Name -match '-([0-9a-f]{16})$') { [void]$hashes.Add($Matches[1]) }
    foreach ($item in Get-ChildItem -LiteralPath (Join-Path $workspaceRoot 'target') -Force) {
        if ($item.Name -eq $keptProfile) {
            foreach ($child in Get-ChildItem -LiteralPath $item.FullName -Force) {
                if ($child.Name -in @('deps','.fingerprint')) {
                    foreach ($artifact in Get-ChildItem -LiteralPath $child.FullName -Force) {
                        $keep = $artifactNames.Contains($artifact.Name)
                        if ($artifact.Name -match '-([0-9a-f]{16})(?:\.|$)') { $keep = $hashes.Contains($Matches[1]) }
                        if (-not $keep) { $cleanupTargets.Add($artifact.FullName) }
                    }
                } elseif ($child.Name -eq 'build') {
                    foreach ($directory in Get-ChildItem -LiteralPath $child.FullName -Force) {
                        if (-not $buildDirs.Contains($directory.Name)) { $cleanupTargets.Add($directory.FullName) }
                    }
                } elseif ($child.PSIsContainer) {
                    $cleanupTargets.Add($child.FullName)
                } elseif ($child.Name -notin @('photo_editor.exe','photo_editor.pdb','photo_editor.d','.cargo-lock') -and -not $artifactNames.Contains($child.Name)) {
                    $cleanupTargets.Add($child.FullName)
                }
            }
        } elseif ($item.Name -in @('debug','release')) {
            $cleanupTargets.Add($item.FullName)
        } elseif ($item.Name -notin @('.rustc_info.json','latest-build.json')) {
            $cleanupTargets.Add($item.FullName)
        }
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
[pscustomobject]@{ DryRun = [bool]$DryRun; Files = $fileCount; Bytes = $byteCount; Targets = $cleanupTargets.Count; KeptBuildProfile = $keptProfile }
