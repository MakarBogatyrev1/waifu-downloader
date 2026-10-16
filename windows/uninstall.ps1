param([int]$OldPid)
$ErrorActionPreference='Stop'
Wait-Process -Id $OldPid -ErrorAction SilentlyContinue
$program=[IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'Programs\WaifuGallery'))
$expected=[IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'Programs\WaifuGallery'))
if($program -ne $expected -or !(Test-Path -LiteralPath (Join-Path $program '.installed'))) {exit 1}
$menu=Join-Path ([Environment]::GetFolderPath('Programs')) 'Waifu Gallery'
Remove-Item -LiteralPath (Join-Path $menu 'Waifu Gallery.lnk') -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $menu -ErrorAction SilentlyContinue
foreach($name in @('WaifuGallery.exe','README.md','LICENSE','THIRD-PARTY-LICENSES.txt','.installed','WaifuGallery.exe.previous')) {Remove-Item -LiteralPath (Join-Path $program $name) -Force -ErrorAction SilentlyContinue}
Remove-Item -LiteralPath $program -ErrorAction SilentlyContinue
Remove-Item -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\WaifuGallery.Desktop' -Recurse -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue
