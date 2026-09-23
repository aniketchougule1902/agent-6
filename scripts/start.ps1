param([switch]$SkipBuild)
$ErrorActionPreference='Stop'
$repoRoot=Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $repoRoot
foreach($tool in @('cargo','node','npm.cmd','python')) {if(-not(Get-Command $tool -ErrorAction SilentlyContinue)){throw "Missing prerequisite: $tool"}}
if(-not(Test-Path -LiteralPath '.env')){Copy-Item -LiteralPath '.env.example' -Destination '.env'}
New-Item -ItemType Directory -Force -Path 'data/logs' | Out-Null
$enginePort=Get-NetTCPConnection -LocalPort 8787 -State Listen -ErrorAction SilentlyContinue
$uiPort=Get-NetTCPConnection -LocalPort 5173 -State Listen -ErrorAction SilentlyContinue
if($enginePort -and $uiPort){$health=Invoke-RestMethod 'http://127.0.0.1:8787/api/health';if(-not $health.services){throw 'Port occupied by another service'};Write-Host 'Already running: http://127.0.0.1:5173';exit 0}
if(-not $SkipBuild){
 & cargo build --release -p agent6-engine
 if($LASTEXITCODE -ne 0){throw 'Engine build failed'}
 Push-Location 'apps/ui'
 try{& npm.cmd ci;if($LASTEXITCODE -ne 0){throw 'npm ci failed'};& npm.cmd run build;if($LASTEXITCODE -ne 0){throw 'UI build failed'}}finally{Pop-Location}
}
$owned=@()
if(-not $enginePort){
 $binary=Join-Path $repoRoot 'target/release/agent6-engine.exe'
 $p=Start-Process -FilePath $binary -WorkingDirectory $repoRoot -WindowStyle Hidden -RedirectStandardOutput "$repoRoot/data/logs/engine.out.log" -RedirectStandardError "$repoRoot/data/logs/engine.err.log" -PassThru
 $owned+=@{pid=$p.Id;path=$binary}
}
if(-not $uiPort){
 $node=(Get-Command node).Source
 $vite=Join-Path $repoRoot 'apps/ui/node_modules/vite/bin/vite.js'
 $p=Start-Process -FilePath $node -ArgumentList @(('"'+$vite+'"'),'preview','--host','127.0.0.1','--port','5173','--strictPort') -WorkingDirectory "$repoRoot/apps/ui" -WindowStyle Hidden -RedirectStandardOutput "$repoRoot/data/logs/ui.out.log" -RedirectStandardError "$repoRoot/data/logs/ui.err.log" -PassThru
 $owned+=@{pid=$p.Id;path=$node}
}
ConvertTo-Json -InputObject @($owned) | Set-Content -Encoding UTF8 -LiteralPath 'data/launcher-processes.json'
$ready=$false
for($i=0;$i -lt 30;$i++){try{$null=Invoke-RestMethod 'http://127.0.0.1:8787/api/health';$null=Invoke-WebRequest 'http://127.0.0.1:5173' -UseBasicParsing;$ready=$true;break}catch{Start-Sleep -Seconds 1}}
if(-not $ready){throw 'Startup failed; inspect data/logs'}
Write-Host 'Agent-6 running: http://127.0.0.1:5173'
