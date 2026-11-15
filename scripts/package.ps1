param(
    [string[]]$Targets = @('x86_64-pc-windows-msvc', 'i686-pc-windows-msvc', 'aarch64-pc-windows-msvc')
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$arch = @{ 'x86_64-pc-windows-msvc' = 'x64'; 'i686-pc-windows-msvc' = 'x86'; 'aarch64-pc-windows-msvc' = 'arm64' }
$dist = Join-Path $root 'dist'
New-Item -ItemType Directory -Force $dist | Out-Null

$sums = @()
foreach ($target in $Targets) {
    cargo build --release --locked --target $target
    if ($LASTEXITCODE -ne 0) { throw "Build failed for $target" }

    $stage = Join-Path $dist "stage-$($arch[$target])"
    if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
    New-Item -ItemType Directory -Force $stage | Out-Null
    Copy-Item "target\$target\release\mahimarkd_plusplus.dll" (Join-Path $stage 'MahiMarkD++.dll')
    Copy-Item LICENSE, COPYRIGHT, THIRD-PARTY-NOTICES.md $stage

    $zip = Join-Path $dist "MahiMarkD-PlusPlus-$version-$($arch[$target]).zip"
    if (Test-Path $zip) { Remove-Item $zip -Force }
    Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
    Remove-Item $stage -Recurse -Force

    $hash = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLowerInvariant()
    $sums += "$hash  $(Split-Path -Leaf $zip)"
}

$sums | Set-Content -Encoding ascii (Join-Path $dist 'SHA256SUMS.txt')
$sums
