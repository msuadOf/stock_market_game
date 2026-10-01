[CmdletBinding(PositionalBinding = $false)]
param([switch]$Worker, [switch]$Cleanup, [string]$OwnedWork, [Parameter(Position = 0, ValueFromRemainingArguments = $true)][string[]]$BuildArgs)
$ErrorActionPreference = 'Stop'
try {
    $root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
    Set-Location $root
    if ($Cleanup) {
        if (-not $OwnedWork -or [IO.Path]::GetDirectoryName($OwnedWork) -ne (Join-Path $root 'target/build-work') -or [IO.Path]::GetFileName($OwnedWork) -notmatch '^server-[0-9a-f]{32}$') { throw 'cleanup requires a validated supervisor-owned work ID.' }
        foreach ($directory in @((Join-Path $root 'target'), (Join-Path $root 'target/build-work'), $OwnedWork)) {
            $item = Get-Item -Force -LiteralPath $directory
            if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw "unsafe owned-work cleanup directory: $directory" }
        }
        Remove-Item -LiteralPath $OwnedWork -Recurse -Force
        exit 0
    }
    if ($OwnedWork) { throw 'OwnedWork is only available in the internal cleanup mode.' }
    $jobs = [Environment]::ProcessorCount
    $output = 'target/build-artifacts/server'
    $dryRun = $false
    $jobsSeen = $false
    $outputSeen = $false
    $arguments = @($BuildArgs)
    if ($arguments.Count -gt 0 -and $arguments[0] -eq 'server') { $arguments = @($arguments | Select-Object -Skip 1) }
    if ($arguments.Count -eq 1 -and $arguments[0] -in @('--help', '-h')) {
        Write-Output 'Usage: scripts\server-build.bat [--jobs N] [--dry-run] [--output target/build-artifacts/NAME]'; exit 0
    }
    for ($index = 0; $index -lt $arguments.Count; $index++) {
        switch ($arguments[$index]) {
            '--jobs' {
                $index++
                if ($jobsSeen -or $index -ge $arguments.Count -or $arguments[$index] -notmatch '^[1-9][0-9]{0,5}$') { throw '--jobs requires a positive integer (maximum 999999), once.' }
                $jobs = [int]$arguments[$index]; $jobsSeen = $true
            }
            '--output' {
                $index++
                if ($outputSeen -or $index -ge $arguments.Count) { throw '--output requires a directory, once.' }
                $output = $arguments[$index]; $outputSeen = $true
            }
            '--dry-run' {
                if ($dryRun) { throw 'duplicate option: --dry-run.' }
                $dryRun = $true
            }
            default { throw "unknown argument: $($arguments[$index])" }
        }
    }
    $artifactRoot = [IO.Path]::GetFullPath((Join-Path $root 'target/build-artifacts'))
    $destination = [IO.Path]::GetFullPath((Join-Path $root $output))
    if ([IO.Path]::GetDirectoryName($destination) -ne $artifactRoot -or [IO.Path]::GetFileName($destination) -notmatch '^[a-zA-Z0-9][a-zA-Z0-9_-]*$') { throw '--output must be a direct child: target/build-artifacts/NAME.' }
    Write-Output "[build server] jobs=$jobs; shared deadline=300000ms; execution/publishing <=298000ms, termination <=299000ms, final 1000ms reserved for cleanup."
    Write-Output 'Build requirements: Rust/Cargo and Windows PowerShell. Deployment: native executable + LICENSE; no build tools.'
    Write-Output 'Native target command: rustc -vV (host field); dry-run does not probe Rust.'
    Write-Output "cargo build --locked --release -p server --bin server --jobs $jobs --target <rustc-host>"
    Write-Output "Artifact: $destination\server.exe + LICENSE"
    Write-Output "Start: $destination\server.exe --services server"
    Write-Output 'Default existing outputs are refused; choose --output target/build-artifacts/NAME for repeat builds.'
    if ($dryRun) { Write-Output 'Mode: dry-run (no build commands executed; no artifact created).'; exit 0 }
    $deadline = [Diagnostics.Stopwatch]::StartNew()
    foreach ($directory in @('target', 'target/build-cache', 'target/build-cache/server', 'target/build-work', 'target/build-artifacts')) {
        if (Test-Path -LiteralPath $directory) {
            $item = Get-Item -Force -LiteralPath $directory
            if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw "unsafe directory (symlink/reparse point or non-directory): $directory" }
        } else { New-Item -ItemType Directory -Path $directory | Out-Null }
    }
    if (Test-Path -LiteralPath $destination) { throw "refusing existing output: $destination; choose --output target/build-artifacts/NAME." }
    if (-not $Worker) {
        $work = Join-Path $root ('target/build-work/server-' + [guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $work | Out-Null
        $workerTerminated = $true
        try {
            New-Item -ItemType Directory -Path (Join-Path $work 'artifact') | Out-Null
            $env:STOCK_SERVER_BUILD_WORK = $work
            $quotedArguments = @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $PSCommandPath, '-Worker') + $arguments
            $commandLine = ($quotedArguments | ForEach-Object { '"' + $_.Replace('"', '\"') + '"' }) -join ' '
            $child = Start-Process -FilePath (Join-Path $PSHOME 'powershell.exe') -ArgumentList $commandLine -NoNewWindow -PassThru
            $workerTerminated = $false
            $remaining = [Math]::Max(0, 298000 - $deadline.ElapsedMilliseconds)
            if (-not $child.WaitForExit([int]$remaining)) {
                $terminator = Start-Process -FilePath 'taskkill.exe' -ArgumentList @('/PID', "$($child.Id)", '/T', '/F') -NoNewWindow -PassThru
                $remaining = [Math]::Max(0, 299000 - $deadline.ElapsedMilliseconds)
                if (-not $terminator.WaitForExit([int]$remaining)) { throw 'termination deadline exhausted; taskkill has not confirmed process-tree termination.' }
                if ($terminator.ExitCode -ne 0 -and -not $child.HasExited) { throw "process-tree termination failed with exit $($terminator.ExitCode)." }
                $remaining = [Math]::Max(0, 299000 - $deadline.ElapsedMilliseconds)
                if (-not $child.WaitForExit([int]$remaining)) { throw 'build process did not close within its termination budget.' }
                $workerTerminated = $true
                throw 'shared execution deadline exhausted; build process tree terminated.'
            }
            $workerTerminated = $true
            $status = $child.ExitCode
        } finally {
            if (-not $workerTerminated) { throw "build process-tree termination not confirmed; cleanup cannot safely run: $work" }
            $remaining = [Math]::Max(0, 300000 - $deadline.ElapsedMilliseconds)
            if ($remaining -eq 0) { throw "shared deadline exhausted; owned-work cleanup not completed: $work" }
            $cleanupArguments = @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $PSCommandPath, '-Cleanup', '-OwnedWork', $work)
            $cleanupLine = ($cleanupArguments | ForEach-Object { '"' + $_.Replace('"', '\"') + '"' }) -join ' '
            $cleaner = Start-Process -FilePath (Join-Path $PSHOME 'powershell.exe') -ArgumentList $cleanupLine -NoNewWindow -PassThru
            $remaining = [Math]::Max(0, 300000 - $deadline.ElapsedMilliseconds)
            if (-not $cleaner.WaitForExit([int]$remaining)) {
                $cleaner.Kill()
                throw "shared 300000ms deadline exhausted; owned-work cleanup child killed, cleanup NOT completed: $work"
            }
            if ($cleaner.ExitCode -ne 0) { throw "owned-work cleanup failed with exit $($cleaner.ExitCode): $work" }
        }
        exit $status
    }
    $work = $env:STOCK_SERVER_BUILD_WORK
    if (-not $work -or [IO.Path]::GetDirectoryName($work) -ne (Join-Path $root 'target/build-work') -or [IO.Path]::GetFileName($work) -notmatch '^server-[0-9a-f]{32}$') { throw 'internal worker requires the external deadline supervisor.' }
    $payload = Join-Path $work 'artifact'
    $env:CARGO_TARGET_DIR = Join-Path $root 'target/build-cache/server'
    $env:CARGO_BUILD_JOBS = "$jobs"
    $version = & rustc -vV
    if ($LASTEXITCODE -ne 0) { throw "rustc native-host probe failed with exit $LASTEXITCODE." }
    $hosts = @($version | Where-Object { $_ -match '^host: [a-zA-Z0-9_-]+$' })
    if ($hosts.Count -ne 1) { throw 'rustc -vV did not report exactly one valid native host.' }
    $nativeTarget = $hosts[0].Substring(6)
    & cargo build --locked --release -p server --bin server --jobs $jobs --target $nativeTarget
    if ($LASTEXITCODE -ne 0) { throw "Cargo release build failed with exit $LASTEXITCODE." }
    $executable = Get-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR "$nativeTarget/release/server.exe")
    if ($executable.PSIsContainer -or $executable.Length -eq 0 -or ($executable.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'missing/unsafe native release executable.' }
    $license = Get-Item -LiteralPath (Join-Path $root 'LICENSE')
    if ($license.PSIsContainer -or $license.Length -eq 0 -or ($license.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'root LICENSE missing/unsafe.' }
    Copy-Item -LiteralPath $executable.FullName -Destination (Join-Path $payload 'server.exe')
    Copy-Item -LiteralPath $license.FullName -Destination (Join-Path $payload 'LICENSE')
    [IO.Directory]::Move($payload, $destination)
    Write-Output "[build server] native executable published: $destination\server.exe"
} catch {
    [Console]::Error.WriteLine("[build server] $($_.Exception.Message)")
    exit 1
}
