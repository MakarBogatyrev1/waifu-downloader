param([Parameter(Mandatory=$true)][string]$SourceExe)
$ErrorActionPreference='Stop'
$program=Join-Path $env:LOCALAPPDATA 'Programs\WaifuGallery'
$data=Join-Path $env:LOCALAPPDATA 'WaifuGallery'
$sourceFolder=Split-Path -Parent $SourceExe
$target=Join-Path $program 'WaifuGallery.exe'
New-Item -ItemType Directory -Force -Path $program,$data | Out-Null
if([IO.Path]::GetFullPath($SourceExe) -ne [IO.Path]::GetFullPath($target)) { Copy-Item -LiteralPath $SourceExe -Destination $target -Force }
foreach($file in @('README.md','LICENSE','THIRD-PARTY-LICENSES.txt')) {
    $src=Join-Path $sourceFolder $file
    if(Test-Path -LiteralPath $src) { Copy-Item -LiteralPath $src -Destination (Join-Path $program $file) -Force }
}
if(!(Test-Path -LiteralPath (Join-Path $data 'settings-rust.json')) -and (Test-Path -LiteralPath (Join-Path $sourceFolder 'settings-rust.json'))) {
    Copy-Item -LiteralPath (Join-Path $sourceFolder 'settings-rust.json') -Destination $data
    foreach($file in @('settings.json')) {if(Test-Path -LiteralPath (Join-Path $sourceFolder $file)){Copy-Item -LiteralPath (Join-Path $sourceFolder $file) -Destination $data}}
    $copiedImages=$false
    $images=Join-Path $sourceFolder 'images'
    if((Test-Path -LiteralPath $images) -and !(Test-Path -LiteralPath (Join-Path $data 'images'))) {Copy-Item -LiteralPath $images -Destination $data -Recurse;$copiedImages=$true}
    $settings=Get-Content -Encoding UTF8 -Raw -LiteralPath (Join-Path $data 'settings-rust.json') | ConvertFrom-Json
    if($settings.custom_save_folder -and ![IO.Path]::IsPathRooted($settings.custom_save_folder)) {$settings.custom_save_folder=Join-Path $sourceFolder $settings.custom_save_folder}
    if($copiedImages -and $settings.local_favorites) {
        $settings.local_favorites=@($settings.local_favorites | ForEach-Object {
            if($_.StartsWith($images+'\',[StringComparison]::OrdinalIgnoreCase)){(Join-Path $data 'images')+$_.Substring($images.Length)}else{$_}
        })
    }
    [IO.File]::WriteAllText((Join-Path $data 'settings-rust.json'),($settings | ConvertTo-Json -Depth 10),(New-Object System.Text.UTF8Encoding($false)))
}
[IO.File]::WriteAllText((Join-Path $program '.installed'),'WaifuGallery')
$menu=Join-Path ([Environment]::GetFolderPath('Programs')) 'Waifu Gallery'
New-Item -ItemType Directory -Force -Path $menu | Out-Null
$shell=New-Object -ComObject WScript.Shell
$shortcut=$shell.CreateShortcut((Join-Path $menu 'Waifu Gallery.lnk'))
$shortcut.TargetPath=$target
$shortcut.WorkingDirectory=$program
$shortcut.IconLocation="$target,0"
$shortcut.Save()
$key='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\WaifuGallery.Desktop'
New-Item -Force -Path $key | Out-Null
New-ItemProperty -Path $key -Name DisplayName -Value 'Waifu Gallery' -PropertyType String -Force | Out-Null
New-ItemProperty -Path $key -Name DisplayVersion -Value '2.2.0' -PropertyType String -Force | Out-Null
New-ItemProperty -Path $key -Name Publisher -Value 'Waifu Gallery' -PropertyType String -Force | Out-Null
New-ItemProperty -Path $key -Name InstallLocation -Value $program -PropertyType String -Force | Out-Null
New-ItemProperty -Path $key -Name DisplayIcon -Value $target -PropertyType String -Force | Out-Null
New-ItemProperty -Path $key -Name UninstallString -Value ('"'+$target+'" --uninstall') -PropertyType String -Force | Out-Null
New-ItemProperty -Path $key -Name NoModify -Value 1 -PropertyType DWord -Force | Out-Null
New-ItemProperty -Path $key -Name NoRepair -Value 1 -PropertyType DWord -Force | Out-Null
