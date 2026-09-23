$ErrorActionPreference='Stop'
$repoRoot=Split-Path -Parent $PSScriptRoot
$record=Join-Path $repoRoot 'data/launcher-processes.json'
if(-not(Test-Path -LiteralPath $record)){Write-Host 'No launcher-owned processes. Stop other terminals with Ctrl+C.';exit}
$owned=Get-Content -Raw -LiteralPath $record | ConvertFrom-Json
foreach($item in $owned){
 $p=Get-CimInstance Win32_Process -Filter "ProcessId = $($item.pid)" -ErrorAction SilentlyContinue
 if($p -and $p.ExecutablePath -eq $item.path -and $p.CommandLine -like "*$repoRoot*"){Stop-Process -Id $p.ProcessId}
}
Write-Host 'Stopped matching launcher-owned services. Paper account preserved.'
