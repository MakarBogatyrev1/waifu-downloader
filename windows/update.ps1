param([int]$OldPid,[string]$Target,[string]$Staged,[string]$ExpectedDigest,[string]$DataDir,[switch]$Restart)
$ErrorActionPreference='Stop'
try {
    if(Get-Process -Id $OldPid -ErrorAction SilentlyContinue) {Wait-Process -Id $OldPid -Timeout 120}
    $stream=[IO.File]::OpenRead($Staged)
    $sha=[Security.Cryptography.SHA256]::Create()
    try {$actual=([BitConverter]::ToString($sha.ComputeHash($stream))).Replace('-','')} finally {$stream.Dispose();$sha.Dispose()}
    if($actual -ne $ExpectedDigest) {throw 'Update checksum mismatch'}
    $next=$Target+'.next'
    $backup=$Target+'.previous'
    Copy-Item -LiteralPath $Staged -Destination $next -Force
    Move-Item -LiteralPath $Target -Destination $backup -Force
    try {
        Move-Item -LiteralPath $next -Destination $Target -Force
        if($Restart) {
            $quotedData='"'+($DataDir -replace '(\+)$','$1$1')+'"'
            Start-Process -FilePath $Target -ArgumentList @('--data-dir',$quotedData) -WindowStyle Hidden
        }
        Remove-Item -LiteralPath $Staged -Force
    } catch {
        Copy-Item -LiteralPath $backup -Destination $Target -Force
        throw
    }
} catch {
    if(Test-Path -LiteralPath $DataDir) {[IO.File]::WriteAllText((Join-Path $DataDir 'update-error.txt'),$_.Exception.Message)}
} finally {
    Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue
}
