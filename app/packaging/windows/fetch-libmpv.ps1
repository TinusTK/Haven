# Puts what the Windows player needs into packaging\windows\libmpv:
# libmpv-2.dll (the LGPL libmpv, shipped beside haven.exe), mpv.lib (what
# the build links against) and vulkan-1.dll with its licence (the Vulkan
# loader, which libmpv-2.dll cannot load without and which PCs without
# full graphics drivers lack). Run from anywhere; CI runs it before the
# release build.
#
#   pwsh packaging\windows\fetch-libmpv.ps1                 # download (needs gh, signed in)
#   pwsh packaging\windows\fetch-libmpv.ps1 -Archive x.7z   # use a libmpv archive already here
#
# The archive is the LGPL build from https://github.com/zhongfly/mpv-winbuild
# (mpv built with -Dgpl=false, FFmpeg without --enable-gpl). That project
# deletes its releases after 30 days, so a copy is kept as a release of
# Haven's own repository. To move to a newer libmpv: upload the new archive
# as a new release there and change the three values below.
#
# The Vulkan loader is the official runtime zip from LunarG
# (https://vulkan.lunarg.com/sdk/home, "Runtime zip"), also kept as a
# release of Haven's repository so the build does not depend on that site.

param(
    [string]$Archive = ""
)

$ErrorActionPreference = "Stop"

$Repo   = "TinusTK/Haven"
$Tag    = "libmpv-win-20261113"
$File   = "mpv-dev-lgpl-x86_64-20261113-git-fe753a7824.7z"
$Sha256 = "d4f9e4ae675685d1a35d926aac65573cc05e71ea7391c344a91f325f73e4386e"

$VulkanTag    = "vulkan-rt-1.4.363.0"
$VulkanFile   = "VulkanRT-X64-1.4.363.0-Components.zip"
$VulkanSha256 = "a25a927aa8b9f0371048f1861cf88ac3b9bc9b1fb332c42d897c8ab32695769a"

$dest = Join-Path $PSScriptRoot "libmpv"
$work = Join-Path ([System.IO.Path]::GetTempPath()) "haven-libmpv"
if (Test-Path $work) { Remove-Item $work -Recurse -Force }
New-Item $work -ItemType Directory | Out-Null

if (-not $Archive) {
    gh release download $Tag --repo $Repo --pattern $File --dir $work
    if ($LASTEXITCODE -ne 0) { throw "Could not download $File from release $Tag of $Repo" }
    $Archive = Join-Path $work $File
}

# Never use an archive that is not exactly the one that was checked.
$hash = (Get-FileHash $Archive -Algorithm SHA256).Hash.ToLower()
if ($hash -ne $Sha256) { throw "Checksum of $Archive is $hash, expected $Sha256" }

$unpacked = Join-Path $work "unpacked"
7z x $Archive "-o$unpacked" -y | Out-Null
if ($LASTEXITCODE -ne 0) { throw "Could not unpack $Archive (is 7-Zip installed?)" }
Copy-Item (Join-Path $unpacked "libmpv-2.dll") $dest -Force

# The archive has no import library the MSVC linker can use, so make one
# from the functions the DLL exports. lib.exe and dumpbin.exe come with the
# Visual Studio C++ build tools.
$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
if (-not (Test-Path $vswhere)) { throw "Visual Studio C++ build tools not found (no vswhere.exe)" }
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw "Visual Studio C++ build tools not found" }
$bin = Get-ChildItem (Join-Path $vs "VC\Tools\MSVC") -Directory |
    Sort-Object { [version]$_.Name } | Select-Object -Last 1 |
    ForEach-Object { Join-Path $_.FullName "bin\Hostx64\x64" }

$dll = Join-Path $dest "libmpv-2.dll"
$exports = & (Join-Path $bin "dumpbin.exe") /nologo /exports $dll | ForEach-Object {
    # Only mpv's own functions: the DLL also exports a few from libraries inside it.
    if ($_ -match '^\s+\d+\s+[0-9A-Fa-f]+\s+[0-9A-Fa-f]+\s+(mpv_\w+)\s*$') { $Matches[1] }
}
if (-not $exports -or $exports.Count -lt 40) { throw "Could not read the functions libmpv-2.dll exports" }

$def = Join-Path $dest "mpv.def"
Set-Content $def (@("LIBRARY libmpv-2.dll", "EXPORTS") + $exports) -Encoding ascii
& (Join-Path $bin "lib.exe") /nologo "/def:$def" /name:libmpv-2.dll "/out:$(Join-Path $dest 'mpv.lib')" /machine:x64
if ($LASTEXITCODE -ne 0) { throw "lib.exe could not make mpv.lib" }

# The Vulkan loader, from the official runtime zip.
gh release download $VulkanTag --repo $Repo --pattern $VulkanFile --dir $work
if ($LASTEXITCODE -ne 0) { throw "Could not download $VulkanFile from release $VulkanTag of $Repo" }
$vulkanZip = Join-Path $work $VulkanFile
$hash = (Get-FileHash $vulkanZip -Algorithm SHA256).Hash.ToLower()
if ($hash -ne $VulkanSha256) { throw "Checksum of $VulkanFile is $hash, expected $VulkanSha256" }
$vulkan = Join-Path $work "vulkan"
Expand-Archive $vulkanZip -DestinationPath $vulkan
$vulkanDll = Get-ChildItem $vulkan -Recurse -Filter vulkan-1.dll | Where-Object { $_.Directory.Name -eq "x64" }
$vulkanLicence = Get-ChildItem $vulkan -Recurse -Filter VulkanRT-License.txt
if (-not $vulkanDll -or -not $vulkanLicence) { throw "$VulkanFile does not hold x64\vulkan-1.dll and VulkanRT-License.txt" }
Copy-Item $vulkanDll.FullName $dest -Force
Copy-Item $vulkanLicence.FullName $dest -Force

Write-Host "Ready in ${dest}: libmpv-2.dll, mpv.lib ($($exports.Count) functions), vulkan-1.dll"
