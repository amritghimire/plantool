# Install plantool on Windows: download the release zip and SHA256SUMS.txt, verify, and put
# plantool.exe on the user's PATH.
#
#   irm https://raw.githubusercontent.com/amritghimire/plantool/main/install.ps1 | iex
#
# Environment: PLANTOOL_VERSION (tag, default latest), PLANTOOL_INSTALL_DIR (default %LOCALAPPDATA%\plantool\bin)
$ErrorActionPreference = "Stop"
$repo = "amritghimire/plantool"
if ($env:PROCESSOR_ARCHITECTURE -ne "AMD64") { throw "plantool ships only for x64 Windows (found $env:PROCESSOR_ARCHITECTURE)" }
$version = $env:PLANTOOL_VERSION
if (-not $version) {
  try {
    $version = (Invoke-RestMethod -Headers @{ Accept = "application/vnd.github+json" } "https://api.github.com/repos/$repo/releases/latest").tag_name
  } catch {
    $version = (Invoke-RestMethod -Headers @{ Accept = "application/vnd.github+json" } "https://api.github.com/repos/$repo/releases?per_page=1")[0].tag_name
  }
}
$archive = "plantool-windows-x64.zip"
$base = "https://github.com/$repo/releases/download/$version"
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("plantool-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
  Write-Host "downloading plantool $version for windows-x64"
  Invoke-WebRequest "$base/$archive" -OutFile (Join-Path $tmp $archive)
  Invoke-WebRequest "$base/SHA256SUMS.txt" -OutFile (Join-Path $tmp "SHA256SUMS.txt")
  $line = Get-Content (Join-Path $tmp "SHA256SUMS.txt") | Where-Object { $_ -match "\s\*?$([regex]::Escape($archive))$" } | Select-Object -First 1
  if (-not $line) { throw "$archive is not listed in SHA256SUMS.txt" }
  $expected = ($line -split "\s+")[0].ToLower()
  $actual = (Get-FileHash (Join-Path $tmp $archive) -Algorithm SHA256).Hash.ToLower()
  if ($actual -ne $expected) { throw "checksum mismatch for $archive (expected $expected, got $actual)" }
  Expand-Archive -Force (Join-Path $tmp $archive) -DestinationPath $tmp
  $dir = $env:PLANTOOL_INSTALL_DIR
  if (-not $dir) { $dir = Join-Path $env:LOCALAPPDATA "plantool\bin" }
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
  Move-Item -Force (Join-Path $tmp "plantool.exe") (Join-Path $dir "plantool.exe")
  $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
  if (-not (($userPath -split ";") -contains $dir)) {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$dir", "User")
    $env:Path = "$env:Path;$dir"
    Write-Host "added $dir to your user PATH (open a new terminal to pick it up)"
  }
  Write-Host "installed $(& (Join-Path $dir 'plantool.exe') --version) to $dir"
  Write-Host "next: cd into a repo and run   plantool new <slug>"
} finally {
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
