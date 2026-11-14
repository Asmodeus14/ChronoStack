# SPDX-License-Identifier: MIT OR Apache-2.0
# Install the ChronoStack CLI on Windows (x64).
#
#   irm https://github.com/Asmodeus14/ChronoStack/releases/latest/download/install.ps1 | iex
#
# Options (environment variables):
#   CHRONOSTACK_VERSION        version to install (default: the newest release, including pre-releases)
#   CHRONOSTACK_INSTALL_DIR    where to put chronostack.exe (default: %LOCALAPPDATA%\chronostack\bin)
#   CHRONOSTACK_DOWNLOAD_BASE  download from here instead of GitHub Releases (testing)
#   CHRONOSTACK_NO_MODIFY_PATH set to 1 to leave PATH alone
#
# The archive is verified against the release's SHA256SUMS before anything is installed.
# No admin rights. The install folder is added to your user PATH (for new terminals)
# and to this session's PATH, so `chronostack` works right away.
$ErrorActionPreference = 'Stop'
$repo = 'Asmodeus14/ChronoStack'
$dest = if ($env:CHRONOSTACK_INSTALL_DIR) { $env:CHRONOSTACK_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'chronostack\bin' }

if (-not [Environment]::Is64BitOperatingSystem) { throw 'chronostack-install: only 64-bit Windows is supported' }
if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { Write-Warning 'chronostack-install: no native ARM64 build; installing the x64 binary (runs under emulation)' }

$version = $env:CHRONOSTACK_VERSION
if (-not $version) {
  $rel = Invoke-RestMethod -UseBasicParsing "https://api.github.com/repos/$repo/releases?per_page=1"
  if (-not $rel) { throw 'chronostack-install: no release found' }
  $version = @($rel)[0].tag_name
}
$version = $version.TrimStart('v')

$base = if ($env:CHRONOSTACK_DOWNLOAD_BASE) { $env:CHRONOSTACK_DOWNLOAD_BASE } else { "https://github.com/$repo/releases/download/v$version" }
$file = "chronostack-$version-win32-x64.zip"
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("chronostack-" + [Guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
  Write-Host "chronostack-install: downloading $file"
  Invoke-WebRequest -UseBasicParsing "$base/$file" -OutFile (Join-Path $tmp $file)
  Invoke-WebRequest -UseBasicParsing "$base/SHA256SUMS" -OutFile (Join-Path $tmp 'SHA256SUMS')
  $line = Get-Content (Join-Path $tmp 'SHA256SUMS') | Where-Object { ($_ -split '\s+')[1] -in @($file, "*$file") } | Select-Object -First 1
  if (-not $line) { throw "chronostack-install: $file is not listed in SHA256SUMS" }
  $expected = ($line -split '\s+')[0].ToLowerInvariant()
  $actual = (Get-FileHash -Algorithm SHA256 (Join-Path $tmp $file)).Hash.ToLowerInvariant()
  if ($expected -ne $actual) { throw "chronostack-install: checksum mismatch for $file" }
  Write-Host 'chronostack-install: checksum ok'
  Expand-Archive -Path (Join-Path $tmp $file) -DestinationPath (Join-Path $tmp 'x') -Force
  New-Item -ItemType Directory -Force -Path $dest | Out-Null
  Copy-Item (Join-Path $tmp 'x\chronostack.exe') (Join-Path $dest 'chronostack.exe') -Force
} finally {
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($env:CHRONOSTACK_NO_MODIFY_PATH -ne '1') {
  # New terminals: the user PATH (no admin needed).
  if (-not (($userPath -split ';') -contains $dest)) {
    [Environment]::SetEnvironmentVariable('Path', ($userPath.TrimEnd(';') + ';' + $dest).TrimStart(';'), 'User')
    Write-Host "chronostack-install: added $dest to your user PATH"
  }
  # This terminal: `irm ... | iex` runs in your own session, so update it too.
  if (-not (($env:Path -split ';') -contains $dest)) {
    $env:Path = "$dest;$env:Path"
  }
}
$ver = & (Join-Path $dest 'chronostack.exe') --version
Write-Host "chronostack-install: installed $ver to $dest"
if ((Get-Command chronostack -ErrorAction SilentlyContinue)) {
  Write-Host "chronostack-install: ready: type 'chronostack' in this terminal (new terminals find it too)"
} else {
  Write-Host "chronostack-install: open a new terminal, or run:  `$env:Path = `"$dest;`$env:Path`""
}
Write-Host "chronostack-install: next: cd into a Docker Compose project and run 'chronostack up --wait'"
