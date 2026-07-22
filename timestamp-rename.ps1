# PowerShell script for timestamp rename
param([string]$targetPath)

$imgExt = @('.jxl', '.jpg', '.jpeg', '.png', '.gif', '.bmp')

if (-not (Test-Path $targetPath)) {
    Write-Host "Error: Folder not found"
    exit 1
}

Write-Host ""
Write-Host "Processing..."
Write-Host ""

Get-ChildItem -LiteralPath $targetPath -File -Recurse | 
Where-Object { $imgExt -contains $_.Extension.ToLower() -and $_.BaseName -notmatch '^[0-9]{14}' } | 
ForEach-Object {
    $b = $_.LastWriteTime.ToString('yyyyMMddHHmmss')
    $ext = $_.Extension
    $n = $b + $ext
    $counter = 0
    
    while ((Test-Path (Join-Path $_.DirectoryName $n)) -and $counter -lt 100) {
        $counter++
        $r = Get-Random -Minimum 10 -Maximum 1000
        $n = $b + $r + $ext
    }
    
    if (Test-Path (Join-Path $_.DirectoryName $n)) {
        Write-Host "SKIP: Cannot find unique name for $($_.Name)"
        return
    }
    
    Write-Host ">> Renaming: $($_.Name) -> $n"
    Rename-Item -LiteralPath $_.FullName -NewName $n -ErrorAction SilentlyContinue
}

Write-Host ""
Write-Host "Timestamp rename completed!"
